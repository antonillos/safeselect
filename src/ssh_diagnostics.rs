use crate::config::{ProjectConfig, SharedSshConfig, SshConfig};
use std::process::Child;
use std::time::{Duration, Instant};

pub const REUSED_ENDPOINT_MESSAGE: &str = "Reusing an existing database endpoint; no SSH tunnel was established by this check. Endpoint responsiveness does not verify the configured remote destination.";

#[derive(Debug, PartialEq, Eq)]
pub enum TunnelStatus {
    Ready,
    Exited,
    TimedOut,
}

pub fn wait_for_tunnel(
    child: &mut Child,
    timeout: Duration,
    mut probe: impl FnMut() -> bool,
) -> std::io::Result<TunnelStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            return Ok(TunnelStatus::Exited);
        }
        if probe() && child.try_wait()?.is_none() {
            return Ok(TunnelStatus::Ready);
        }
        std::thread::sleep(
            Duration::from_millis(250).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    Ok(TunnelStatus::TimedOut)
}

pub fn failure_message(code: Option<i32>, stderr: &str, password_auth: bool) -> &'static str {
    let detail = stderr.to_ascii_lowercase();
    if (password_auth && matches!(code, Some(6 | 7)))
        || detail.contains("host key verification failed")
        || detail.contains("remote host identification has changed")
    {
        "SSH host key verification failed. Check the configured known-hosts file. Database authentication was not attempted."
    } else if (password_auth && code == Some(5))
        || detail.contains("permission denied")
        || detail.contains("authentication failed")
    {
        "SSH authentication failed for the configured bastion. Check its SSH credential source. Database authentication was not attempted."
    } else if detail.contains("address already in use") || detail.contains("cannot listen to port")
    {
        "SSH local forwarding port is unavailable. Check for another process using the configured local endpoint. Database authentication was not attempted."
    } else if detail.contains("connection timed out") || detail.contains("operation timed out") {
        "SSH connection timed out. Check bastion connectivity. Database authentication was not attempted."
    } else if detail.contains("connection refused")
        || detail.contains("could not resolve hostname")
        || detail.contains("no route to host")
        || detail.contains("network is unreachable")
    {
        "SSH network connection failed. Check bastion connectivity. Database authentication was not attempted."
    } else {
        "SSH tunnel establishment failed. Check the SSH configuration and forwarding destination. Database authentication was not attempted."
    }
}

fn credential_source(shared: &SharedSshConfig) -> (&str, Option<&str>) {
    if let Some(variable) = shared.secret_variable.as_deref() {
        ("env", Some(variable))
    } else {
        ("keychain", shared.secret_account.as_deref())
    }
}

