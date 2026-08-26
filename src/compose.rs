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
        if let Some((key, value)) = parse_dotenv_line(raw_line) {
            vars.insert(key, value);
        }
    }
    vars
}

fn parse_dotenv_line(raw_line: &str) -> Option<(String, String)> {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), unquote_dotenv_value(value.trim())))
}

fn unquote_dotenv_value(value: &str) -> String {
    let quoted = value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')));
    if quoted {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
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
    collect_compose_files(dir, &mut files);
    files
}

fn collect_compose_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let candidates = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ];

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            collect_compose_entry(entry.path(), &candidates, files);
        }
    }
}

fn collect_compose_entry(
    path: std::path::PathBuf,
    candidates: &[&str],
    files: &mut Vec<std::path::PathBuf>,
) {
    if path.is_dir() {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if !name.starts_with('.') && name != "node_modules" && name != "target" {
            collect_compose_files(&path, files);
        }
    } else if path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| candidates.contains(&name))
    {
        files.push(path);
    }
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
    append_import_summary(&mut parts, env_names);

    parts.push(String::new());
    parts.push("Next steps:".to_string());
    parts.push("1. Ensure the PostgreSQL JDBC driver is available: safeselect driver download --vendor postgresql".to_string());

    append_password_summary(&mut parts, project_name, no_password_envs);
    append_connectivity_summary(&mut parts, env_names);
    append_agent_summary(&mut parts, env_names, include_agent_step);

    ImportGuidance {
        text: parts.join("\n"),
        imported_env_names: env_names.to_vec(),
    }
}

fn append_import_summary(parts: &mut Vec<String>, env_names: &[String]) {
    if env_names.is_empty() {
        parts.push("All environments already exist. Nothing imported.".to_string());
    } else {
        parts.push(format!(
            "Imported {} connection(s): {}",
            env_names.len(),
            env_names.join(", ")
        ));
    }
}

fn append_password_summary(parts: &mut Vec<String>, project_name: &str, env_names: &[String]) {
    if env_names.is_empty() {
        parts.push("2. Passwords were imported or are already configured.".to_string());
    } else {
        parts.push("2. Configure missing passwords:".to_string());
        for env_name in env_names {
            parts.push(format!(
                "   - {}",
                secret_setup_hint(project_name, env_name)
            ));
        }
    }
}

fn append_connectivity_summary(parts: &mut Vec<String>, env_names: &[String]) {
    if env_names.is_empty() {
        parts.push("3. Run safeselect check --environment <env> after you add one.".to_string());
    } else {
        parts.push("3. Verify connectivity:".to_string());
        for env_name in env_names {
            parts.push(format!("   - safeselect check --environment {env_name}"));
        }
    }
}

fn append_agent_summary(parts: &mut Vec<String>, env_names: &[String], include: bool) {
    if !include {
        return;
    }
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

    parse_password_output(account, output)
}

fn parse_password_output(account: &str, output: std::process::Output) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::SafeselectError::KeychainNotFound(format!(
            "{account}: {stderr}"
        )));
    }

    String::from_utf8(output.stdout)
        .map(|password| password.trim().to_string())
        .map_err(|_| crate::error::SafeselectError::Secret("invalid UTF-8 from keychain".into()))
}

pub fn delete_password_from_keychain(account: &str) -> Result<()> {
    let output = delete_keychain_command(account)
        .output()
        .map_err(delete_keychain_command_error)?;
    report_keychain_delete_result(&output);
    Ok(())
}

fn report_keychain_delete_result(output: &std::process::Output) {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("WARN: could not delete old Keychain entry: {stderr}");
    }
}

fn delete_keychain_command(account: &str) -> std::process::Command {
    let mut command = std::process::Command::new("security");
    command.args(["delete-generic-password", "-a", account, "-s", "safeselect"]);
    command
}

fn delete_keychain_command_error(error: std::io::Error) -> crate::error::SafeselectError {
    crate::error::SafeselectError::Secret(format!("security delete failed: {error}"))
}

pub fn store_password_in_keychain(account: &str, password: &str) -> Result<()> {
    let output = keychain_command(account, password)
        .output()
        .map_err(keychain_command_error)?;
    report_keychain_store_result(&output);
    Ok(())
}

