mod driver;
mod environment;
mod project;

pub use driver::DriverConfig;
pub use environment::{
    DatabaseConfig, EnvironmentConfig, LimitsOverride, SecretConfig,
};
pub use project::{AuditConfig, LimitsConfig, ProjectConfig, SecurityPolicy};

use crate::error::{Result, SafeselectError};
use std::path::{Path, PathBuf};

pub struct ConfigLoader {
    drivers_dir: PathBuf,
    projects_dir: PathBuf,
    config_dir: PathBuf,
}

pub struct ResolvedConfig {
    pub project: ProjectConfig,
    pub environment: EnvironmentConfig,
    pub driver: DriverConfig,
    pub password: String,
}

impl ConfigLoader {
    pub fn new() -> Self {
        let base = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("safeselect");
        Self {
            drivers_dir: base.join("drivers"),
            projects_dir: base.join("projects"),
            config_dir: base,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn drivers_dir(&self) -> &Path {
        &self.drivers_dir
    }

    pub fn projects_dir(&self) -> &Path {
        &self.projects_dir
    }

    pub fn list_projects(&self) -> Result<Vec<String>> {
        let mut projects = vec![];
        if !self.projects_dir.exists() {
            return Ok(projects);
        }
        for entry in std::fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    if !name.starts_with('.') {
                        projects.push(name.to_string());
                    }
                }
            }
        }
        projects.sort();
        Ok(projects)
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

    pub fn load_project(&self, name: &str) -> Result<ProjectConfig> {
        let project_dir = self.projects_dir.join(name);
        if !project_dir.exists() {
            return Err(SafeselectError::ProjectNotFound(
                name.to_string(),
                self.projects_dir.clone(),
            ));
        }
        let project_file = project_dir.join("project.toml");
        if !project_file.exists() {
            return Err(SafeselectError::Config(format!(
                "project.toml not found in {}",
                project_dir.display()
            )));
        }
        let content = std::fs::read_to_string(&project_file)?;
        let config: ProjectConfig = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_environment(&self, project: &str, env: &str) -> Result<EnvironmentConfig> {
        let env_file = self
            .projects_dir
            .join(project)
            .join("environments")
            .join(format!("{env}.toml"));
        if !env_file.exists() {
            return Err(SafeselectError::EnvironmentNotFound(
                env.to_string(),
                project.to_string(),
            ));
        }
        let content = std::fs::read_to_string(&env_file)?;
        let config: EnvironmentConfig = toml::from_str(&content)?;
        Ok(config)
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
            if mode & 0o007 != 0 {
                return Err(SafeselectError::InsecurePermissions(path.to_path_buf()));
            }
        }
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        let actual = hex::encode(hasher.finalize());
        if actual != config.sha256 {
            return Err(SafeselectError::DriverChecksumMismatch(config.vendor.clone()));
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        project_name: &str,
        env_name: &str,
    ) -> Result<ResolvedConfig> {
        let project = self.load_project(project_name)?;
        let mut environment = self.load_environment(project_name, env_name)?;

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
            })
        } else {
            Err(SafeselectError::Config(format!(
                "no secret configured for {project_name}/{env_name}"
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
