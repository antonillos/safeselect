use crate::config::AuditConfig;
use crate::error::{Result, SafeselectError};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub mcp_client: String,
    pub project: String,
    pub environment: String,
    pub category: String,
    pub decision: String,
    pub query_hash: String,
}

pub struct AuditLog {
    writer: std::io::BufWriter<std::fs::File>,
    config: AuditConfig,
    project: String,
    environment: String,
    mcp_client: String,
    current_path: PathBuf,
    bytes_written: u64,
}

impl AuditLog {
    pub fn open(
        config: &AuditConfig,
        project: &str,
        environment: &str,
        mcp_client: &str,
    ) -> Result<Self> {
        if !config.enabled {
            return Err(SafeselectError::Audit(
                "audit is disabled but must be enabled for security".into(),
            ));
        }

        let dir = expand_tilde(&config.directory);
        let audit_dir = PathBuf::from(&dir).join(project).join(environment);
        std::fs::create_dir_all(&audit_dir)?;

        let filename = format!("{}.jsonl", Utc::now().format("%Y%m%d-%H%M%S-%f"));
        let path = audit_dir.join(&filename);
        let file = std::fs::File::create_new(&path).map_err(|e| {
            SafeselectError::Audit(format!("cannot create audit file {}: {e}", path.display()))
        })?;

        let writer = std::io::BufWriter::new(file);

        Ok(Self {
            writer,
            config: config.clone(),
            project: project.to_string(),
            environment: environment.to_string(),
            mcp_client: mcp_client.to_string(),
            current_path: path,
            bytes_written: 0,
        })
    }

    pub fn record(&mut self, category: &str, decision: &str, sql: &str) -> Result<()> {
        let query_hash = self.hash_sql(sql);
        let entry = AuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            mcp_client: self.mcp_client.clone(),
            project: self.project.clone(),
            environment: self.environment.clone(),
            category: category.to_string(),
            decision: decision.to_string(),
            query_hash,
        };

        let line = serde_json::to_string(&entry)?;
        let line_bytes = (line.len() + 1) as u64;

        if self.bytes_written + line_bytes > self.config.max_file_bytes {
            self.rotate()?;
        }

        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        self.bytes_written += line_bytes;

        Ok(())
    }

    fn rotate(&mut self) -> Result<()> {
        self.writer.flush()?;
        let dir = self.current_path.parent().unwrap().to_path_buf();
        let filename = format!("{}.jsonl", Utc::now().format("%Y%m%d-%H%M%S-%f"));
        let path = dir.join(&filename);
        let file = std::fs::File::create_new(&path)
            .map_err(|e| SafeselectError::Audit(format!("cannot rotate audit file: {e}")))?;
        self.writer = std::io::BufWriter::new(file);
        self.current_path = path;
        self.bytes_written = 0;

        self.cleanup_old(&dir)?;
        Ok(())
    }

    fn cleanup_old(&self, dir: &std::path::Path) -> Result<()> {
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
            .collect();
        files.sort_by_key(|e| e.path());

        while files.len() > self.config.retain_files as usize {
            if let Some(oldest) = files.first() {
                let _ = std::fs::remove_file(oldest.path());
                files.remove(0);
            }
        }
        Ok(())
    }

    fn hash_sql(&self, sql: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(sql.as_bytes());
        hex::encode(hasher.finalize())
    }
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), rest);
        }
    }
    if let Some(_rest) = path.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.display().to_string();
        }
    }
    path.to_string()
}
