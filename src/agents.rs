use crate::error::{Result, SafeselectError};
use std::path::PathBuf;

pub fn detect_clients() -> Result<Vec<ClientConfig>> {
    let mut clients = vec![];

    let candidates: Vec<(&str, fn() -> Option<PathBuf>)> = vec![
        ("opencode", detect_opencode_config),
        ("copilot", detect_copilot_config),
        ("cursor", detect_cursor_config),
        ("windsurf", detect_windsurf_config),
        ("claude-code", detect_claude_code_config),
        ("codex", detect_codex_config),
        ("gemini-cli", detect_gemini_config),
    ];

    for (name, detector) in candidates {
        let path = detector();
        clients.push(ClientConfig {
            name: name.to_string(),
            config_path: path.clone().unwrap_or_default(),
            detected: path.is_some(),
        });
    }

    Ok(clients)
}

pub struct ClientConfig {
    pub name: String,
    pub config_path: PathBuf,
    pub detected: bool,
}

pub fn install_entry(client: &str, project: &str, environment: &str, entry_name: &str) -> Result<()> {
    let config_path = get_client_config(client)?;
    let content = std::fs::read_to_string(&config_path)?;

    verify_permissions(&config_path)?;

    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(&config_path, &backup_path)?;

    let entry = serde_json::json!({
        "command": "safeselect",
        "args": ["serve", "--project", project, "--environment", environment]
    });

    let new_content = match client {
        "opencode" | "cursor" | "windsurf" | "codex" | "claude-code" => {
            append_mcp_json(&content, &entry, entry_name)?
        }
        "copilot" | "gemini-cli" => {
            append_ini_entry(&content, entry_name)?
        }
        _ => return Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    };

    println!("--- Config diff for {client} ({}) ---", config_path.display());
    show_diff(&content, &new_content);
    println!("\nBackup saved to: {}", backup_path.display());

    std::fs::write(&config_path, &new_content)?;

    let verify = std::fs::read_to_string(&config_path)?;
    if verify != new_content {
        std::fs::write(&config_path, &content)?;
        return Err(SafeselectError::Other("Write verification failed, rolled back".into()));
    }

    println!("Entry '{entry_name}' installed for {client}");
    Ok(())
}

pub fn uninstall_entry(client: &str, entry_name: &str) -> Result<()> {
    let config_path = get_client_config(client)?;
    let content = std::fs::read_to_string(&config_path)?;

    if !content.contains(entry_name) && !content.contains("safeselect") {
        return Err(SafeselectError::Other(format!(
            "No safeselect entry found in {client} config"
        )));
    }

    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(&config_path, &backup_path)?;

    let new_content = remove_mcp_entry(&content, entry_name)?;
    std::fs::write(&config_path, &new_content)?;

    println!("Entry '{entry_name}' uninstalled from {client}");
    Ok(())
}

fn get_client_config(client: &str) -> Result<PathBuf> {
    let detector = match client {
        "opencode" => detect_opencode_config as fn() -> Option<PathBuf>,
        "copilot" => detect_copilot_config,
        "cursor" => detect_cursor_config,
        "windsurf" => detect_windsurf_config,
        "claude-code" => detect_claude_code_config,
        "codex" => detect_codex_config,
        "gemini-cli" => detect_gemini_config,
        c => return Err(SafeselectError::Other(format!("Unknown client: {c}"))),
    };

    detector().ok_or_else(|| SafeselectError::Other(format!("{client} not found on this system")))
}

fn detect_opencode_config() -> Option<PathBuf> {
    let config = dirs::config_dir()?.join("opencode").join("opencode.json");
    if config.exists() { Some(config) } else { None }
}

fn detect_copilot_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let candidates = vec![
        home.join(".vscode").join("argv.json"),
        home.join(".config").join("Code").join("User").join("argv.json"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn detect_cursor_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    [home.join(".cursor").join("config.json"), home.join(".config").join("cursor").join("config.json")]
        .into_iter().find(|p| p.exists())
}

fn detect_windsurf_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".windsurf").join("config.json");
    if path.exists() { Some(path) } else { None }
}

fn detect_claude_code_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("config.json");
    if path.exists() { Some(path) } else { None }
}

fn detect_codex_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".codex").join("config.json");
    if path.exists() { Some(path) } else { None }
}

fn detect_gemini_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".gemini").join("config.json");
    if path.exists() { Some(path) } else { None }
}

fn verify_permissions(path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = path.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(SafeselectError::Other(format!(
                "Config file is a symlink: {}", path.display()
            )));
        }
        let mode = meta.permissions().mode();
        if mode & 0o002 != 0 || mode & 0o020 != 0 {
            return Err(SafeselectError::Other(format!(
                "Config file has unsafe permissions (group/world writable): {}", path.display()
            )));
        }
    }
    Ok(())
}

fn append_mcp_json(content: &str, entry: &serde_json::Value, name: &str) -> Result<String> {
    let mut config: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;

    let servers = config
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut());

    match servers {
        Some(map) => {
            if map.contains_key(name) {
                return Err(SafeselectError::Other(format!(
                    "Entry '{name}' already exists in mcpServers"
                )));
            }
            map.insert(name.to_string(), entry.clone());
        }
        None => {
            let mut map = serde_json::Map::new();
            map.insert(name.to_string(), entry.clone());
            config["mcpServers"] = serde_json::Value::Object(map);
        }
    }

    Ok(serde_json::to_string_pretty(&config)?)
}

fn append_ini_entry(content: &str, name: &str) -> Result<String> {
    Ok(format!(
        "{}\n\n[mcpServers.{}]\ncommand = safeselect\nargs = [\"serve\", \"--project\", \"<project>\", \"--environment\", \"<env>\"]\n",
        content.trim(),
        name
    ))
}

fn remove_mcp_entry(content: &str, name: &str) -> Result<String> {
    if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            servers.remove(name);
        }
        Ok(serde_json::to_string_pretty(&config)?)
    } else {
        Ok(remove_text_block(content, name))
    }
}

fn remove_text_block(content: &str, name: &str) -> String {
    content
        .lines()
        .filter(|line| {
            !line.contains(name) && !line.contains("safeselect")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_diff(old: &str, new: &str) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(old, new);
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        print!("{}{}", sign, change.value());
    }
}
