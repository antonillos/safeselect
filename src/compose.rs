use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;

fn secret_env_var(env_name: &str) -> String {
    format!(
        "SAFESELECT_PASSWORD_{}",
        env_name.to_uppercase().replace('-', "_")
    )
}

/// Returns a platform-appropriate hint for configuring a database secret.
pub fn secret_setup_hint(project_name: &str, env_name: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "security add-generic-password -a \"{project_name}/{env_name}\" -s \"safeselect\" -w \"<password>\""
        )
    } else {
        let var = secret_env_var(env_name);
        format!(
            "export {var}=\"<password>\"  # then edit .safeselect/environments/{env_name}.toml:\n  \
             [database.secret]\n  source = \"env\"\n  variable = \"{var}\""
        )
    }
}

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub created: usize,
    pub env_names: Vec<String>,
    /// (env_name, account_name_for_keychain)
    pub no_password: Vec<(String, String)>,
}

pub struct ImportGuidance {
    pub text: String,
    pub imported_env_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ComposeConnection {
    pub name: String,
    pub env_name: String,
    pub service: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password_literal: Option<String>,
    pub password_var: Option<String>,
    pub compose_path: String,
}

#[derive(serde::Deserialize, Debug)]
struct ComposeFile {
    #[serde(default)]
    services: HashMap<String, ComposeService>,
}

#[derive(serde::Deserialize, Debug)]
struct ComposeService {
    image: Option<String>,
    #[serde(default)]
    environment: Option<EnvValue>,
    #[serde(default)]
    ports: Vec<PortValue>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum EnvValue {
    Map(HashMap<String, serde_yaml::Value>),
    List(Vec<String>),
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum PortValue {
    Short(String),
    Long(ComposePort),
}

#[derive(serde::Deserialize, Debug)]
struct ComposePort {
    published: Option<serde_yaml::Value>,
    host_ip: Option<String>,
}

fn is_postgres_image(image: &str) -> bool {
    let lower = image.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    // Remove tag (everything after ':')
    let base = name.split(':').next().unwrap_or(name);
    // Match any image with "postgres" in its name
    base.contains("postgres") || base.contains("postgis") || base.contains("timescaledb")
}

fn parse_env_list(items: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Some(pos) = item.find('=') {
            let key = item[..pos].trim().to_string();
            let val = item[pos + 1..].trim().to_string();
            map.insert(key, val);
        }
    }
    map
}

fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        let mut value = value.trim().to_string();
        if value.len() >= 2 {
            let quoted = (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''));
            if quoted {
                value = value[1..value.len() - 1].to_string();
            }
        }
        vars.insert(key.to_string(), value);
    }
    vars
}

fn load_dotenv(dir: &Path) -> HashMap<String, String> {
    let path = dir.join(".env");
    std::fs::read_to_string(path)
        .map(|content| parse_dotenv(&content))
        .unwrap_or_default()
}

