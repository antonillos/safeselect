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
            let name = map
                .get("name")
                .or_else(|| map.get("favoriteName"))
                .or_else(|| map.get("connectionName"))
                .and_then(|v| v.as_str())
                .unwrap_or("mongodb")
                .to_string();

            for key in ["connectionString", "connection_string", "uri", "url"] {
                if let Some(url) = map.get(key).and_then(|v| v.as_str()) {
                    if is_mongodb_url(url) {
                        connections.push(CompassConnection {
                            name: name.clone(),
                            url: url.to_string(),
                            ssh_host: find_string(
                                map,
                                &["sshTunnelHostname", "sshHost", "hostname"],
                            ),
                            ssh_port: find_u16(map, &["sshTunnelPort", "sshPort"]),
                            ssh_user: find_string(
                                map,
                                &["sshTunnelUsername", "sshUsername", "username"],
                            ),
                            ssh_local_host: find_string(
                                map,
                                &["sshTunnelLocalHost", "sshLocalHost"],
                            ),
                            ssh_local_port: find_u16(map, &["sshTunnelLocalPort", "sshLocalPort"]),
                            ssh_key_file: find_string(
                                map,
                                &[
                                    "sshTunnelIdentityKeyFile",
                                    "sshIdentityKeyFile",
                                    "identityKeyFile",
                                ],
                            ),
                            ssh_auth_type: find_string(
                                map,
                                &["sshTunnelAuthenticationMethod", "sshAuthType"],
                            ),
                        });
                    }
                }
            }

            for child in map.values() {
                collect_connections(child, connections);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_connections(item, connections);
            }
        }
        serde_json::Value::String(url) if is_mongodb_url(url) => {
            connections.push(CompassConnection {
                name: "mongodb".to_string(),
                url: url.to_string(),
                ssh_host: None,
                ssh_port: None,
                ssh_user: None,
                ssh_local_host: None,
                ssh_local_port: None,
                ssh_key_file: None,
                ssh_auth_type: None,
            });
        }
        _ => {}
    }
}

fn find_string(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for (key, value) in map {
        if keys
            .iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
        {
            if let Some(value) = value.as_str().filter(|value| !value.is_empty()) {
                return Some(value.to_string());
            }
        }
        match value {
            serde_json::Value::Object(child) => {
                if let Some(found) = find_string(child, keys) {
                    return Some(found);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if let serde_json::Value::Object(child) = item {
                        if let Some(found) = find_string(child, keys) {
                            return Some(found);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_u16(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<u16> {
    for (key, value) in map {
        if keys
            .iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
        {
            if let Some(value) = value.as_u64().and_then(|value| u16::try_from(value).ok()) {
                return Some(value);
            }
            if let Some(value) = value.as_str().and_then(|value| value.parse().ok()) {
                return Some(value);
            }
        }
        match value {
            serde_json::Value::Object(child) => {
                if let Some(found) = find_u16(child, keys) {
                    return Some(found);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    if let serde_json::Value::Object(child) = item {
                        if let Some(found) = find_u16(child, keys) {
                            return Some(found);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
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
}
