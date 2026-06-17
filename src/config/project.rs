use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub version: u32,
    pub display_name: Option<String>,
    #[serde(default)]
    pub security: SecurityPolicy,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub audit: AuditConfig,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            version: 1,
            display_name: None,
            security: SecurityPolicy::default(),
            limits: LimitsConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    #[serde(default)]
    pub allowed_schemas: Vec<String>,
    #[serde(default)]
    pub denied_relations: Vec<String>,
    #[serde(default = "default_true")]
    pub require_single_statement: bool,
    #[serde(default)]
    pub allow_volatile_functions: bool,
    #[serde(default)]
    pub allow_set_role: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_stmt_timeout")]
    pub statement_timeout_ms: u64,
    #[serde(default = "default_conn_timeout")]
    pub connection_timeout_ms: u64,
    #[serde(default = "default_max_rows")]
    pub max_rows: u64,
    #[serde(default = "default_max_bytes")]
    pub max_result_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_audit_dir")]
    pub directory: String,
    #[serde(default = "default_audit_max_bytes")]
    pub max_file_bytes: u64,
    #[serde(default = "default_audit_retain")]
    pub retain_files: u32,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            allowed_schemas: vec![],
            denied_relations: vec![],
            require_single_statement: true,
            allow_volatile_functions: false,
            allow_set_role: false,
        }
    }
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            statement_timeout_ms: default_stmt_timeout(),
            connection_timeout_ms: default_conn_timeout(),
            max_rows: default_max_rows(),
            max_result_bytes: default_max_bytes(),
        }
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: default_audit_dir(),
            max_file_bytes: default_audit_max_bytes(),
            retain_files: default_audit_retain(),
        }
    }
}

fn default_true() -> bool { true }
fn default_stmt_timeout() -> u64 { 5000 }
fn default_conn_timeout() -> u64 { 5000 }
fn default_max_rows() -> u64 { 500 }
fn default_max_bytes() -> u64 { 2_000_000 }
fn default_audit_dir() -> String { "~/.local/state/safeselect/audit".to_string() }
fn default_audit_max_bytes() -> u64 { 10_000_000 }
fn default_audit_retain() -> u32 { 10 }