fn resolve_scalar(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

fn resolve_compose_value(value: &str, dotenv: &HashMap<String, String>) -> String {
    let trimmed = value.trim();
    if !(trimmed.starts_with("${") && trimmed.ends_with('}')) {
        return trimmed.to_string();
    }

    let inner = &trimmed[2..trimmed.len() - 1];
    let (key, default) = if let Some((key, default)) = inner.split_once(":-") {
        (key.trim(), Some(default))
    } else if let Some((key, default)) = inner.split_once('-') {
        (key.trim(), Some(default))
    } else {
        (inner.trim(), None)
    };

    dotenv
        .get(key)
        .cloned()
        .or_else(|| std::env::var(key).ok())
        .or_else(|| default.map(|v| v.to_string()))
        .unwrap_or_else(|| trimmed.to_string())
}

fn resolve_env(
    env: &Option<EnvValue>,
    dotenv: &HashMap<String, String>,
) -> HashMap<String, String> {
    match env {
        Some(EnvValue::Map(m)) => m
            .iter()
            .map(|(k, v)| {
                let s = resolve_compose_value(&resolve_scalar(v), dotenv);
                (k.clone(), s)
            })
            .collect(),
        Some(EnvValue::List(l)) => parse_env_list(l)
            .into_iter()
            .map(|(k, v)| (k, resolve_compose_value(&v, dotenv)))
            .collect(),
        None => HashMap::new(),
    }
}

fn parse_port_string(port_str: &str) -> u16 {
    // "5432:5432" or "5432:5432/tcp" or "5432"
    let s = port_str.split('/').next().unwrap_or(port_str);
    let host_part = s.split(':').next().unwrap_or(s);
    host_part.parse().unwrap_or(5432)
}

fn parse_port(port: &PortValue) -> u16 {
    match port {
        PortValue::Short(value) => parse_port_string(value),
        PortValue::Long(value) => value
            .published
            .as_ref()
            .map(resolve_scalar)
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(5432),
    }
}

fn is_var_ref(val: &str) -> Option<String> {
    let v = val.trim();
    if v.starts_with("${") && v.ends_with('}') {
        let inner = &v[2..v.len() - 1];
        let inner = inner.split(':').next().unwrap_or(inner).trim();
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }
    None
}

pub fn scan_all(scan_path: &Path) -> Result<Vec<(String, Vec<ComposeConnection>)>> {
    let compose_files = find_compose_files(scan_path);

    if compose_files.is_empty() {
        return Ok(vec![]);
    }

    let mut results: Vec<(String, Vec<ComposeConnection>)> = vec![];

    for path in &compose_files {
        let content = std::fs::read_to_string(path)?;
        let connections = parse_compose_file(path, &content)?;
        if !connections.is_empty() {
            let label = project_label(path, scan_path);
            results.push((label, connections));
        }
    }

    Ok(results)
}

fn project_label(compose_path: &Path, scan_root: &Path) -> String {
    if let Some(parent) = compose_path.parent() {
        if let Ok(relative) = parent.strip_prefix(scan_root) {
            if relative.as_os_str().is_empty() {
                compose_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("docker-compose.yml")
                    .to_string()
            } else {
                format!(
                    "{}/{}",
                    relative.display(),
                    compose_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                )
            }
        } else {
            compose_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("docker-compose.yml")
                .to_string()
        }
    } else {
        compose_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("docker-compose.yml")
            .to_string()
    }
}

fn find_compose_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = vec![];
    let candidates = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ];

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // skip hidden dirs, node_modules, target
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if !name.starts_with('.') && name != "node_modules" && name != "target" {
                        files.extend(find_compose_files(&path));
                    }
                }
            } else if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if candidates.contains(&name) {
                        files.push(path);
                    }
                }
            }
        }
    }

    files
}

fn parse_compose_file(path: &Path, content: &str) -> Result<Vec<ComposeConnection>> {
    let compose: ComposeFile = serde_yaml::from_str(content)?;
    let dotenv = path.parent().map(load_dotenv).unwrap_or_default();
    let mut connections = vec![];

    for (service_name, service) in &compose.services {
        let image = match &service.image {
            Some(img) => img,
            None => continue,
        };

        if !is_postgres_image(image) {
            continue;
        }

        let env = resolve_env(&service.environment, &dotenv);
        let database = env
            .get("POSTGRES_DB")
            .or_else(|| env.get("POSTGRES_PASSWORD").map(|_| service_name))
            .cloned()
            .unwrap_or_else(|| service_name.clone());

        let username = env
            .get("POSTGRES_USER")
            .cloned()
            .unwrap_or_else(|| "postgres".to_string());

        let password_literal = env.get("POSTGRES_PASSWORD").and_then(|p| {
            if is_var_ref(p).is_some() {
                None
            } else {
                Some(p.clone())
            }
        });

        let password_var = env.get("POSTGRES_PASSWORD").and_then(|p| is_var_ref(p));

        let port = service.ports.first().map(parse_port).unwrap_or(5432);

        let env_name = service_name.to_lowercase().replace(' ', "-");

        let name = format!("{} ({})", service_name, path.display());

        connections.push(ComposeConnection {
            name,
            env_name,
            service: service_name.clone(),
            host: "localhost".to_string(),
            port,
            database,
            username,
            password_literal,
            password_var,
            compose_path: path.to_string_lossy().to_string(),
        });
    }

    Ok(connections)
}

