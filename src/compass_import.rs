//! Interactive Compass choices and reimport identity. Secrets never go into TOML.
use crate::config::{password, SecretConfig};
use crate::error::{Result, SafeselectError};
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(test)]
#[path = "tests/compass_import.rs"]
mod tests;

fn variable_component(value: &str) -> String {
    value
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>()
        .join("_")
}

pub fn friendly_variable(project: &str, environment: &str, ssh: bool) -> String {
    let project = variable_component(project);
    let project = if project.is_empty() {
        "SAFESELECT".into()
    } else {
        project
    };
    let prefix = if project.starts_with(|c: char| c.is_ascii_digit()) {
        "_"
    } else {
        ""
    };
    let environment = variable_component(environment);
    let environment = if environment.is_empty() {
        "MONGODB".into()
    } else {
        environment
    };
    let kind = if ssh { "SSH" } else { "DB" };
    format!("{prefix}{project}_{environment}_{kind}_PASSWORD")
}

fn authority(url: &str) -> Option<(usize, usize)> {
    let start = url.find("://")? + 3;
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    Some((start, end))
}

/// Hide all userinfo in selection labels; preserve the original only in memory.
pub fn display_url(url: &str) -> String {
    let Some((start, end)) = authority(url) else {
        return "MongoDB connection".into();
    };
    let host = url[start..end].rsplit('@').next().unwrap_or("");
    // Query options can themselves contain credentials. Do not display them.
    format!("{}{host}", &url[..start])
}

fn percent_decode(value: &str) -> Result<String> {
    let mut bytes = Vec::new();
    let mut input = value.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let hi = input.next().and_then(|c| (c as char).to_digit(16));
            let lo = input.next().and_then(|c| (c as char).to_digit(16));
            let (Some(hi), Some(lo)) = (hi, lo) else {
                return Err(SafeselectError::Secret(
                    "Invalid encoded Compass credential".into(),
                ));
            };
            bytes.push((hi * 16 + lo) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes)
        .map_err(|_| SafeselectError::Secret("Invalid encoded Compass credential".into()))
}

pub fn split_database_url(url: &str) -> Result<(String, String, Option<String>)> {
    let Some((start, end)) = authority(url) else {
        return Ok((url.into(), String::new(), None));
    };
    let Some((userinfo, _)) = url[start..end].rsplit_once('@') else {
        return Ok((url.into(), String::new(), None));
    };
    let (user, pw) = match userinfo.split_once(':') {
        Some((user, pw)) => (user, Some(percent_decode(pw)?)),
        None => (userinfo, None),
    };
    let password = pw.filter(|s| !s.is_empty());
    Ok((
        password::inject_mongodb_password_placeholder(url, user),
        user.into(),
        password,
    ))
}

