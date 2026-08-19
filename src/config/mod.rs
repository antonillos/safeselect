mod driver;
mod environment;
mod project;

pub use driver::DriverConfig;
pub use environment::{DatabaseConfig, EnvironmentConfig, LimitsOverride, SecretConfig, SshConfig};
pub use project::{AuditConfig, LimitsConfig, ProjectConfig, SecurityPolicy, SharedSshConfig};

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
    pub driver: Option<DriverConfig>,
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
            if let Some(driver) = load_driver_entry(&entry?.path())? {
                drivers.push(driver);
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
                let account = secret.account.as_deref().ok_or_else(|| {
                    SafeselectError::Secret("account required for keychain".into())
                })?;
                let service = secret.service.as_deref().ok_or_else(|| {
                    SafeselectError::Secret("service required for keychain".into())
                })?;
                resolve_keychain(service, account)
            }
            "env" => {
                let var = secret.variable.as_deref().ok_or_else(|| {
                    SafeselectError::Secret("variable name required for env source".into())
                })?;
                std::env::var(var).map_err(|_| SafeselectError::EnvVarNotSet(var.to_string()))
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
            return Err(SafeselectError::DriverChecksumMismatch(
                config.vendor.clone(),
            ));
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
            return Err(SafeselectError::LocalProjectNotFound(
                repo_root.to_path_buf(),
            ));
        }

        let project_file = safeselect_dir.join("project.toml");
        let project = if project_file.exists() {
            let content = std::fs::read_to_string(&project_file)?;
            toml::from_str(&content)?
        } else {
            ProjectConfig::default()
        };

        let env_file = safeselect_dir
            .join("environments")
            .join(format!("{env_name}.toml"));
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
        merge_project_ssh(&project, &mut environment)?;

        let driver = match environment.database.kind {
            crate::backend::BackendKind::Jdbc => {
                let vendor = environment.database.driver.as_deref().ok_or_else(|| {
                    SafeselectError::Config(format!(
                        "database.driver is required for JDBC environment '{}'",
                        env_name
                    ))
                })?;
                let driver = self.load_driver(vendor)?;
                self.validate_driver_file(&driver)?;
                Some(driver)
            }
            crate::backend::BackendKind::Document => None,
        };

        let password = if let Some(ref secret) = environment.database.secret {
            self.resolve_secret(secret)?
        } else if environment.database.kind == crate::backend::BackendKind::Document {
            String::new()
        } else {
            return Err(SafeselectError::Config(format!(
                "no secret configured in {}\n\
                 Run:\n  safeselect config set-password --environment {env_name}",
                env_file.display()
            )));
        };
        self.apply_limits(&project, &mut environment);
        Ok(ResolvedConfig {
            project,
            environment,
            driver,
            password,
            repo_root: repo_root.to_path_buf(),
        })
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

fn load_driver_entry(path: &Path) -> Result<Option<(String, DriverConfig)>> {
    if path.extension().is_none_or(|extension| extension != "toml") {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let config: DriverConfig = toml::from_str(&content)?;
    Ok(path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| (stem.to_string(), config)))
}

pub fn merge_project_ssh(
    project: &ProjectConfig,
    environment: &mut EnvironmentConfig,
) -> Result<()> {
    let Some(ssh) = environment.ssh.as_mut() else {
        return Ok(());
    };
    let Some(bastion_name) = ssh.bastion.as_deref() else {
        return Ok(());
    };
    let Some(shared) = project.ssh_bastions.get(bastion_name) else {
        return Err(SafeselectError::Config(format!(
            "ssh bastion '{bastion_name}' not found in project.toml"
        )));
    };

    merge_shared_ssh_fields(ssh, shared);
    Ok(())
}