fn report_keychain_store_result(output: &std::process::Output) {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("WARN: could not store password in Keychain: {stderr}");
    }
}

fn keychain_command(account: &str, password: &str) -> std::process::Command {
    let mut command = std::process::Command::new("security");
    command.args([
        "add-generic-password",
        "-a",
        account,
        "-s",
        "safeselect",
        "-w",
        password,
        "-U",
    ]);
    command
}

fn keychain_command_error(error: std::io::Error) -> crate::error::SafeselectError {
    crate::error::SafeselectError::Secret(format!("security command failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_secret_environment_variable_name() {
        assert_eq!(secret_env_var("local-db"), "SAFESELECT_PASSWORD_LOCAL_DB");
    }

    #[test]
    fn builds_keychain_command_with_account_and_password() {
        let command = keychain_command("project/local", "secret");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), "security");
        assert_eq!(
            args,
            [
                "add-generic-password",
                "-a",
                "project/local",
                "-s",
                "safeselect",
                "-w",
                "secret",
                "-U",
            ]
        );
    }

    #[test]
    fn reports_keychain_store_status_without_panicking() {
        report_keychain_store_result(&std::process::Command::new("true").output().unwrap());
        report_keychain_store_result(&std::process::Command::new("false").output().unwrap());
    }

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

    #[test]
    fn explains_empty_import_without_agent_step() {
        let guidance = build_guidance_from_parts("project", &[], &[], false);

        assert!(guidance
            .text
            .contains("All environments already exist. Nothing imported."));
        assert!(!guidance.text.contains("agent install"));
    }

    #[test]
    fn parses_dotenv_comments_exports_and_quotes() {
        let values = parse_dotenv(
            "\n# ignored\nexport USER=reader\nPASSWORD=\"secret\"\nEMPTY_KEY=\ninvalid\n=missing\n",
        );

        assert_eq!(values.get("USER").map(String::as_str), Some("reader"));
        assert_eq!(values.get("PASSWORD").map(String::as_str), Some("secret"));
        assert_eq!(values.get("EMPTY_KEY").map(String::as_str), Some(""));
        assert!(!values.contains_key("invalid"));
    }

    #[test]
    fn parses_environment_key_value_list() {
        let values = parse_env_list(&[
            "DB_HOST=localhost".into(),
            " DB_PORT = 5432 ".into(),
            "invalid".into(),
        ]);
        assert_eq!(values.get("DB_HOST").map(String::as_str), Some("localhost"));
        assert_eq!(values.get("DB_PORT").map(String::as_str), Some("5432"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn builds_keychain_delete_command() {
        let command = delete_keychain_command("project/local");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(command.get_program(), "security");
        assert_eq!(
            args,
            [
                "delete-generic-password",
                "-a",
                "project/local",
                "-s",
                "safeselect"
            ]
        );
    }

    #[test]
    fn finds_compose_files_recursively_without_build_directories() {
        let root =
            std::env::temp_dir().join(format!("safeselect-compose-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("compose.yml"), "").unwrap();
        std::fs::write(root.join("nested/docker-compose.yaml"), "").unwrap();
        std::fs::write(root.join("target/compose.yml"), "").unwrap();
        std::fs::write(root.join("node_modules/compose.yml"), "").unwrap();
        std::fs::write(root.join(".hidden/compose.yml"), "").unwrap();

        let files = find_compose_files(&root);

        assert_eq!(files.len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_postgres_image_variants() {
        assert!(is_postgres_image("postgres:17"));
        assert!(is_postgres_image("postgis/postgis:latest"));
        assert!(is_postgres_image("timescale/timescaledb:latest"));
        assert!(!is_postgres_image("mysql:8"));
    }

    #[test]
    fn parses_unquoted_dotenv_values() {
        assert_eq!(
            parse_dotenv_line("export PORT=5432"),
            Some(("PORT".into(), "5432".into()))
        );
        assert_eq!(parse_dotenv_line("# comment"), None);
    }

    #[test]
    fn preserves_empty_environment_values() {
        assert_eq!(
            parse_env_list(&["EMPTY=".into()]).get("EMPTY"),
            Some(&String::new())
        );
    }
}
