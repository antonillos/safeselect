use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub version: u32,
    pub database: DatabaseConfig,
    pub tls: Option<TlsConfig>,
    pub ssh: Option<SshConfig>,
    #[serde(default)]
    pub limits: LimitsOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub driver: String,
    pub url: String,
    pub username: String,
    pub secret: Option<SecretConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretConfig {
    pub source: String,
    pub service: Option<String>,
    pub account: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub mode: String,
    pub ca_file: Option<String>,
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    #[serde(default)]
    pub enabled: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub identity_file: Option<String>,
    pub known_hosts: Option<String>,
    pub forward_host: Option<String>,
    pub forward_port: Option<u16>,
    pub auth_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsOverride {
    pub statement_timeout_ms: Option<u64>,
    pub max_rows: Option<u64>,
    pub max_result_bytes: Option<u64>,
    pub idle_timeout_seconds: Option<u64>,
}

impl Default for LimitsOverride {
    fn default() -> Self {
        Self {
            statement_timeout_ms: None,
            max_rows: None,
            max_result_bytes: None,
            idle_timeout_seconds: None,
        }
    }
}