pub fn fingerprint(conn: &crate::compass::CompassConnection) -> String {
    use sha2::{Digest, Sha256};
    let mut url = conn.url.clone();
    if let Some((start, end)) = authority(&url) {
        if let Some((userinfo, host)) = url[start..end].rsplit_once('@') {
            let user = userinfo.split(':').next().unwrap_or("");
            url = format!("{}{user}@{host}{}", &url[..start], &url[end..]);
        }
    }
    let identity = serde_json::json!([url, conn.ssh_host, conn.ssh_port, conn.ssh_user]);
    hex::encode(Sha256::digest(identity.to_string().as_bytes()))
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
pub struct ImportIndex {
    #[serde(default)]
    connections: BTreeMap<String, Vec<String>>,
}

impl ImportIndex {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join("compass-imports.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let index: Self = toml::from_str(&std::fs::read_to_string(path)?)
            .map_err(|_| SafeselectError::Config("Invalid Compass import index".into()))?;
        if index
            .connections
            .values()
            .flatten()
            .any(|name| !valid_environment_name(name))
        {
            return Err(SafeselectError::Config(
                "Invalid environment name in Compass import index".into(),
            ));
        }
        Ok(index)
    }

    pub fn candidates(
        &self,
        dir: &Path,
        conn: &crate::compass::CompassConnection,
        default: &str,
    ) -> Vec<String> {
        let mut names = self
            .connections
            .get(&fingerprint(conn))
            .cloned()
            .unwrap_or_default();
        if !names.iter().any(|name| name == default) {
            names.push(default.into());
        }
        // Recognize pre-index imports, including custom names. Compare sanitized
        // URLs using the saved tunnel endpoint, without resolving DNS or secrets.
        if let Ok(entries) = std::fs::read_dir(dir) {
            let project = dir
                .parent()
                .and_then(|root| crate::load_project_config(root).ok());
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                    continue;
                }
                let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(mut existing) = toml::from_str::<crate::config::EnvironmentConfig>(&content)
                else {
                    continue;
                };
                if let Some(project) = &project {
                    if crate::config::merge_project_ssh(project, &mut existing).is_err() {
                        continue;
                    }
                }
                if legacy_match(conn, &existing) {
                    names.push(name.into());
                }
            }
        }
        names.retain(|name| {
            valid_environment_name(name) && dir.join(format!("{name}.toml")).exists()
        });
        names.sort();
        names.dedup();
        names
    }

    pub fn record(&mut self, conn: &crate::compass::CompassConnection, name: &str) {
        // An overwritten environment belongs only to its latest imported identity.
        for names in self.connections.values_mut() {
            names.retain(|existing| existing != name);
        }
        self.connections
            .entry(fingerprint(conn))
            .or_default()
            .push(name.into());
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let content =
            toml::to_string_pretty(self).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        crate::compass_import::write_atomic(&dir.join("compass-imports.toml"), &content)
    }
}

fn valid_environment_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn select_environment(
    dir: &Path,
    candidates: &[String],
    default: &str,
    non_interactive: bool,
) -> Result<Option<String>> {
    if non_interactive {
        if !candidates.is_empty() {
            println!(
                "Skipping existing Compass connection (non-interactive import never overwrites)."
            );
            return Ok(None);
        }
        if dir.join(format!("{default}.toml")).exists() {
            return Ok(None);
        }
        return Ok(Some(default.into()));
    }
    if !candidates.is_empty() {
        let action = select(
            "This connection already has an environment:",
            vec![
                "Update existing environment",
                "Create a new environment",
                "Skip this connection",
            ],
        )?;
        match action.as_str() {
            "Update existing environment" => {
                let selected = if candidates.len() == 1 {
                    candidates[0].clone()
                } else {
                    inquire::Select::new("Environment to update:", candidates.to_vec())
                        .prompt()
                        .map_err(cancelled)?
                };
                let message = format!("Update '{selected}'? Existing password sources, TLS and limits are kept unless explicitly changed.");
                if inquire::Confirm::new(&message)
                    .with_default(false)
                    .prompt()
                    .map_err(cancelled)?
                {
                    return Ok(Some(selected));
                }
                return Ok(None);
            }
            "Skip this connection" => return Ok(None),
            _ => {}
        }
    }
    let suggested = crate::unique_env_name(dir, default);
    loop {
        let requested = inquire::Text::new("Environment name:")
            .with_default(&suggested)
            .prompt()
            .map_err(cancelled)?;
        let name = crate::slug_env_name(&requested);
        if name.is_empty() {
            println!("Choose a non-empty environment name.");
            continue;
        }
        if dir.join(format!("{name}.toml")).exists() {
            println!("That environment already exists; choose a new name.");
            continue;
        }
        return Ok(Some(name));
    }
}

fn cancelled(_: inquire::InquireError) -> SafeselectError {
    SafeselectError::Other("Import cancelled".into())
}
fn select(prompt: &str, options: Vec<&str>) -> Result<String> {
    inquire::Select::new(prompt, options)
        .prompt()
        .map(str::to_string)
        .map_err(cancelled)
}

