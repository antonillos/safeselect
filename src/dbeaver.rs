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
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_user: Option<String>,
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
    #[serde(default)]
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
    #[serde(default)]
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

fn parse_data_sources(content: &str) -> Result<Vec<DBeaverConnection>> {
    let config: DBeaverConfig = serde_json::from_str(content)?;

    let sources = config.connections.into_vec();

    let mut connections = vec![];

    for src in sources {
        let cfg = src.configuration.as_ref();

        let host = src
            .host
            .or_else(|| cfg.and_then(|c| c.host.clone()))
            .unwrap_or_default();

        if host.is_empty() {
            continue;
        }

        let port_str = src
            .port
            .or_else(|| cfg.and_then(|c| c.port.clone()))
            .unwrap_or_else(|| "5432".into());

        let port = port_str.parse::<u16>().unwrap_or(5432);

        let database = src
            .database
            .or_else(|| cfg.and_then(|c| c.database.clone()))
            .unwrap_or_default();

        let username = src
            .username
            .or_else(|| cfg.and_then(|c| c.user_name.clone()))
            .unwrap_or_default();

        let name = src.name.unwrap_or_else(|| format!("{host}/{database}"));

        if src.password.is_some() {
            eprintln!("WARN: Skipping password for '{name}' — SafeSelect does not import credentials");
        }

        let (ssh_host, ssh_port, ssh_user) = if let Some(handlers) = cfg.and_then(|c| c.handlers.as_ref()) {
            if let Some(tunnel) = handlers.get("ssh_tunnel") {
                let enabled = tunnel.enabled.unwrap_or(false);
                if enabled {
                    let props = tunnel.properties.as_ref();
                    let sh = props.and_then(|p| p.get("host")).and_then(|v| v.as_str()).map(|s| s.to_string());
                    let sp = props.and_then(|p| p.get("port")).and_then(|v| v.as_f64()).map(|n| n as u16);
                    let su = props.and_then(|p| p.get("userName")).and_then(|v| v.as_str()).map(|s| s.to_string());
                    (sh, sp, su)
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        };

        connections.push(DBeaverConnection {
            name,
            host,
            port,
            database,
            driver: src.driver.unwrap_or_default(),
            username,
            ssh_host,
            ssh_port,
            ssh_user,
        });
    }

    Ok(connections)
}
