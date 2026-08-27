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
mod tests {
    use super::*;

    #[test]
    fn normalizes_postgres_driver_aliases() {
        assert_eq!(normalize_driver("postgres"), "postgresql");
        assert_eq!(normalize_driver("POSTGRES-JDBC"), "postgresql");
        assert_eq!(normalize_driver("mysql"), "mysql");
    }

    #[test]
    fn converts_connection_lists_and_maps_to_vectors() {
        let list = ConnectionsField::List(vec![]).into_vec();
        let map = ConnectionsField::Map(HashMap::new()).into_vec();
        assert!(list.is_empty());
        assert!(map.is_empty());
    }

    #[test]
    fn parses_postgres_jdbc_url_with_optional_port() {
        let parsed = parse_postgres_jdbc_url("jdbc:postgresql://db.example:5433/app").unwrap();
        assert_eq!(parsed.host, "db.example");
        assert_eq!(parsed.port, 5433);
        assert_eq!(parsed.database, "app");
        assert!(parse_postgres_jdbc_url("jdbc:mysql://db/app").is_none());
    }

    #[test]
    fn rejects_incomplete_postgres_jdbc_url() {
        assert!(parse_postgres_jdbc_url("jdbc:postgresql://").is_none());
        assert!(parse_postgres_jdbc_url("jdbc:postgresql://db").is_none());
    }

    #[test]
    fn preserves_explicit_sslmode_from_jdbc_url() {
        assert_eq!(
            parse_sslmode("jdbc:postgresql://db:5432/app?sslmode=require&connectTimeout=5"),
            Some("require".to_string())
        );
        assert_eq!(parse_sslmode("jdbc:postgresql://db:5432/app"), None);
    }

    #[test]
    fn parses_dbeaver_sources_from_list_and_map_shapes() {
        let content = r#"{
          "connections": [{"name":"list","host":"db1","port":"5432","database":"app","driver":"postgres"}],
          "data-sources": {"map": {"name":"map","url":"jdbc:postgresql://db2:5433/app"}}
        }"#;
        let connections = parse_data_sources(content).unwrap();
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].driver, "postgresql");
        assert_eq!(connections[1].host, "db2");
        assert_eq!(connections[1].port, 5433);
    }

    #[test]
    fn prefers_jdbc_url_when_dbeaver_fields_are_stale() {
        let content = r#"{
          "connections": [{
            "name":"url-configured",
            "host":"localhost",
            "port":"5432",
            "database":"postgres",
            "url":"jdbc:postgresql://localhost:15432/safeselect_demo"
          }]
        }"#;

        let connections = parse_data_sources(content).unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].host, "localhost");
        assert_eq!(connections[0].port, 15432);
        assert_eq!(connections[0].database, "safeselect_demo");
    }

    #[test]
    fn applies_nested_configuration_fallbacks_and_skips_sources_without_hosts() {
        let content = r###"{
          "connections": [
            {"name":"nested","configuration":{"host":"db","port":"bad","database":"app","driver":"postgres","userName":"agent","handlers":{"ssh_tunnel":{"enabled":true,"properties":{"#host":"jumpbox","#port":2222,"#user":"tunnel-user","#localHost":"127.0.0.1","#localPort":15432,"#keyFile":"/tmp/id_ed25519","#authType":"KEY"}}}}},
            {"name":"missing-host","database":"ignored"}
          ],
          "data-sources": []
        }"###;
        let connections = parse_data_sources(content).unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].name, "nested");
        assert_eq!(connections[0].host, "db");
        assert_eq!(connections[0].port, 5432);
        assert_eq!(connections[0].database, "app");
        assert_eq!(connections[0].username, "agent");
        assert_eq!(connections[0].driver, "");
        assert_eq!(connections[0].ssh_host.as_deref(), Some("jumpbox"));
        assert_eq!(connections[0].ssh_port, Some(2222));
        assert_eq!(connections[0].ssh_user.as_deref(), Some("tunnel-user"));
        assert_eq!(connections[0].ssh_local_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(connections[0].ssh_local_port, Some(15432));
        assert_eq!(
            connections[0].ssh_key_file.as_deref(),
            Some("/tmp/id_ed25519")
        );
        assert_eq!(connections[0].ssh_auth_type.as_deref(), Some("KEY"));
    }

    #[test]
    fn imports_data_sources_from_a_zip_archive() {
        let path =
            std::env::temp_dir().join(format!("safeselect-dbeaver-{}.zip", uuid::Uuid::new_v4()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "workspace/readme.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, b"not a data source").unwrap();
        zip.start_file(
            "workspace/data-sources.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"connections":[{"name":"local","host":"localhost","database":"app"}]}"#,
        )
        .unwrap();
        zip.finish().unwrap();

        let connections = import_zip(&path).unwrap();
        assert_eq!(connections[0].name, "local");
        let _ = std::fs::remove_file(path);
    }
}
