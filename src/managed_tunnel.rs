use crate::config::{EnvironmentConfig, SshConfig};
use crate::error::{Result, SafeselectError};
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// An authenticated master owns the forwarding; control requests acknowledge binds.
pub struct ManagedTunnel {
    child: Child,
    directory: PathBuf,
    socket: PathBuf,
    ssh: SshConfig,
    port: u16,
    program: PathBuf,
    stderr: Option<std::thread::JoinHandle<&'static str>>,
}

pub fn is_managed(ssh: &SshConfig) -> bool {
    ssh.enabled && (ssh.forward_host.is_some() || ssh.forward_port.is_some())
}

fn failure(message: &str) -> SafeselectError {
    SafeselectError::Other(format!("SSH tunnel setup failed: {message}"))
}

impl ManagedTunnel {
    pub fn start(ssh: &SshConfig, fallback_account: &str) -> Result<Self> {
        Self::start_with_program(ssh, fallback_account, Path::new("ssh"))
    }

    fn start_with_program(ssh: &SshConfig, fallback_account: &str, program: &Path) -> Result<Self> {
        if !crate::missing_ssh_fields(ssh).is_empty() {
            return Err(failure("SSH forwarding configuration is incomplete."));
        }
        let directory = control_directory()?;
        let socket = directory.join("ctl");
        let mut child = match spawn_master(ssh, fallback_account, program, &socket) {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        let pipe = child.stderr.take().unwrap();
        let stderr = Some(std::thread::spawn(move || read_failure(pipe)));
        let mut tunnel = Self {
            child,
            directory,
            socket,
            ssh: ssh.clone(),
            port: 0,
            program: program.to_path_buf(),
            stderr,
        };
        tunnel.wait_for_master()?;
        tunnel.open_forwarding()?;
        Ok(tunnel)
    }

    fn wait_for_master(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(self.startup_failure(status.code()));
            }
            if self.control("check").status()?.success() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(failure(
                    crate::ssh_diagnostics::TUNNEL_ENDPOINT_TIMEOUT_MESSAGE,
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn startup_failure(&mut self, code: Option<i32>) -> SafeselectError {
        let detail = self
            .stderr
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or("SSH tunnel establishment failed.");
        let detail = if crate::ssh_diagnostics::recognized_failure_message(detail).is_some() {
            detail
        } else {
            crate::ssh_diagnostics::failure_message(code, "", crate::ssh_uses_password(&self.ssh))
        };
        failure(detail)
    }

    fn open_forwarding(&mut self) -> Result<()> {
        // The OS chooses a candidate. SSH must acknowledge the bind; retry races.
        for _ in 0..8 {
            if let Some(port) = self.try_forwarding()? {
                self.port = port;
                return Ok(());
            }
            if self.child.try_wait()?.is_some() {
                return Err(failure("SSH tunnel establishment failed."));
            }
        }
        Err(failure(crate::ssh_diagnostics::failure_message(
            None,
            "address already in use",
            false,
        )))
    }

    fn try_forwarding(&self) -> Result<Option<u16>> {
        let reservation = TcpListener::bind(("127.0.0.1", 0))?;
        let port = reservation.local_addr()?.port();
        drop(reservation);
        let target = format!(
            "127.0.0.1:{port}:{}:{}",
            self.ssh.forward_host.as_deref().unwrap(),
            self.ssh.forward_port.unwrap()
        );
        let output = self.control("forward").args(["-L", &target]).output()?;
        Ok(output.status.success().then_some(port))
    }

    fn control(&self, operation: &str) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args([
                "-F",
                "none",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=2",
                "-S",
                self.socket.to_str().unwrap(),
                "-O",
                operation,
                "-p",
                &self.ssh.port.unwrap_or(22).to_string(),
            ])
            .arg(format!(
                "{}@{}",
                self.ssh.username.as_deref().unwrap(),
                self.ssh.host.as_deref().unwrap()
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    pub fn is_alive(&mut self, ssh: &SshConfig) -> Result<bool> {
        Ok(self.ssh == *ssh
            && self.child.try_wait()?.is_none()
            && self.control("check").status()?.success())
    }

    pub fn apply(&self, environment: &mut EnvironmentConfig) -> Result<()> {
        environment.database.url = forwarded_url(&environment.database.url, self.port)?;
        if let Some(ssh) = environment.ssh.as_mut() {
            ssh.local_host = Some("127.0.0.1".into());
            ssh.local_port = Some(self.port);
        }
        Ok(())
    }
}

impl Drop for ManagedTunnel {
    fn drop(&mut self) {
        let _ = self.control("exit").status();
        #[cfg(unix)]
        if matches!(self.child.try_wait(), Ok(None)) {
            // An unreaped child reserves this process-group ID against reuse.
            // sshpass may have spawned ssh; terminate the owned group as well.
            unsafe {
                libc::killpg(self.child.id() as i32, libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.stderr.take() {
            let _ = reader.join();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn forwarded_url(url: &str, port: u16) -> Result<String> {
    let start = url
        .find("://")
        .map(|index| index + 3)
        .ok_or_else(|| failure("SSH forwarding requires a single-host database URL."))?;
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |index| start + index);
    let authority = &url[start..end];
    let (userinfo, host) = authority
        .rsplit_once('@')
        .map_or(("", authority), |(user, host)| {
            (&authority[..user.len() + 1], host)
        });
    if host.is_empty() || host.contains(',') || url.starts_with("mongodb+srv:") {
        return Err(failure(
            "SSH forwarding requires a single-host database URL.",
        ));
    }
    Ok(format!(
        "{}{userinfo}127.0.0.1:{port}{}",
        &url[..start],
        &url[end..]
    ))
}

fn control_directory() -> Result<PathBuf> {
    // Keep Unix socket paths below the platform limit, including on macOS.
    let temporary = if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let directory = temporary.join(format!("ss-{}", uuid::Uuid::new_v4().simple()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    Ok(directory)
}

fn spawn_master(
    ssh: &SshConfig,
    fallback_account: &str,
    program: &Path,
    socket: &Path,
) -> Result<Child> {
    let mut args = crate::build_tunnel_ssh_args(ssh);
    let forwarding = args.iter().position(|arg| arg == "-L").unwrap();
    args.drain(forwarding..forwarding + 2);
    args.extend([
        "-M".into(),
        "-S".into(),
        socket.to_string_lossy().into_owned(),
        "-o".into(),
        "ControlMaster=yes".into(),
        "-o".into(),
        "ControlPersist=no".into(),
        "-o".into(),
        "ClearAllForwardings=yes".into(),
    ]);
    let mut command = if crate::ssh_uses_password(ssh) {
        let password = crate::resolve_tunnel_password(ssh, fallback_account)?;
        let mut command = Command::new("sshpass");
        command.arg("-e").arg(program).env("SSHPASS", password);
        command
    } else {
        let mut command = Command::new(program);
        command.args(["-o", "BatchMode=yes"]);
        command
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Ok(command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?)
}

fn read_failure(mut pipe: impl Read) -> &'static str {
    let mut buffer = [0; 4096];
    let mut tail = String::new();
    let mut message = "SSH tunnel establishment failed.";
    while let Ok(count) = pipe.read(&mut buffer) {
        if count == 0 {
            break;
        }
        tail.push_str(&String::from_utf8_lossy(&buffer[..count]));
        let classified = crate::ssh_diagnostics::failure_message(None, &tail, false);
        if crate::ssh_diagnostics::recognized_failure_message(classified).is_some() {
            message = classified;
        }
        // Only retain enough context for a diagnostic split across reads.
        tail = tail
            .chars()
            .rev()
            .take(256)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    message
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use std::net::TcpStream;
    use std::os::unix::fs::PermissionsExt;

    pub(crate) struct Fixture(pub(crate) PathBuf);

    impl Fixture {
        pub(crate) fn new() -> Self {
            let directory = PathBuf::from("/tmp").join(format!("ss-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let program = directory.join("ssh");
            std::fs::write(&program, include_str!("../tests/fixtures/managed_ssh.py")).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(directory)
        }

        pub(crate) fn start(&self, ssh: &SshConfig) -> Result<ManagedTunnel> {
            ManagedTunnel::start_with_program(ssh, "unused", &self.0.join("ssh"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn environment(destination: &str) -> EnvironmentConfig {
        toml::from_str(&format!(
            r#"
version = 1
[database]
kind = "document"
vendor = "mongodb"
url = "mongodb://agent:__SAFESELECT_PASSWORD__@localhost:15433/app?tls=true&directConnection=true"
[ssh]
enabled = true
host = "bastion.invalid"
username = "agent"
local_host = "localhost"
local_port = 15433
forward_host = "{destination}"
forward_port = 27017
[limits]
"#
        ))
        .unwrap()
    }

    fn destination(port: u16) -> String {
        let mut connection = TcpStream::connect(("127.0.0.1", port)).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut value = String::new();
        connection.read_to_string(&mut value).unwrap();
        value
    }

    #[test]
    fn concurrent_tunnels_ignore_legacy_port_and_close_only_their_own_listener() {
        let fixture = Fixture::new();
        let legacy = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut first_config = environment("first.invalid");
        let mut second_config = environment("second.invalid");
        for config in [&mut first_config, &mut second_config] {
            config.ssh.as_mut().unwrap().local_port = Some(legacy.local_addr().unwrap().port());
        }
        let original = toml::to_string(&first_config).unwrap();
        let first = fixture.start(first_config.ssh.as_ref().unwrap()).unwrap();
        let mut second = fixture.start(second_config.ssh.as_ref().unwrap()).unwrap();
        assert_ne!(first.port, second.port);
        assert_ne!(first.port, legacy.local_addr().unwrap().port());
        assert_eq!(destination(first.port), "first.invalid");
        assert_eq!(destination(second.port), "second.invalid");
        assert_eq!(toml::to_string(&first_config).unwrap(), original);
        let mut effective = first_config.clone();
        first.apply(&mut effective).unwrap();
        assert!(effective
            .database
            .url
            .contains(&format!("@127.0.0.1:{}/app?tls=true", first.port)));
        assert_eq!(effective.ssh.unwrap().local_port, Some(first.port));
        let first_port = first.port;
        let directory = first.directory.clone();
        drop(first);
        assert!(TcpStream::connect(("127.0.0.1", first_port)).is_err());
        assert!(!directory.exists());
        assert_eq!(destination(second.port), "second.invalid");
        assert!(second
            .is_alive(second_config.ssh.as_ref().unwrap())
            .unwrap());
        second_config.ssh.as_mut().unwrap().forward_host = Some("changed.invalid".into());
        assert!(!second
            .is_alive(second_config.ssh.as_ref().unwrap())
            .unwrap());
        assert!(legacy.local_addr().is_ok());
    }

    #[test]
    fn retries_bind_races_and_rejects_unacknowledged_forwarding() {
        let fixture = Fixture::new();
        let mut config = environment("remote.invalid");
        let ssh = config.ssh.as_mut().unwrap();
        ssh.host = Some("race-once.invalid".into());
        let tunnel = fixture.start(ssh).unwrap();
        assert_eq!(destination(tunnel.port), "remote.invalid");
        ssh.host = Some("reject-forward.invalid".into());
        assert!(fixture.start(ssh).is_err());
        assert_eq!(destination(tunnel.port), "remote.invalid");
    }

    #[test]
    fn rejects_authentication_failure_and_reports_only_its_category() {
        let fixture = Fixture::new();
        let mut config = environment("remote.invalid");
        let ssh = config.ssh.as_mut().unwrap();
        ssh.host = Some("reject-auth.invalid".into());
        let error = fixture.start(ssh).err().unwrap().to_string();
        assert!(error.contains("SSH authentication failed"));
        assert!(!error.contains("reject-auth"));
        assert!(!error.contains("agent"));
    }

    #[test]
    fn rewrites_only_single_host_authority_and_preserves_credentials_and_options() {
        for (input, expected) in [
            (
                "jdbc:postgresql://localhost:15433/app?sslmode=require",
                "jdbc:postgresql://127.0.0.1:49152/app?sslmode=require",
            ),
            (
                "mongodb://user:p%40ss@[::1]:15433/app?tls=true",
                "mongodb://user:p%40ss@127.0.0.1:49152/app?tls=true",
            ),
        ] {
            assert_eq!(forwarded_url(input, 49152).unwrap(), expected);
        }
        for input in [
            "jdbc:invalid",
            "mongodb://",
            "mongodb://host1,host2/app",
            "mongodb+srv://cluster.invalid/app",
        ] {
            assert!(forwarded_url(input, 49152).is_err());
        }
    }

    #[test]
    fn external_tunnels_remain_external_and_partial_forwarding_is_rejected() {
        let fixture = Fixture::new();
        let mut config = environment("remote.invalid");
        let ssh = config.ssh.as_mut().unwrap();
        ssh.forward_host = None;
        ssh.forward_port = None;
        assert!(!is_managed(ssh));
        ssh.forward_port = Some(27017);
        assert!(is_managed(ssh));
        assert!(fixture.start(ssh).is_err());
        ssh.enabled = false;
        assert!(!is_managed(ssh));
    }
}
