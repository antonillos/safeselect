use crate::error::Result;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

#[derive(Debug)]
pub struct DBeaverConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub driver: String,
    pub username: String,
    pub password: Option<String>,
    pub sslmode: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_user: Option<String>,
    pub ssh_local_host: Option<String>,
    pub ssh_local_port: Option<u16>,
    pub ssh_key_file: Option<String>,
    pub ssh_auth_type: Option<String>,
}

pub fn import_zip(zip_path: &Path) -> Result<Vec<DBeaverConnection>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let mut connections = vec![];

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();

        if name.ends_with("/data-sources.json") {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            connections = parse_data_sources(&content)?;
        }
    }

    Ok(connections)
}

/// Password-independent identity, including the remote endpoint and bastion.
pub fn fingerprint(conn: &DBeaverConnection) -> String {
    use sha2::{Digest, Sha256};
    let identity = serde_json::json!([
        conn.host,
        conn.port,
        conn.database,
        conn.driver,
        conn.username,
        conn.sslmode,
        conn.ssh_host,
        conn.ssh_port.unwrap_or(22),
        conn.ssh_user
    ]);
    hex::encode(Sha256::digest(identity.to_string().as_bytes()))
}

pub fn candidates(
    dir: &Path,
    index: &crate::compass_import::ImportIndex,
    conn: &DBeaverConnection,
    project: &crate::config::ProjectConfig,
) -> Vec<String> {
    let mut names = index.indexed_candidates(dir, &fingerprint(conn));
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
                continue;
            }
            let existing = (|| -> crate::error::Result<crate::config::EnvironmentConfig> {
                let mut env = toml::from_str(&std::fs::read_to_string(&path)?)?;
                crate::config::merge_project_ssh(project, &mut env)?;
                Ok(env)
            })();
            if existing.is_ok_and(|env| legacy_match(conn, &env)) {
                names.push(name.into());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn legacy_match(conn: &DBeaverConnection, env: &crate::config::EnvironmentConfig) -> bool {
    let ssh = env.ssh.as_ref().filter(|s| s.enabled);
    same_legacy_database(conn, &env.database)
        && same_legacy_tunnel(conn, ssh)
        && legacy_database_url_matches(conn, env, ssh)
}

fn same_legacy_database(
    conn: &DBeaverConnection,
    database: &crate::config::DatabaseConfig,
) -> bool {
    database.kind == crate::backend::BackendKind::Jdbc
        && database.username == conn.username
        && database.driver.as_deref() == Some(conn.driver.as_str())
}

fn same_legacy_tunnel(conn: &DBeaverConnection, ssh: Option<&crate::config::SshConfig>) -> bool {
    match (conn.ssh_host.as_ref(), ssh) {
        (None, None) => true,
        (Some(host), Some(ssh)) => {
            ssh.host.as_ref() == Some(host)
                && ssh.port.unwrap_or(22) == conn.ssh_port.unwrap_or(22)
                && ssh.username == conn.ssh_user
                && ssh.forward_host.as_ref() == Some(&conn.host)
                && ssh.forward_port == Some(conn.port)
        }
        _ => false,
    }
}

fn legacy_database_url_matches(
    conn: &DBeaverConnection,
    env: &crate::config::EnvironmentConfig,
    ssh: Option<&crate::config::SshConfig>,
) -> bool {
    if env.database.url == database_url(conn, ssh) {
        return true;
    }
    // Older direct imports omitted sslmode; tunneled imports already kept it.
    ssh.is_none()
        && conn.sslmode.is_some()
        && env.database.url
            == format!(
                "jdbc:postgresql://{}:{}/{}",
                conn.host, conn.port, conn.database
            )
}

pub fn database_url(conn: &DBeaverConnection, ssh: Option<&crate::config::SshConfig>) -> String {
    let (host, port) = ssh.map_or((conn.host.as_str(), conn.port), |ssh| {
        (
            ssh.local_host.as_deref().unwrap_or("localhost"),
            ssh.local_port.unwrap_or(crate::DEFAULT_SSH_LOCAL_PORT),
        )
    });
    let sslmode = conn
        .sslmode
        .as_deref()
        .map(|s| format!("?sslmode={s}"))
        .unwrap_or_default();
    format!("jdbc:postgresql://{host}:{port}/{}{sslmode}", conn.database)
}

#[derive(serde::Deserialize)]
struct DBeaverConfig {
    #[serde(default)]
    connections: ConnectionsField,
    #[serde(default, alias = "data-sources")]
    data_sources: ConnectionsField,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum ConnectionsField {
    List(Vec<DBeaverRawConnection>),
    Map(HashMap<String, DBeaverRawConnection>),
}

impl Default for ConnectionsField {
    fn default() -> Self {
        ConnectionsField::List(vec![])
    }
}

impl ConnectionsField {
    fn into_vec(self) -> Vec<DBeaverRawConnection> {
        match self {
            ConnectionsField::List(v) => v,
            ConnectionsField::Map(m) => m.into_values().collect(),
        }
    }
}

#[derive(serde::Deserialize, Debug)]
struct DBeaverRawConnection {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default, alias = "userName")]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    configuration: Option<DBeaverConfiguration>,
}