pub fn write_config_files(
    repo_root: &Path,
    connections: &[ComposeConnection],
    project_name: &str,
) -> Result<ImportResult> {
    use crate::config;

    let safeselect_dir = repo_root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir)?;

    let mut created = 0;
    let mut env_names = vec![];
    let mut no_password = vec![];

    let project_config = config::ProjectConfig::default();
    let project_toml = toml::to_string_pretty(&project_config)
        .map_err(|e| crate::error::SafeselectError::TomlSer(e.to_string()))?;
    let project_file = safeselect_dir.join("project.toml");
    if !project_file.exists() {
        std::fs::write(&project_file, project_toml)?;
        created += 1;
    }

    for conn in connections {
        let url = format!(
            "jdbc:postgresql://{}:{}/{}",
            conn.host, conn.port, conn.database
        );

        let secret = if let Some(ref var) = conn.password_var {
            Some(config::SecretConfig {
                source: "env".to_string(),
                service: None,
                account: None,
                variable: Some(var.clone()),
            })
        } else if let Some(ref literal) = conn.password_literal {
            if cfg!(target_os = "macos") {
                let account = format!("{}/{}", project_name, conn.env_name);
                store_password_in_keychain(&account, literal)?;
                Some(config::SecretConfig {
                    source: "macos-keychain".to_string(),
                    service: Some("safeselect".to_string()),
                    account: Some(account),
                    variable: None,
                })
            } else {
                Some(config::SecretConfig {
                    source: "env".to_string(),
                    service: None,
                    account: None,
                    variable: Some(secret_env_var(&conn.env_name)),
                })
            }
        } else {
            None
        };

        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                kind: crate::backend::BackendKind::Jdbc,
                vendor: Some("postgresql".to_string()),
                driver: Some("postgresql".to_string()),
                url,
                username: conn.username.clone(),
                secret,
            },
            tls: None,
            ssh: None,
            limits: config::LimitsOverride::default(),
        };

        let env_toml = toml::to_string_pretty(&env_config)
            .map_err(|e| crate::error::SafeselectError::TomlSer(e.to_string()))?;
        let env_file = env_dir.join(format!("{}.toml", conn.env_name));
        if !env_file.exists() {
            if conn.password_var.is_none() && conn.password_literal.is_none() {
                let account = format!("{}/{}", project_name, conn.env_name);
                eprintln!(
                    "WARN: No password configured for '{}'.\n  {}",
                    conn.service,
                    secret_setup_hint(project_name, &conn.env_name)
                );
                no_password.push((conn.env_name.clone(), account));
            }
            std::fs::write(&env_file, env_toml)?;
            created += 1;
        }
        env_names.push(conn.env_name.clone());
    }

    Ok(ImportResult {
        created,
        env_names,
        no_password,
    })
}

pub fn build_import_guidance(
    project_name: &str,
    result: &ImportResult,
    imported_names: &[String],
    include_agent_step: bool,
) -> ImportGuidance {
    let env_names = if result.env_names.is_empty() {
        imported_names.to_vec()
    } else {
        result.env_names.clone()
    };
    let no_password_names: Vec<String> =
        result.no_password.iter().map(|(n, _)| n.clone()).collect();

    build_guidance_from_parts(
        project_name,
        &env_names,
        &no_password_names,
        include_agent_step,
    )
}

pub fn build_guidance_from_parts(
    project_name: &str,
    env_names: &[String],
    no_password_envs: &[String],
    include_agent_step: bool,
) -> ImportGuidance {
    let mut parts = vec![];
    if env_names.is_empty() {
        parts.push("All environments already exist. Nothing imported.".to_string());
    } else {
        parts.push(format!(
            "Imported {} connection(s): {}",
            env_names.len(),
            env_names.join(", ")
        ));
    }

    parts.push(String::new());
    parts.push("Next steps:".to_string());
    parts.push("1. Ensure the PostgreSQL JDBC driver is available: safeselect driver download --vendor postgresql".to_string());

    if no_password_envs.is_empty() {
        parts.push("2. Passwords were imported or are already configured.".to_string());
    } else {
        parts.push("2. Configure missing passwords:".to_string());
        for env_name in no_password_envs {
            parts.push(format!(
                "   - {}",
                secret_setup_hint(project_name, env_name)
            ));
        }
    }

    if env_names.is_empty() {
        parts.push("3. Run safeselect check --environment <env> after you add one.".to_string());
    } else {
        parts.push("3. Verify connectivity:".to_string());
        for env_name in env_names {
            parts.push(format!("   - safeselect check --environment {env_name}"));
        }
    }

    if include_agent_step {
        if env_names.is_empty() {
            parts.push("4. Install the MCP entry after you have an environment: safeselect agent install opencode --environment <env>".to_string());
        } else {
            parts.push("4. Install SafeSelect in your AI agent:".to_string());
            for env_name in env_names {
                parts.push(format!(
                    "   - safeselect agent install opencode --environment {env_name}"
                ));
            }
        }
    }

    ImportGuidance {
        text: parts.join("\n"),
        imported_env_names: env_names.to_vec(),
    }
}

