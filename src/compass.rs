use crate::error::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CompassConnection {
    pub name: String,
    pub url: String,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_user: Option<String>,
    pub ssh_local_host: Option<String>,
    pub ssh_local_port: Option<u16>,
    pub ssh_key_file: Option<String>,
    pub ssh_auth_type: Option<String>,
}

pub fn import_path(path: &Path) -> Result<Vec<CompassConnection>> {
    let mut files = vec![];
    collect_json_files(path, &mut files)?;

    let mut connections = vec![];
    for file in files {
        let content = std::fs::read_to_string(&file)?;
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        collect_connections(&json, &mut connections);
    }
    dedupe_connections(connections)
}

fn collect_json_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_file() {
        files.push(path.to_path_buf());
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_connections(value: &serde_json::Value, connections: &mut Vec<CompassConnection>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(connection) = parse_connection_object(map) {
                connections.push(connection);
            }

            for child in map.values() {
                if !matches!(child, serde_json::Value::String(_)) {
                    collect_connections(child, connections);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_connections(item, connections);
            }
        }
        _ => {}
    }
}

fn parse_connection_object(
    map: &serde_json::Map<String, serde_json::Value>,
) -> Option<CompassConnection> {
    let url = map
        .get("connectionOptions")
        .and_then(|value| value.as_object())
        .and_then(|options| {
            options
                .get("connectionString")
                .or_else(|| options.get("connection_string"))
                .or_else(|| options.get("uri"))
                .or_else(|| options.get("url"))
        })
        .and_then(|value| value.as_str())
        .or_else(|| {
            ["connectionString", "connection_string", "uri", "url"]
                .iter()
                .find_map(|key| map.get(*key).and_then(|value| value.as_str()))
        })?;

    if !is_mongodb_url(url) {
        return None;
    }

    let favorite = map.get("favorite").and_then(|value| value.as_object());
    let name = favorite
        .and_then(|favorite| favorite.get("name"))
        .or_else(|| map.get("name"))
        .or_else(|| map.get("favoriteName"))
        .or_else(|| map.get("connectionName"))
        .and_then(|value| value.as_str())
        .unwrap_or("mongodb")
        .to_string();

    let ssh_tunnel = map
        .get("connectionOptions")
        .and_then(|value| value.as_object())
        .and_then(|options| options.get("sshTunnel"))
        .and_then(|value| value.as_object());

    Some(CompassConnection {
        name,
        url: url.to_string(),
        ssh_host: ssh_tunnel
            .and_then(|ssh| ssh.get("host"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ssh_port: ssh_tunnel
            .and_then(|ssh| ssh.get("port"))
            .and_then(parse_u16_value),
        ssh_user: ssh_tunnel
            .and_then(|ssh| ssh.get("username"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ssh_local_host: ssh_tunnel
            .and_then(|ssh| ssh.get("localHost"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ssh_local_port: ssh_tunnel
            .and_then(|ssh| ssh.get("localPort"))
            .and_then(parse_u16_value),
        ssh_key_file: ssh_tunnel
            .and_then(|ssh| ssh.get("identityKeyFile"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        ssh_auth_type: ssh_tunnel
            .and_then(|ssh| ssh.get("authenticationMethod"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn parse_u16_value(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn is_mongodb_url(value: &str) -> bool {
    value.starts_with("mongodb://") || value.starts_with("mongodb+srv://")
}

fn dedupe_connections(connections: Vec<CompassConnection>) -> Result<Vec<CompassConnection>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut deduped = vec![];
    for connection in connections {
        if seen.insert(connection.url.clone()) {
            deduped.push(connection);
        }
    }
    Ok(deduped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_connection_strings_from_json_file() {
        let dir =
            std::env::temp_dir().join(format!("safeselect-compass-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("connections.json");
        std::fs::write(
            &file,
            r#"{
              "connections": [
                {
                  "name": "Local Mongo",
                  "connectionString": "mongodb://localhost:27017/app"
                }
              ]
            }"#,
        )
        .unwrap();

        let connections = import_path(&file).unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].name, "Local Mongo");
        assert_eq!(connections[0].url, "mongodb://localhost:27017/app");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_compass_favorite_name_and_ssh_tunnel() {
        let dir = std::env::temp_dir().join(format!(
            "safeselect-compass-test-ssh-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("connections.json");
        std::fs::write(
            &file,
            r#"{
              "connections": [
                {
                  "favorite": { "name": "iopcompclopre002 (pre)" },
                  "connectionOptions": {
                    "connectionString": "mongodb+srv://user@cluster.mongodb.net/?readPreferenceTags=nodeType%3Areadonly",
                    "sshTunnel": {
                      "host": "localhost",
                      "port": "2222",
                      "username": "jumpboxdev"
                    }
                  }
                }
              ]
            }"#,
        )
        .unwrap();

        let connections = import_path(&file).unwrap();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].name, "iopcompclopre002 (pre)");
        assert_eq!(connections[0].ssh_host.as_deref(), Some("localhost"));
        assert_eq!(connections[0].ssh_port, Some(2222));
        assert_eq!(connections[0].ssh_user.as_deref(), Some("jumpboxdev"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
