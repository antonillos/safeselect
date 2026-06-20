use crate::error::{Result, SafeselectError};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(Serialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Response {
    id: Option<serde_json::Value>,
    #[serde(default)]
    ok: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<ResponseError>,
    #[serde(default)]
    r#type: Option<String>,
}

#[derive(Deserialize)]
struct ResponseError {
    code: String,
    message: String,
}

pub struct SidecarProcess {
    child: Child,
    writer: BufWriter<ChildStdin>,
    reader: BufReader<ChildStdout>,
    next_id: u64,
    statement_timeout_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: u64,
    pub byte_count: u64,
    #[serde(default)]
    pub elapsed_ms: u64,
}

impl SidecarProcess {
    pub fn start(
        driver_path: &str,
        driver_class: &str,
        jdbc_url: &str,
        username: &str,
        password: &str,
    ) -> Result<Self> {
        Self::start_with_timeout(
            driver_path,
            driver_class,
            jdbc_url,
            username,
            password,
            0,
            0,
            false,
        )
    }

    pub fn start_with_timeout(
        driver_path: &str,
        driver_class: &str,
        jdbc_url: &str,
        username: &str,
        password: &str,
        idle_timeout_seconds: u64,
        statement_timeout_ms: u64,
        verbose: bool,
    ) -> Result<Self> {
        let jar_path = Self::ensure_sidecar_jar()?;
        let cp = format!("{}:{}", jar_path.display(), driver_path);

        let mut args = vec![
            "-cp",
            cp.as_str(),
            "com.safeselect.Main",
            "--driver",
            driver_class,
            "--url",
            jdbc_url,
            "--user",
            username,
            "--password-stdin",
        ];
        if idle_timeout_seconds > 0 {
            args.push("--idle-timeout-seconds");
            args.push(Box::leak(idle_timeout_seconds.to_string().into_boxed_str()));
        }
        if statement_timeout_ms > 0 {
            args.push("--statement-timeout-ms");
            args.push(Box::leak(statement_timeout_ms.to_string().into_boxed_str()));
        }
        if verbose {
            args.push("--verbose");
        }

        let mut child = Command::new("java")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| SafeselectError::Sidecar(format!("failed to start Java: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| SafeselectError::Sidecar("failed to capture stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SafeselectError::Sidecar("failed to capture stdout".into()))?;

        let mut proc = Self {
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            child,
            next_id: 0,
            statement_timeout_ms,
        };

        proc.send_password(password)?;
        proc.ping()?;
        Ok(proc)
    }

    fn send_password(&mut self, password: &str) -> Result<()> {
        writeln!(self.writer, "{password}")?;
        self.writer.flush()?;
        let mut ack = String::new();
        self.reader.read_line(&mut ack)?;
        if ack.is_empty() {
            return Err(SafeselectError::Sidecar(
                "sidecar process terminated during startup — JDBC connection failed".into(),
            ));
        }
        let ack = ack.trim();
        if ack != "ready" {
            return Err(SafeselectError::Sidecar(format!(
                "sidecar password rejected: {ack}"
            )));
        }
        Ok(())
    }

    fn ensure_sidecar_jar() -> Result<PathBuf> {
        // First, try to use the JAR from the build directory (for development)
        let build_jar = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("sidecar")
            .join("target")
            .join("safeselect-sidecar.jar");

        if build_jar.exists() {
            return Ok(build_jar);
        }

        // Fallback to embedded JAR (for production)
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("safeselect")
            .join("sidecar");

        let jar_path = data_dir.join("safeselect-sidecar.jar");

        if !jar_path.exists() {
            let embedded = include_bytes!("../sidecar/target/safeselect-sidecar.jar");
            std::fs::create_dir_all(&data_dir)?;
            std::fs::write(&jar_path, embedded)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = jar_path.metadata()?.permissions();
                perms.set_mode(0o644);
                std::fs::set_permissions(&jar_path, perms)?;
            }
        }

        Ok(jar_path)
    }

    pub fn ping(&mut self) -> Result<()> {
        let resp = self.send_request("ping", None)?;
        match resp.ok {
            Some(val) if val == "pong" => Ok(()),
            _ => Err(SafeselectError::Sidecar("ping failed".into())),
        }
    }

    pub fn execute(&mut self, sql: &str) -> Result<QueryResult> {
        let start = std::time::Instant::now();
        tracing::debug!("Sidecar execute started");

        let params = serde_json::json!({"sql": sql});
        let resp = self.send_request("execute", Some(params))?;

        tracing::debug!(
            "Sidecar execute send_request completed ({:?})",
            start.elapsed()
        );

        if let Some(err) = resp.error {
            return Err(SafeselectError::Sidecar(format!(
                "JDBC error [{}]: {}",
                err.code, err.message
            )));
        }

        match resp.ok {
            Some(val) => {
                let result: QueryResult = serde_json::from_value(val)?;
                tracing::debug!("Sidecar execute completed ({:?})", start.elapsed());
                Ok(result)
            }
            None => Err(SafeselectError::Sidecar(
                "empty response from sidecar".into(),
            )),
        }
    }

    fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;

        let req = Request {
            id,
            method: method.to_string(),
            params,
        };

        let line = serde_json::to_string(&req)?;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        let fd = self.reader.get_ref().as_raw_fd();
        // Wait for statement timeout + 1s buffer, with a short minimum so broken
        // tunnels fail fast instead of looking stuck to MCP clients.
        // The 1s buffer allows PostgreSQL to cancel the query via statement_timeout
        // before we kill the sidecar process
        let timeout_ms = if self.statement_timeout_ms > 0 {
            let t = (self.statement_timeout_ms + 1_000u64).max(5_000u64);
            if t > i32::MAX as u64 {
                i32::MAX
            } else {
                t as i32
            }
        } else {
            30_000i32
        };

        loop {
            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ret = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
            match ret {
                -1 => {
                    let err = std::io::Error::last_os_error();
                    // EINTR = interrupted by signal, retry
                    if err.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return Err(SafeselectError::Sidecar(format!("poll error: {err}")));
                }
                0 => {
                    return Err(SafeselectError::Sidecar(format!(
                        "sidecar did not respond within {timeout_ms}ms — restarting"
                    )));
                }
                _ => {}
            }

            let mut response_line = String::new();
            self.reader.read_line(&mut response_line)?;

            if response_line.is_empty() {
                return Err(SafeselectError::Sidecar(
                    "sidecar process terminated".into(),
                ));
            }

            let resp: Response = serde_json::from_str(&response_line)?;
            // Skip async notifications (idle_disconnect, etc.) that have no id
            if resp.r#type.is_some() && resp.id.is_none() {
                continue;
            }
            return Ok(resp);
        }
    }

    pub fn disconnect(&mut self) -> Result<()> {
        let resp = self.send_request("disconnect", None)?;
        if let Some(err) = resp.error {
            return Err(SafeselectError::Sidecar(format!(
                "disconnect failed [{}]: {}",
                err.code, err.message
            )));
        }
        Ok(())
    }

    pub fn connect(&mut self) -> Result<()> {
        let resp = self.send_request("connect", None)?;
        if let Some(err) = resp.error {
            return Err(SafeselectError::Sidecar(format!(
                "connect failed [{}]: {}",
                err.code, err.message
            )));
        }
        Ok(())
    }

    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.send_request("shutdown", None);
        let _ = self.child.wait();
        Ok(())
    }

    /// Force kill the sidecar without trying to send a shutdown request.
    /// Use this when the sidecar is hung or unresponsive.
    pub fn force_kill(mut self) -> Result<()> {
        tracing::warn!("Force killing sidecar process (PID: {})", self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }

    /// Force kill the sidecar without consuming self.
    /// Use this when restarting after a timeout.
    pub fn force_kill_ref(&mut self) {
        tracing::warn!("Force killing sidecar process (PID: {})", self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
