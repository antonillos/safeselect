use crate::error::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CompassConnection {
    pub name: String,
    pub url: String,
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
            });
        }
        _ => {}
    }
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
