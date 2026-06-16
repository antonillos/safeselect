use crate::error::{Result, SafeselectError};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
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
    id: u64,
    #[serde(default)]
    ok: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<ResponseError>,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: u64,
    pub byte_count: u64,
}

impl SidecarProcess {
    pub fn start(driver_path: &str, driver_class: &str, jdbc_url: &str, username: &str, password: &str) -> Result<Self> {
        let jar_path = Self::ensure_sidecar_jar()?;

        let mut child = Command::new("java")
            .args([
                "-cp",
                &format!("{}:{}", jar_path.display(), driver_path),
                "com.safeselect.Main",
                "--driver",
                driver_class,
                "--url",
                jdbc_url,
                "--user",
                username,
                "--password",
                password,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| SafeselectError::Sidecar(format!("failed to start Java: {e}")))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            SafeselectError::Sidecar("failed to capture stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SafeselectError::Sidecar("failed to capture stdout".into())
        })?;

        let mut proc = Self {
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            child,
            next_id: 0,
        };

        proc.ping()?;
        Ok(proc)
    }

    fn ensure_sidecar_jar() -> Result<PathBuf> {
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
        let params = serde_json::json!({"sql": sql});
        let resp = self.send_request("execute", Some(params))?;

        if let Some(err) = resp.error {
            return Err(SafeselectError::Sidecar(format!(
                "JDBC error [{}]: {}",
                err.code, err.message
            )));
        }

        match resp.ok {
            Some(val) => {
                let result: QueryResult = serde_json::from_value(val)?;
                Ok(result)
            }
            None => Err(SafeselectError::Sidecar("empty response from sidecar".into())),
        }
    }

    fn send_request(&mut self, method: &str, params: Option<serde_json::Value>) -> Result<Response> {
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

        let mut response_line = String::new();
        self.reader.read_line(&mut response_line)?;

        if response_line.is_empty() {
            return Err(SafeselectError::Sidecar("sidecar process terminated".into()));
        }

        let resp: Response = serde_json::from_str(&response_line)?;
        Ok(resp)
    }

    pub fn shutdown(mut self) -> Result<()> {
        let _ = self.send_request("shutdown", None);
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
