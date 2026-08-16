use crate::config::AuditConfig;
use crate::error::{Result, SafeselectError};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub mcp_client: String,
    pub project: String,
    pub environment: String,
    pub category: String,
    pub decision: String,
    pub query_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<AuditDetails>,
}

#[derive(Clone, Serialize)]
pub struct AuditDetails {
    pub tool: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

pub struct AuditLog {
    writer: std::io::BufWriter<std::fs::File>,
    config: AuditConfig,
    project: String,
    environment: String,
    mcp_client: String,
    current_path: PathBuf,
    bytes_written: u64,
    session_entries: Vec<AuditEntry>,
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
            session_entries: Vec::new(),
        })
    }

    pub fn record(&mut self, category: &str, decision: &str, sql: &str) -> Result<()> {
        self.record_with_details(category, decision, sql, None)
    }

    pub fn record_with_details(
        &mut self,
        category: &str,
        decision: &str,
        sql: &str,
        details: Option<AuditDetails>,
    ) -> Result<()> {
        let query_hash = self.hash_sql(sql);
        let entry = AuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            mcp_client: self.mcp_client.clone(),
            project: self.project.clone(),
            environment: self.environment.clone(),
            category: category.to_string(),
            decision: decision.to_string(),
            query_hash,
            details,
        };

        let line = serde_json::to_string(&entry)?;
        let line_bytes = (line.len() + 1) as u64;

        if self.bytes_written + line_bytes > self.config.max_file_bytes {
            self.rotate()?;
        }

        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;
        self.bytes_written += line_bytes;
        self.session_entries.push(entry);
        if self.session_entries.len() > 20 {
            self.session_entries.remove(0);
        }

        Ok(())
    }

    pub fn session_entry_count(&self) -> usize {
        self.session_entries.len()
    }

    pub fn recent_session_entries(&self, limit: usize) -> Vec<AuditEntry> {
        let start = self.session_entries.len().saturating_sub(limit.min(20));
        self.session_entries[start..].to_vec()
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
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(directory: &std::path::Path) -> AuditConfig {
        AuditConfig {
            enabled: true,
            directory: directory.display().to_string(),
            max_file_bytes: 1_000_000,
            retain_files: 1,
        }
    }

    #[test]
    fn records_execution_metadata_without_query_text() {
        let directory =
            std::env::temp_dir().join(format!("safeselect-audit-{}", uuid::Uuid::new_v4()));
        let config = config(&directory);
        let mut audit = AuditLog::open(&config, "project", "testing", "test").unwrap();
        audit
            .record_with_details(
                "PASS",
                "allow",
                "SELECT secret FROM users",
                Some(AuditDetails {
                    tool: "select".into(),
                    elapsed_ms: 12,
                    row_count: Some(1),
                    byte_count: Some(42),
                    error_code: None,
                }),
            )
            .unwrap();
        drop(audit);

        let audit_file = std::fs::read_dir(directory.join("project/testing"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let entry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(audit_file).unwrap()).unwrap();
        assert_eq!(entry["details"]["tool"], "select");
        assert_eq!(entry["details"]["row_count"], 1);
        assert!(!entry.to_string().contains("SELECT secret FROM users"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retains_only_the_latest_twenty_session_entries() {
        let directory =
            std::env::temp_dir().join(format!("safeselect-audit-{}", uuid::Uuid::new_v4()));
        let config = config(&directory);
        let mut audit = AuditLog::open(&config, "project", "testing", "test").unwrap();
        for index in 0..21 {
            audit
                .record("PASS", "allow", &format!("SELECT {index}"))
                .unwrap();
        }

        let entries = audit.recent_session_entries(20);
        assert_eq!(audit.session_entry_count(), 20);
        assert_eq!(entries.len(), 20);
        assert_eq!(entries[0].query_hash, audit.hash_sql("SELECT 1"));
        assert!(!serde_json::to_string(&entries).unwrap().contains("SELECT"));

        drop(audit);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