pub enum Destination {
    Keychain(String),
    Session(String),
}

pub fn store_literal<K, E>(
    value: String,
    destination: Destination,
    keychain: K,
    environment: E,
) -> Result<SecretConfig>
where
    K: FnOnce(&str, &str) -> Result<()>,
    E: FnOnce(&str, &str) -> Result<()>,
{
    if value.is_empty() {
        return Err(SafeselectError::Secret("Password must not be empty".into()));
    }
    match destination {
        Destination::Keychain(account) => {
            keychain(&account, &value)?;
            Ok(SecretConfig {
                source: "macos-keychain".into(),
                service: Some("safeselect".into()),
                account: Some(account),
                variable: None,
            })
        }
        Destination::Session(variable) => {
            password::validate_variable(&variable)?;
            if value.contains('\0') {
                return Err(SafeselectError::Secret(
                    "Password cannot contain a NUL byte for environment storage".into(),
                ));
            }
            environment(&variable, &value)?;
            Ok(env_secret(variable))
        }
    }
}

pub fn env_secret(variable: String) -> SecretConfig {
    SecretConfig {
        source: "env".into(),
        service: None,
        account: None,
        variable: Some(variable),
    }
}

fn prompt_variable(default: &str) -> Result<String> {
    loop {
        let value = inquire::Text::new("Password environment variable:")
            .with_default(default)
            .prompt()
            .map_err(cancelled)?;
        match password::variable_input(&value) {
            Ok(variable) => return Ok(variable),
            Err(_) => println!("Use a shell variable name, for example MYAPP_STAGING_DB_PASSWORD."),
        }
    }
}

pub struct CredentialPrompt<'a> {
    pub project: &'a str,
    pub environment: &'a str,
    pub ssh: bool,
    pub imported: Option<&'a str>,
    pub existing: Option<&'a SecretConfig>,
}

trait CredentialInteraction {
    fn select(&mut self, prompt: &str, options: Vec<&str>) -> Result<String>;
    fn confirm(&mut self, prompt: &str) -> Result<bool>;
    fn variable(&mut self, default: &str) -> Result<String>;
    fn password(&mut self) -> Result<String>;
}

struct TerminalInteraction;
impl CredentialInteraction for TerminalInteraction {
    fn select(&mut self, prompt: &str, options: Vec<&str>) -> Result<String> {
        select(prompt, options)
    }
    fn confirm(&mut self, prompt: &str) -> Result<bool> {
        inquire::Confirm::new(prompt)
            .with_default(false)
            .prompt()
            .map_err(cancelled)
    }
    fn variable(&mut self, default: &str) -> Result<String> {
        prompt_variable(default)
    }
    fn password(&mut self) -> Result<String> {
        inquire::Password::new("Password (hidden):")
            .without_confirmation()
            .prompt()
            .map_err(cancelled)
    }
}

trait CredentialStorage {
    fn is_macos(&self) -> bool;
    fn variable_present(&self, variable: &str) -> bool;
    fn keychain(&mut self, account: &str, value: &str) -> Result<()>;
    fn session(&mut self, variable: &str, value: &str) -> Result<()>;
}

struct SystemStorage;
impl CredentialStorage for SystemStorage {
    fn is_macos(&self) -> bool {
        cfg!(target_os = "macos")
    }
    fn variable_present(&self, variable: &str) -> bool {
        std::env::var_os(variable).is_some()
    }
    fn keychain(&mut self, account: &str, value: &str) -> Result<()> {
        crate::compose::store_password_in_keychain(account, value)
    }
    fn session(&mut self, variable: &str, value: &str) -> Result<()> {
        std::env::set_var(variable, value);
        Ok(())
    }
}

