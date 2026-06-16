use crate::error::Result;
use std::io::Read;
use std::path::Path;

pub struct DBeaverConnection {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub driver: String,
    pub username: String,
}

pub fn import_zip(zip_path: &Path) -> Result<Vec<DBeaverConnection>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let mut connections = vec![];

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();

        if name == ".dbeaver/data-sources.json" || name.ends_with("/data-sources.json") {
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
    connections: Vec<DBeaverRawConnection>,
    #[serde(default)]
    #[serde(alias = "data-sources")]
    data_sources: Vec<DBeaverRawConnection>,
}

#[derive(serde::Deserialize)]
struct DBeaverRawConnection {
    #[serde(default)]
    connection_id: Option<String>,
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

#[derive(serde::Deserialize)]
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
}

fn parse_data_sources(content: &str) -> Result<Vec<DBeaverConnection>> {
    let config: DBeaverConfig = serde_json::from_str(content)?;

    let sources = if !config.connections.is_empty() {
        config.connections
    } else {
        config.data_sources
    };

    let mut connections = vec![];

    for src in sources {
        let cfg = src.configuration.as_ref();

        let host = src
            .host
            .or_else(|| cfg.and_then(|c| c.host.clone()))
            .unwrap_or_default();

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

        if host.is_empty() {
            continue;
        }

        if src.password.is_some() {
            eprintln!("WARN: Skipping password for '{name}' — SafeSelect does not import credentials");
        }

        connections.push(DBeaverConnection {
            name,
            host,
            port,
            database,
            driver: src.driver.unwrap_or_default(),
            username,
        });
    }

    Ok(connections)
}