pub fn duplicate_bastion_warning(project: &ProjectConfig, ssh: &SshConfig) -> Option<String> {
    let selected_name = ssh.bastion.as_deref()?;
    let selected = project.ssh_bastions.get(selected_name)?;
    selected.host.as_deref().filter(|host| !host.is_empty())?;
    selected
        .username
        .as_deref()
        .filter(|user| !user.is_empty())?;
    let duplicates: Vec<_> = project
        .ssh_bastions
        .iter()
        .filter(|(name, other)| {
            name.as_str() != selected_name
                && other.host == selected.host
                && other.port.unwrap_or(22) == selected.port.unwrap_or(22)
                && other.username == selected.username
                && credential_source(other) != credential_source(selected)
        })
        .map(|(name, _)| format!("{name:?}"))
        .collect();
    if duplicates.is_empty() {
        return None;
    }
    Some(format!(
        "Selected SSH bastion {selected_name:?}; definitions {} use the same SSH host, port and username with different credential references. Check the selected bastion's credential source and any environment override. Definitions and secrets are unchanged.",
        duplicates.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ssh_failures_without_repeating_private_details() {
        for (code, detail, expected) in [
            (
                5,
                "synthetic-private-user@synthetic-private-host",
                "SSH authentication failed",
            ),
            (
                255,
                "Permission denied (password): synthetic-private-password",
                "SSH authentication failed",
            ),
            (6, "synthetic-private-host", "host key verification failed"),
            (7, "synthetic-private-host", "host key verification failed"),
            (
                255,
                "REMOTE HOST IDENTIFICATION HAS CHANGED: synthetic-private-host",
                "host key verification failed",
            ),
            (
                255,
                "bind synthetic-private-host: Address already in use",
                "forwarding port is unavailable",
            ),
            (
                255,
                "synthetic-private-host: Connection timed out",
                "SSH connection timed out",
            ),
            (
                255,
                "synthetic-private-host: Connection refused",
                "SSH network connection failed",
            ),
            (
                1,
                "synthetic-private-host synthetic-private-password",
                "SSH tunnel establishment failed",
            ),
        ] {
            let message = failure_message(Some(code), detail, true);
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains("synthetic-private"));
            assert!(message.contains("Database authentication was not attempted"));
        }
        assert!(!failure_message(Some(5), "", false).contains("authentication failed"));
    }

    #[test]
    fn exited_ssh_child_does_not_wait_or_accept_an_unrelated_endpoint() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 5"])
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let start = Instant::now();
        let status = wait_for_tunnel(&mut child, Duration::from_secs(20), || {
            panic!("must not probe after SSH exit")
        })
        .unwrap();
        assert_eq!(status, TunnelStatus::Exited);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn living_ssh_child_requires_a_responsive_endpoint() {
        use std::process::Stdio;
        for (reachable, expected) in [(true, TunnelStatus::Ready), (false, TunnelStatus::TimedOut)]
        {
            let mut child = std::process::Command::new("sh")
                .args(["-c", "read request"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            let status =
                wait_for_tunnel(&mut child, Duration::from_millis(50), || reachable).unwrap();
            let _ = child.kill();
            child.wait().unwrap();
            assert_eq!(status, expected);
        }
    }

    #[test]
    fn shared_bastion_supports_distinct_database_destinations_without_warning() {
        let project: ProjectConfig = toml::from_str(
            r#"
version = 1
[ssh_bastions.shared]
host = "jump.example"
username = "tunnel"
secret_account = "synthetic-private-account"
"#,
        )
        .unwrap();
        for (host, port) in [("db-a.example", 15432), ("db-b.example", 15433)] {
            let ssh: SshConfig = toml::from_str(&format!(
                r#"
enabled = true
bastion = "shared"
local_port = {port}
forward_host = "{host}"
forward_port = 5432
"#
            ))
            .unwrap();
            assert!(duplicate_bastion_warning(&project, &ssh).is_none());
        }
    }

    #[test]
    fn warns_only_about_matching_ssh_identity_with_distinct_credential_sources() {
        let mut project: ProjectConfig = toml::from_str(
            r#"
version = 1
[ssh_bastions.selected]
host = "synthetic-private-host"
username = "synthetic-private-user"
secret_account = "synthetic-private-account-a"
[ssh_bastions.other]
host = "synthetic-private-host"
port = 22
username = "synthetic-private-user"
secret_account = "synthetic-private-account-b"
"#,
        )
        .unwrap();
        let ssh: SshConfig = toml::from_str("enabled = true\nbastion = 'selected'").unwrap();
        let message = duplicate_bastion_warning(&project, &ssh).unwrap();
        assert!(message.contains("\"selected\""));
        assert!(message.contains("\"other\""));
        assert!(!message.contains("synthetic-private"));
        project
            .ssh_bastions
            .get_mut("other")
            .unwrap()
            .secret_account = Some("synthetic-private-account-a".into());
        assert!(duplicate_bastion_warning(&project, &ssh).is_none());
        project
            .ssh_bastions
            .get_mut("other")
            .unwrap()
            .secret_variable = Some("SSH_PASSWORD".into());
        assert!(duplicate_bastion_warning(&project, &ssh).is_some());
        project.ssh_bastions.get_mut("other").unwrap().username = Some("different-user".into());
        assert!(duplicate_bastion_warning(&project, &ssh).is_none());
    }
}