impl CredentialPrompt<'_> {
    pub fn run(&self) -> Result<Option<SecretConfig>> {
        self.run_with(&mut TerminalInteraction, &mut SystemStorage)
    }

    fn run_with(
        &self,
        ui: &mut impl CredentialInteraction,
        storage: &mut impl CredentialStorage,
    ) -> Result<Option<SecretConfig>> {
        let kind = if self.ssh { "Bastion" } else { "Database" };
        println!("\n── {kind} password ({}) ──", self.environment);
        let mut choices = vec![];
        if self.existing.is_some() {
            choices.push("Keep existing password source");
        }
        if self.imported.is_some_and(|value| !value.is_empty()) {
            choices.push("Use password from Compass export");
        }
        choices.extend([
            "Enter password (hidden)",
            "Use an exported environment variable",
            "Configure later",
        ]);
        loop {
            let choice = ui.select("How do you want to provide this password?", choices.clone())?;
            match choice.as_str() {
                "Keep existing password source" => return Ok(self.existing.cloned()),
                "Configure later" => {
                    let variable = ui.variable(&self.default_variable())?;
                    shell_hint(&variable);
                    return Ok(Some(env_secret(variable)));
                }
                "Use an exported environment variable" => {
                    let variable = ui.variable(&self.default_variable())?;
                    shell_hint(&variable);
                    return Ok(Some(env_secret(variable)));
                }
                _ => {
                    if let Some(secret) = self.prompt_literal(
                        choice == "Use password from Compass export",
                        ui,
                        storage,
                    )? {
                        return Ok(Some(secret));
                    }
                }
            }
        }
    }

    fn default_variable(&self) -> String {
        self.existing
            .and_then(|s| s.variable.clone())
            .unwrap_or_else(|| friendly_variable(self.project, self.environment, self.ssh))
    }

    fn prompt_literal(
        &self,
        imported: bool,
        ui: &mut impl CredentialInteraction,
        storage: &mut impl CredentialStorage,
    ) -> Result<Option<SecretConfig>> {
        let mut choices = vec![];
        if storage.is_macos() {
            choices.push("macOS Keychain (recommended)");
        }
        choices.push("Environment variable (this import session only)");
        let destination = ui.select("Where do you want to keep this password?", choices)?;
        let destination = if destination.starts_with("macOS") {
            // A distinct account avoids changing another environment's existing/shared credential.
            let suffix = if self.ssh { "/ssh" } else { "" };
            Destination::Keychain(format!(
                "{}/{}/compass-{}{suffix}",
                self.project,
                self.environment,
                uuid::Uuid::new_v4()
            ))
        } else {
            println!("The password will be held only by this SafeSelect process, not saved or exported to your terminal. Future commands require the variable in their launching shell.");
            if !ui.confirm("Use the password only for this import session?")? {
                return Ok(None);
            }
            let variable = ui.variable(&self.default_variable())?;
            if storage.variable_present(&variable)
                && !ui.confirm(
                    "This variable already exists. Replace it for this import session only?",
                )?
            {
                return Ok(None);
            }
            Destination::Session(variable)
        };
        let value = if imported {
            self.imported.unwrap_or_default().to_string()
        } else {
            ui.password()?
        };
        let secret = match destination {
            Destination::Keychain(account) => store_literal(
                value,
                Destination::Keychain(account),
                |account, value| storage.keychain(account, value),
                |_, _| unreachable!(),
            )?,
            Destination::Session(variable) => store_literal(
                value,
                Destination::Session(variable),
                |_, _| unreachable!(),
                |variable, value| storage.session(variable, value),
            )?,
        };
        if let Some(variable) = &secret.variable {
            println!("✓ Password available for this import session only: {variable}");
            shell_hint(variable);
        } else {
            println!("✓ Password saved in macOS Keychain");
        }
        Ok(Some(secret))
    }
}

fn shell_hint(variable: &str) {
    println!("For future commands, set this variable in the launching shell (Bash):");
    println!("  read -rsp 'Password: ' {variable}; echo; export {variable}");
}

