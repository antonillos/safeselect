use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;

/// Returns a platform-appropriate hint for configuring a database secret.
pub fn secret_setup_hint(project_name: &str, env_name: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "security add-generic-password -a \"{project_name}/{env_name}\" -s \"safeselect\" -w \"<password>\""
        )
    } else {
        let var = format!(
            "SAFESELECT_PASSWORD_{}",
            env_name.to_uppercase().replace('-', "_")
        );
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
    ports: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
enum EnvValue {
    Map(HashMap<String, serde_yaml::Value>),
    List(Vec<String>),
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

fn resolve_env(env: &Option<EnvValue>) -> HashMap<String, String> {
    match env {
        Some(EnvValue::Map(m)) => m
            .iter()
            .map(|(k, v)| {
                let s = match v {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    other => format!("{other:?}"),
                };
                (k.clone(), s)
            })
            .collect(),
        Some(EnvValue::List(l)) => parse_env_list(l),
        None => HashMap::new(),
    }
}

fn parse_port(port_str: &str) -> u16 {
    // "5432:5432" or "5432:5432/tcp" or "5432"
    let s = port_str.split('/').next().unwrap_or(port_str);
    let host_part = s.split(':').next().unwrap_or(s);
    host_part.parse().unwrap_or(5432)
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
    let mut connections = vec![];

    for (service_name, service) in &compose.services {
        let image = match &service.image {
            Some(img) => img,
            None => continue,
        };

        if !is_postgres_image(image) {
            continue;
        }

        let env = resolve_env(&service.environment);
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

        let port = service.ports.first().map(|p| parse_port(p)).unwrap_or(5432);

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
            let account = format!("{}/{}", project_name, conn.env_name);
            store_password_in_keychain(&account, literal)?;
            Some(config::SecretConfig {
                source: "macos-keychain".to_string(),
                service: Some("safeselect".to_string()),
                account: Some(account),
                variable: None,
            })
        } else {
            None
        };

        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                driver: "postgresql".to_string(),
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