fn merge_shared_ssh_fields(ssh: &mut SshConfig, shared: &SharedSshConfig) {
    if ssh.host.is_none() {
        ssh.host = shared.host.clone();
    }
    if ssh.port.is_none() {
        ssh.port = shared.port;
    }
    if ssh.username.is_none() {
        ssh.username = shared.username.clone();
    }
    if ssh.secret_account.is_none() {
        ssh.secret_account = shared.secret_account.clone();
    }
    if ssh.identity_file.is_none() {
        ssh.identity_file = shared.identity_file.clone();
    }
    if ssh.known_hosts.is_none() {
        ssh.known_hosts = shared.known_hosts.clone();
    }
    if ssh.auth_type.is_none() {
        ssh.auth_type = shared.auth_type.clone();
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

pub fn project_account_prefix(repo_root: &Path) -> String {
    let project_file = repo_root.join(".safeselect").join("project.toml");
    if let Ok(content) = std::fs::read_to_string(project_file) {
        if let Ok(project) = toml::from_str::<ProjectConfig>(&content) {
            if let Some(name) = project.display_name.as_deref().map(str::trim) {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }

    repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

pub fn preferred_keychain_account(
    repo_root: &Path,
    env_name: &str,
    environment: &EnvironmentConfig,
) -> String {
    if let Some(secret) = environment.database.secret.as_ref() {
        if secret.source == "macos-keychain" {
            if let Some(account) = secret.account.as_deref().map(str::trim) {
                if !account.is_empty() {
                    return account.to_string();
                }
            }
        }
    }

    format!("{}/{}", project_account_prefix(repo_root), env_name)
}

pub fn write_keychain_secret_to_env_file(env_file: &Path, account: &str) -> Result<()> {
    let content = std::fs::read_to_string(env_file)?;
    let mut environment: EnvironmentConfig = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid {}: {e}", env_file.display())))?;

    environment.database.secret = Some(SecretConfig {
        source: "macos-keychain".to_string(),
        service: Some("safeselect".to_string()),
        account: Some(account.to_string()),
        variable: None,
    });

    let updated = toml::to_string_pretty(&environment)
        .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(env_file, updated)?;
    Ok(())
}

fn resolve_keychain(service: &str, account: &str) -> Result<String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-a", account, "-s", service, "-w"])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_shared_ssh_settings_into_environment() {
        let project: ProjectConfig = toml::from_str(
            r#"
version = 1
[ssh_bastions.dev]
host = "bastion.example"
port = 2222
username = "jump"
secret_account = "jump-account"
identity_file = "/tmp/jump.key"
known_hosts = "/tmp/known_hosts"
auth_type = "key"
"#,
        )
        .unwrap();
        let mut environment: EnvironmentConfig = toml::from_str(
            r#"
version = 1
[database]
url = "jdbc:postgresql://db/app"
[ssh]
enabled = true
bastion = "dev"
"#,
        )
        .unwrap();

        merge_project_ssh(&project, &mut environment).unwrap();

        let ssh = environment.ssh.unwrap();
        assert_eq!(ssh.host.as_deref(), Some("bastion.example"));
        assert_eq!(ssh.port, Some(2222));
        assert_eq!(ssh.username.as_deref(), Some("jump"));
        assert_eq!(ssh.secret_account.as_deref(), Some("jump-account"));
        assert_eq!(ssh.identity_file.as_deref(), Some("/tmp/jump.key"));
        assert_eq!(ssh.known_hosts.as_deref(), Some("/tmp/known_hosts"));
        assert_eq!(ssh.auth_type.as_deref(), Some("key"));
    }

    #[test]
    fn lists_only_toml_driver_files() {
        let root = std::env::temp_dir().join(format!("safeselect-drivers-{}", std::process::id()));
        let drivers = root.join("drivers");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&drivers).unwrap();
        std::fs::write(
            drivers.join("postgresql.toml"),
            "version = 1\nvendor = \"postgresql\"\npath = \"/tmp/driver.jar\"\nclass = \"org.postgresql.Driver\"\nsha256 = \"abc\"\n",
        )
        .unwrap();
        std::fs::write(drivers.join("ignored.txt"), "ignored").unwrap();

        let loader = ConfigLoader {
            drivers_dir: drivers,
            config_dir: root.clone(),
        };
        let listed = loader.list_drivers().unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, "postgresql");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lists_no_drivers_when_directory_is_missing() {
        let root =
            std::env::temp_dir().join(format!("safeselect-empty-drivers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let loader = ConfigLoader {
            drivers_dir: root.join("missing"),
            config_dir: root.clone(),
        };

        assert!(loader.list_drivers().unwrap().is_empty());
    }

    #[test]
    fn leaves_environment_ssh_unchanged_when_no_bastion_is_configured() {
        let project = ProjectConfig::default();
        let mut environment: EnvironmentConfig = toml::from_str(
            r#"
version = 1
[database]
url = "jdbc:postgresql://db/app"
"#,
        )
        .unwrap();
        merge_project_ssh(&project, &mut environment).unwrap();
        assert!(environment.ssh.is_none());

        environment.ssh = Some(SshConfig {
            enabled: true,
            bastion: None,
            host: None,
            port: None,
            username: None,
            secret_account: None,
            identity_file: None,
            known_hosts: None,
            local_host: None,
            local_port: None,
            forward_host: None,
            forward_port: None,
            auth_type: None,
        });
        merge_project_ssh(&project, &mut environment).unwrap();
        assert!(environment.ssh.unwrap().host.is_none());
    }
}