pub fn read_password_from_keychain(account: &str) -> Result<String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-a",
            account,
            "-s",
            "safeselect",
            "-w",
        ])
        .output()
        .map_err(|e| crate::error::SafeselectError::Secret(format!("security find failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::SafeselectError::KeychainNotFound(format!(
            "{account}: {stderr}"
        )));
    }

    Ok(String::from_utf8(output.stdout)
        .map_err(|_| crate::error::SafeselectError::Secret("invalid UTF-8 from keychain".into()))?
        .trim()
        .to_string())
}

pub fn delete_password_from_keychain(account: &str) -> Result<()> {
    let output = std::process::Command::new("security")
        .args(["delete-generic-password", "-a", account, "-s", "safeselect"])
        .output()
        .map_err(|e| {
            crate::error::SafeselectError::Secret(format!("security delete failed: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("WARN: could not delete old Keychain entry: {stderr}");
    }

    Ok(())
}

pub fn store_password_in_keychain(account: &str, password: &str) -> Result<()> {
    let output = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-a",
            account,
            "-s",
            "safeselect",
            "-w",
            password,
            "-U",
        ])
        .output()
        .map_err(|e| {
            crate::error::SafeselectError::Secret(format!("security command failed: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("WARN: could not store password in Keychain: {stderr}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_dotenv_defaults_in_environment() {
        let env = Some(EnvValue::Map(HashMap::from([
            (
                "POSTGRES_PASSWORD".to_string(),
                serde_yaml::Value::String("${DB_PASSWORD:-fallback}".to_string()),
            ),
            (
                "POSTGRES_USER".to_string(),
                serde_yaml::Value::String("${DB_USER}".to_string()),
            ),
        ])));
        let dotenv = HashMap::from([
            ("DB_PASSWORD".to_string(), "from-dotenv".to_string()),
            ("DB_USER".to_string(), "reader".to_string()),
        ]);

        let resolved = resolve_env(&env, &dotenv);

        assert_eq!(resolved.get("POSTGRES_PASSWORD").unwrap(), "from-dotenv");
        assert_eq!(resolved.get("POSTGRES_USER").unwrap(), "reader");
    }

    #[test]
    fn parses_short_and_long_ports() {
        let short = PortValue::Short("15432:5432".to_string());
        let long = PortValue::Long(ComposePort {
            published: Some(serde_yaml::Value::Number(15433.into())),
            host_ip: Some("127.0.0.1".to_string()),
        });

        assert_eq!(parse_port(&short), 15432);
        assert_eq!(parse_port(&long), 15433);
    }

    #[test]
    fn parses_compose_file_with_long_ports_and_dotenv() {
        let content = r#"
services:
  db:
    image: postgres:17
    environment:
      POSTGRES_DB: app
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASSWORD:-testpass}
    ports:
      - target: 5432
        published: 15432
        host_ip: 127.0.0.1
"#;
        let temp =
            std::env::temp_dir().join(format!("safeselect-compose-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join(".env"), "DB_USER=agent\n").unwrap();
        let compose_path = temp.join("compose.yaml");

        let parsed = parse_compose_file(&compose_path, content).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].port, 15432);
        assert_eq!(parsed[0].username, "agent");
        assert_eq!(parsed[0].password_literal.as_deref(), Some("testpass"));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn builds_agent_ready_import_guidance() {
        let result = ImportResult {
            created: 1,
            env_names: vec!["testing".to_string()],
            no_password: vec![("testing".to_string(), "project/testing".to_string())],
        };

        let guidance = build_import_guidance("project", &result, &["testing".to_string()], true);

        assert!(guidance.text.contains("Next steps:"));
        assert!(guidance
            .text
            .contains("safeselect check --environment testing"));
        assert!(guidance
            .text
            .contains("safeselect agent install opencode --environment testing"));
    }
}