pub fn ssh_secret(ssh: &crate::config::SshConfig) -> Option<SecretConfig> {
    if let Some(variable) = &ssh.secret_variable {
        return Some(env_secret(variable.clone()));
    }
    ssh.secret_account.as_ref().map(|account| SecretConfig {
        source: "macos-keychain".into(),
        service: Some("safeselect".into()),
        account: Some(account.clone()),
        variable: None,
    })
}

/// Never mutate a shared bastion as a side effect of importing one environment.
pub fn register_bastion(
    project: &mut crate::config::ProjectConfig,
    ssh: &crate::config::SshConfig,
) -> String {
    let shared = crate::project_ssh_bastion_from_env(ssh);
    if let Some((name, _)) = project
        .ssh_bastions
        .iter()
        .find(|(_, candidate)| **candidate == shared)
    {
        return name.clone();
    }
    let base = crate::default_bastion_name(ssh);
    let mut name = base.clone();
    let mut suffix = 2;
    while project.ssh_bastions.contains_key(&name) {
        name = format!("{base}-{suffix}");
        suffix += 1;
    }
    project.ssh_bastions.insert(name.clone(), shared);
    name
}

fn legacy_match(
    conn: &crate::compass::CompassConnection,
    existing: &crate::config::EnvironmentConfig,
) -> bool {
    if existing.database.kind != crate::backend::BackendKind::Document {
        return false;
    }
    let url = if let Some(ssh) = existing.ssh.as_ref().filter(|s| s.enabled) {
        if conn.ssh_host != ssh.host
            || conn.ssh_user != ssh.username
            || Some(conn.ssh_port.unwrap_or(22)) != ssh.port
        {
            return false;
        }
        if let Some((host, port)) = crate::extract_tcp_host_port(&conn.url) {
            if ssh.forward_host.as_deref() != Some(host.as_str()) || ssh.forward_port != Some(port)
            {
                return false;
            }
        } else {
            return false;
        } // SRV origins without provenance remain ambiguous.
        crate::rewrite_mongodb_url_for_local_endpoint(
            &conn.url,
            ssh.local_host.as_deref().unwrap_or("localhost"),
            ssh.local_port.unwrap_or(crate::DEFAULT_SSH_LOCAL_PORT),
        )
        .unwrap_or_else(|| conn.url.clone())
    } else {
        if conn.ssh_host.is_some() {
            return false;
        }
        conn.url.clone()
    };
    match (
        split_database_url(&url),
        split_database_url(&existing.database.url),
    ) {
        (Ok((left, user, _)), Ok((right, _, _))) => {
            left == right && user == existing.database.username
        }
        _ => false,
    }
}

/// Show only selected sources, never resolved values or Keychain account identifiers.
pub fn print_summary(
    project: &crate::config::ProjectConfig,
    environment: &crate::config::EnvironmentConfig,
) -> Result<()> {
    fn source(label: &str, secret: Option<&SecretConfig>) -> Result<()> {
        match secret {
            Some(secret) if secret.source == "env" => {
                let variable = secret
                    .variable
                    .as_deref()
                    .ok_or_else(|| SafeselectError::Secret("Missing password variable".into()))?;
                password::validate_variable(variable)?;
                println!("  {label}: environment variable {variable}");
            }
            Some(secret) if secret.source == "macos-keychain" => {
                println!("  {label}: macOS Keychain")
            }
            Some(_) => {
                return Err(SafeselectError::Secret(
                    "Unsupported password source".into(),
                ))
            }
            None => println!("  {label}: no password source"),
        }
        Ok(())
    }
    let mut resolved = environment.clone();
    crate::config::merge_project_ssh(project, &mut resolved)?;
    println!("Password sources (values hidden):");
    source("Database", resolved.database.secret.as_ref())?;
    if let Some(ssh) = resolved
        .ssh
        .as_ref()
        .filter(|s| s.auth_type.as_deref() == Some("PASSWORD"))
    {
        let secret = ssh_secret(ssh);
        source("Bastion", secret.as_ref())?;
    }
    Ok(())
}
