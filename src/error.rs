use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SafeselectError {
    #[error("Config error: {0}")]
    Config(String),

    #[error(".safeselect/ not found near {0}")]
    LocalProjectNotFound(PathBuf),

    #[error("Environment '{0}' not found in {1}")]
    EnvironmentNotFound(String, String),

    #[error("Driver '{0}' not found")]
    DriverNotFound(String),

    #[error("Driver file not found: {0}")]
    DriverFileNotFound(PathBuf),

    #[error("Driver checksum mismatch for {0}")]
    DriverChecksumMismatch(String),

    #[error("Insecure permissions on {0}")]
    InsecurePermissions(PathBuf),

    #[error("Secret source error: {0}")]
    Secret(String),

    #[error("Secret '{0}' not found in macOS Keychain")]
    KeychainNotFound(String),

    #[error("Environment variable '{0}' not set")]
    EnvVarNotSet(String),

    #[error("Sidecar error: {0}")]
    Sidecar(String),

    #[error("Sidecar not started")]
    SidecarNotStarted,

    #[error("Sidecar Java not found at {0}")]
    SidecarJavaNotFound(PathBuf),

    #[error("Security error: {0}")]
    Security(String),

    #[error("Query rejected: {0}")]
    QueryRejected(String),

    #[error("Limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Audit error: {0}")]
    Audit(String),

    #[error("MCP protocol error: {0}")]
    McpProtocol(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSer(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("YAML parse error: {0}")]
    Yaml(String),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("{0}")]
    Other(String),
}

impl From<serde_yaml::Error> for SafeselectError {
    fn from(e: serde_yaml::Error) -> Self {
        SafeselectError::Yaml(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SafeselectError>;

impl From<String> for SafeselectError {
    fn from(s: String) -> Self {
        SafeselectError::Other(s)
    }
}
