mod driver;
mod environment;
mod project;

pub use driver::DriverConfig;
pub use environment::{
    DatabaseConfig, EnvironmentConfig, LimitsOverride, SecretConfig, SshConfig,
};
pub use project::{AuditConfig, LimitsConfig, ProjectConfig, SecurityPolicy};

use crate::error::{Result, SafeselectError};
use std::path::{Path, PathBuf};

/// Global config loader. Only manages drivers (shared JARs) and sidecar.
/// Project config lives in .safeselect/ directories inside each repo.
pub struct ConfigLoader {
    drivers_dir: PathBuf,
    config_dir: PathBuf,
}

pub struct ResolvedConfig {
    pub project: ProjectConfig,
    pub environment: EnvironmentConfig,
    pub driver: DriverConfig,
    pub password: String,
    pub repo_root: PathBuf,
}

impl ConfigLoader {
    pub fn new() -> Self {
        let base = if let Ok(dir) = std::env::var("SAFESELECT_CONFIG_DIR") {
            PathBuf::from(dir)
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config/safeselect")
        };
        Self {
            drivers_dir: base.join("drivers"),
            config_dir: base,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn drivers_dir(&self) -> &Path {
        &self.drivers_dir
    }

    pub fn list_drivers(&self) -> Result<Vec<(String, DriverConfig)>> {
        let mut drivers = vec![];
        if !self.drivers_dir.exists() {
            return Ok(drivers);
        }
        for entry in std::fs::read_dir(&self.drivers_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "toml") {
                let content = std::fs::read_to_string(&path)?;
                let config: DriverConfig = toml::from_str(&content)?;
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    drivers.push((stem.to_string(), config));
                }
            }
        }
        drivers.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(drivers)
    }

    pub fn load_driver(&self, vendor: &str) -> Result<DriverConfig> {
        let driver_file = self.drivers_dir.join(format!("{vendor}.toml"));
        if !driver_file.exists() {
            return Err(SafeselectError::DriverNotFound(vendor.to_string()));
        }
        let content = std::fs::read_to_string(&driver_file)?;
        let config: DriverConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn resolve_secret(&self, secret: &SecretConfig) -> Result<String> {
        match secret.source.as_str() {
            "macos-keychain" => {
                let account = secret
                    .account
                    .as_deref()
                    .ok_or_else(|| SafeselectError::Secret("account required for keychain".into()))?;
                let service = secret
                    .service
                    .as_deref()
                    .ok_or_else(|| SafeselectError::Secret("service required for keychain".into()))?;
                resolve_keychain(service, account)
            }
            "env" => {
                let var = secret.variable.as_deref().ok_or_else(|| {
                    SafeselectError::Secret("variable name required for env source".into())
                })?;
                std::env::var(var)
                    .map_err(|_| SafeselectError::EnvVarNotSet(var.to_string()))
            }
            other => Err(SafeselectError::Secret(format!(
                "unknown secret source: {other}"
            ))),
        }
    }

    pub fn validate_driver_file(&self, config: &DriverConfig) -> Result<()> {
        use sha2::{Digest, Sha256};
        let path = Path::new(&config.path);
        if !path.exists() {
            return Err(SafeselectError::DriverFileNotFound(path.to_path_buf()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = path.metadata()?;
            let mode = metadata.permissions().mode();
            if mode & 0o002 != 0 {
                return Err(SafeselectError::InsecurePermissions(path.to_path_buf()));
            }
        }
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut buf)?;
        hasher.update(&buf);
        let actual = hex::encode(hasher.finalize());
        if actual != config.sha256 {
            return Err(SafeselectError::DriverChecksumMismatch(config.vendor.clone()));
        }
        Ok(())
    }

    /// Find a .safeselect/ directory by walking up from `cwd`.
    /// Returns the repo root (parent of .safeselect/).
    pub fn find_local_project(&self, cwd: &Path) -> Option<PathBuf> {
        let mut current = Some(cwd);
        while let Some(dir) = current {
            if dir.join(".safeselect").is_dir() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
        None
    }

    /// Resolve config for a local .safeselect/ project.
    pub fn resolve_local(&self, repo_root: &Path, env_name: &str) -> Result<ResolvedConfig> {
        let safeselect_dir = repo_root.join(".safeselect");
        if !safeselect_dir.is_dir() {
            return Err(SafeselectError::LocalProjectNotFound(repo_root.to_path_buf()));
        }

        let project_file = safeselect_dir.join("project.toml");
        let project = if project_file.exists() {
            let content = std::fs::read_to_string(&project_file)?;
            toml::from_str(&content)?
        } else {
            ProjectConfig::default()
        };

        let env_file = safeselect_dir.join("environments").join(format!("{env_name}.toml"));
        if !env_file.exists() {
            return Err(SafeselectError::EnvironmentNotFound(
                env_name.to_string(),
                safeselect_dir.join("environments").display().to_string(),
            ));
        }
        let content = std::fs::read_to_string(&env_file)?;
        let mut environment: EnvironmentConfig = toml::from_str(&content).map_err(|e| {
            let msg = format!(
                "invalid {}: {e}\n\
                 Hint: if you added [database.secret] manually, ensure it has a \"source\" field.\n  \
                 Valid sources: \"macos-keychain\" (macOS Keychain) or \"env\" (environment variable).\n  \
                 See: safeselect import-compose --help",
                env_file.display()
            );
            SafeselectError::Config(msg)
        })?;

        let driver = self.load_driver(&environment.database.driver)?;
        self.validate_driver_file(&driver)?;

        if let Some(ref secret) = environment.database.secret {
            let password = self.resolve_secret(secret)?;
            self.apply_limits(&project, &mut environment);
            Ok(ResolvedConfig {
                project,
                environment,
                driver,
                password,
                repo_root: repo_root.to_path_buf(),
            })
        } else {
            Err(SafeselectError::Config(format!(
                "no secret configured in {}\n\
                 Run:\n  safeselect config set-password --environment {env_name}",
                env_file.display()
            )))
        }
    }

    fn apply_limits(&self, project: &ProjectConfig, env: &mut EnvironmentConfig) {
        let pal = &project.limits;
        if let Some(st) = env.limits.statement_timeout_ms {
            if st > pal.statement_timeout_ms {
                env.limits.statement_timeout_ms = Some(pal.statement_timeout_ms);
            }
        }
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_keychain(service: &str, account: &str) -> Result<String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            account,
            "-s",
            service,
            "-w",
        ])
        .output()
        .map_err(|e| SafeselectError::Secret(format!("security command failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SafeselectError::KeychainNotFound(format!(
            "{service}/{account}: {stderr}"
        )));
    }

    Ok(String::from_utf8(output.stdout)
        .map_err(|_| SafeselectError::Secret("invalid UTF-8 from keychain".into()))?
        .trim()
        .to_string())
}