#[derive(serde::Deserialize, Debug)]
struct DBeaverConfiguration {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    driver: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default, alias = "userName")]
    user_name: Option<String>,
    #[serde(default)]
    handlers: Option<HashMap<String, DBeaverHandler>>,
}

#[derive(serde::Deserialize, Debug)]
struct DBeaverHandler {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    properties: Option<HashMap<String, serde_json::Value>>,
}

fn normalize_driver(driver: &str) -> String {
    match driver.to_lowercase().as_str() {
        "postgres-jdbc" | "postgresql" | "postgres" => "postgresql".to_string(),
        other => other.to_string(),
    }
}

fn parse_data_sources(content: &str) -> Result<Vec<DBeaverConnection>> {
    let config: DBeaverConfig = serde_json::from_str(content)?;

    let mut sources = config.connections.into_vec();
    sources.extend(config.data_sources.into_vec());

    let mut connections = vec![];

    for src in sources {
        let cfg = src.configuration.as_ref();

        let jdbc_url = src.url.clone().or_else(|| cfg.and_then(|c| c.url.clone()));
        let parsed_url = jdbc_url.as_deref().and_then(parse_postgres_jdbc_url);
        let sslmode = jdbc_url.as_deref().and_then(parse_sslmode);

        // A DBeaver connection configured by URL can retain stale individual
        // host, port, and database fields. Treat the parsed JDBC URL as the
        // authoritative source whenever it is available.
        let host = parsed_url
            .as_ref()
            .map(|p| p.host.clone())
            .or(src.host)
            .or_else(|| cfg.and_then(|c| c.host.clone()))
            .unwrap_or_default();

        if host.is_empty() {
            continue;
        }

        let port_str = parsed_url
            .as_ref()
            .map(|p| p.port.to_string())
            .or(src.port)
            .or_else(|| cfg.and_then(|c| c.port.clone()))
            .unwrap_or_else(|| "5432".into());

        let port = port_str.parse::<u16>().unwrap_or(5432);

        let database = parsed_url
            .as_ref()
            .map(|p| p.database.clone())
            .or(src.database)
            .or_else(|| cfg.and_then(|c| c.database.clone()))
            .unwrap_or_default();

        let username = src
            .username
            .or_else(|| cfg.and_then(|c| c.user_name.clone()))
            .unwrap_or_default();

        let name = src.name.unwrap_or_else(|| format!("{host}/{database}"));

        let password = src.password.clone();

        let (
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_local_host,
            ssh_local_port,
            ssh_key_file,
            ssh_auth_type,
        ) = if let Some(handlers) = cfg.and_then(|c| c.handlers.as_ref()) {
            if let Some(tunnel) = handlers.get("ssh_tunnel") {
                let enabled = tunnel.enabled.unwrap_or(false);
                if enabled {
                    let props = tunnel.properties.as_ref();
                    let sh = props
                        .and_then(|p| p.get("#host").or_else(|| p.get("host")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let sp = props
                        .and_then(|p| p.get("#port").or_else(|| p.get("port")))
                        .and_then(|v| v.as_f64())
                        .map(|n| n as u16);
                    let su = props
                        .and_then(|p| p.get("#user").or_else(|| p.get("userName")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let slh = props
                        .and_then(|p| p.get("#localHost").or_else(|| p.get("localHost")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let slp = props
                        .and_then(|p| p.get("#localPort").or_else(|| p.get("localPort")))
                        .and_then(|v| v.as_f64())
                        .map(|n| n as u16);
                    let skf = props
                        .and_then(|p| p.get("#keyFile").or_else(|| p.get("keyFile")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let sat = props
                        .and_then(|p| p.get("#authType").or_else(|| p.get("authType")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    (sh, sp, su, slh, slp, skf, sat)
                } else {
                    (None, None, None, None, None, None, None)
                }
            } else {
                (None, None, None, None, None, None, None)
            }
        } else {
            (None, None, None, None, None, None, None)
        };

        connections.push(DBeaverConnection {
            name,
            host,
            port,
            database,
            driver: src
                .driver
                .as_deref()
                .map(normalize_driver)
                .unwrap_or_default(),
            username,
            password,
            sslmode,
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_local_host,
            ssh_local_port,
            ssh_key_file,
            ssh_auth_type,
        });
    }

    Ok(connections)
}

struct ParsedJdbcUrl {
    host: String,
    port: u16,
    database: String,
}

fn parse_postgres_jdbc_url(url: &str) -> Option<ParsedJdbcUrl> {
    let without_prefix = url.strip_prefix("jdbc:postgresql://")?;
    let (host_port, rest) = without_prefix.split_once('/')?;
    let database = rest.split('?').next().unwrap_or(rest).to_string();
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse::<u16>().unwrap_or(5432)),
        None => (host_port.to_string(), 5432),
    };

    if host.is_empty() || database.is_empty() {
        return None;
    }

    Some(ParsedJdbcUrl {
        host,
        port,
        database,
    })
}

fn parse_sslmode(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|parameter| {
        let (key, value) = parameter.split_once('=')?;
        key.eq_ignore_ascii_case("sslmode")
            .then(|| value.to_string())
    })
}

#[cfg(test)]
#[path = "tests/dbeaver.rs"]
mod tests;
