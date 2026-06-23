use crate::error::{Result, SafeselectError};
use std::path::{Path, PathBuf};

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

pub fn install_entry(
    client: &str,
    environment: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    mcp_timeout_ms: u64,
    local: bool,
) -> Result<()> {
    let config_path = if local {
        get_local_client_config(client, repo_root)?
    } else {
        // Auto-detect local config and ask user
        if let Some(root) = repo_root {
            if let Some(local_path) = detect_local_client_config(client, root) {
                println!("Found local config: {}", local_path.display());
                println!("Install to local project config instead of global? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().eq_ignore_ascii_case("y") {
                    println!("Installing to local config...");
                    local_path
                } else {
                    get_client_config(client)?
                }
            } else {
                get_client_config(client)?
            }
        } else {
            get_client_config(client)?
        }
    };
    let content = std::fs::read_to_string(&config_path)?;

    verify_permissions(&config_path)?;

    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(&config_path, &backup_path)?;

    let entry = serde_json::json!({
        "command": "safeselect",
        "args": ["serve", "--environment", environment],
        "timeout": mcp_timeout_ms
    });

    let mut opencode_entry = serde_json::json!({
        "type": "local",
        "command": ["safeselect", "serve", "--environment", environment],
        "timeout": mcp_timeout_ms,
        "enabled": true
    });

    if let Some(root) = repo_root {
        opencode_entry["cwd"] = serde_json::json!(root.to_string_lossy().to_string());
    }

    if let Some(dir) = config_dir {
        opencode_entry["environment"] = serde_json::json!({
            "SAFESELECT_CONFIG_DIR": dir.to_string_lossy().to_string()
        });
    }

    let new_content = match client {
        "opencode" => append_opencode_json(&content, &opencode_entry, entry_name)?,
        "cursor" | "windsurf" | "codex" | "claude-code" => {
            append_mcp_json(&content, &entry, entry_name)?
        }
        "copilot" | "gemini-cli" => append_ini_entry(&content, entry_name, environment)?,
        _ => return Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    };

    println!(
        "--- Config diff for {client} ({}) ---",
        config_path.display()
    );
    show_diff(&content, &new_content);
    println!("\nBackup saved to: {}", backup_path.display());

    std::fs::write(&config_path, &new_content)?;

    let verify = std::fs::read_to_string(&config_path)?;
    if verify != new_content {
        std::fs::write(&config_path, &content)?;
        return Err(SafeselectError::Other(
            "Write verification failed, rolled back".into(),
        ));
    }

    println!("Entry '{entry_name}' installed for {client}");
    Ok(())
}

pub fn upgrade_entry(
    client: &str,
    entry_name: Option<&str>,
    environment: Option<&str>,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    mcp_timeout_ms: u64,
    local: bool,
) -> Result<()> {
    let (config_path, resolved_entry_name) =
        resolve_upgrade_target(client, entry_name, environment, repo_root, local)?;
    let content = std::fs::read_to_string(&config_path)?;

    verify_permissions(&config_path)?;

    let environment = match environment {
        Some(env) => env.to_string(),
        None => {
            detect_entry_environment(client, &content, &resolved_entry_name)?.ok_or_else(|| {
                SafeselectError::Other(format!(
                "Cannot detect environment for entry '{resolved_entry_name}'; use --environment"
            ))
            })?
        }
    };
    let target_entry_name = canonical_entry_name(repo_root, &environment)
        .unwrap_or_else(|| resolved_entry_name.to_string());

    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(&config_path, &backup_path)?;

    let entry = serde_json::json!({
        "command": "safeselect",
        "args": ["serve", "--environment", environment],
        "timeout": mcp_timeout_ms
    });

    let mut opencode_entry = serde_json::json!({
        "type": "local",
        "command": ["safeselect", "serve", "--environment", environment],
        "timeout": mcp_timeout_ms,
        "enabled": true
    });

    if let Some(root) = repo_root {
        opencode_entry["cwd"] = serde_json::json!(root.to_string_lossy().to_string());
    }

    if let Some(dir) = config_dir {
        opencode_entry["environment"] = serde_json::json!({
            "SAFESELECT_CONFIG_DIR": dir.to_string_lossy().to_string()
        });
    }

    let new_content = match client {
        "opencode" => replace_opencode_json(
            &content,
            &opencode_entry,
            &resolved_entry_name,
            &target_entry_name,
        )?,
        "cursor" | "windsurf" | "codex" | "claude-code" => {
            replace_mcp_json(&content, &entry, &resolved_entry_name, &target_entry_name)?
        }
        "copilot" | "gemini-cli" => replace_ini_entry(
            &content,
            &resolved_entry_name,
            &target_entry_name,
            &environment,
        )?,
        _ => return Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    };

    println!(
        "--- Config diff for {client} ({}) ---",
        config_path.display()
    );
    show_diff(&content, &new_content);
    println!("\nBackup saved to: {}", backup_path.display());

    std::fs::write(&config_path, &new_content)?;

    let verify = std::fs::read_to_string(&config_path)?;
    if verify != new_content {
        std::fs::write(&config_path, &content)?;
        return Err(SafeselectError::Other(
            "Write verification failed, rolled back".into(),
        ));
    }

    if target_entry_name == resolved_entry_name {
        println!("Entry '{resolved_entry_name}' upgraded for {client}");
    } else {
        println!(
            "Entry '{resolved_entry_name}' upgraded and renamed to '{target_entry_name}' for {client}"
        );
    }
    Ok(())
}

pub fn uninstall_entry(client: &str, entry_name: &str, repo_root: Option<&Path>) -> Result<()> {
    let config_path = resolve_uninstall_target(client, entry_name, repo_root)?;
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

fn resolve_uninstall_target(
    client: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(root) = repo_root {
        let mut current = Some(root);
        while let Some(dir) = current {
            if let Some(local_path) = detect_local_client_config(client, dir) {
                let content = std::fs::read_to_string(&local_path)?;
                if config_has_entry(client, &content, entry_name)? {
                    return Ok(local_path);
                }
            }
            current = dir.parent();
        }
    }

    let config_path = get_client_config(client)?;
    let content = std::fs::read_to_string(&config_path)?;
    if config_has_entry(client, &content, entry_name)? {
        Ok(config_path)
    } else {
        Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{entry_name}' found in {client} config"
        )))
    }
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

fn detect_local_client_config(client: &str, repo_root: &Path) -> Option<PathBuf> {
    match client {
        "opencode" => {
            let opencode_dir = repo_root.join(".opencode");
            let candidates = [
                opencode_dir.join("opencode.jsonc"),
                opencode_dir.join("opencode.json"),
                opencode_dir.join("config.jsonc"),
                opencode_dir.join("config.json"),
            ];
            candidates.into_iter().find(|p| p.exists())
        }
        "cursor" => {
            let config = repo_root.join(".cursor").join("settings.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "windsurf" => {
            let config = repo_root.join(".windsurf").join("settings.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "claude-code" => {
            let config = repo_root.join(".claude").join("settings.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "codex" => {
            let config = repo_root.join(".codex").join("settings.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn get_local_client_config(client: &str, repo_root: Option<&Path>) -> Result<PathBuf> {
    let root = repo_root.ok_or_else(|| {
        SafeselectError::Other(
            "no project root specified; use --project or run from a project directory".into(),
        )
    })?;

    let local_path = match client {
        "opencode" => {
            let opencode_dir = root.join(".opencode");
            let candidates = [
                opencode_dir.join("opencode.jsonc"),
                opencode_dir.join("opencode.json"),
                opencode_dir.join("config.jsonc"),
                opencode_dir.join("config.json"),
            ];
            if let Some(existing) = candidates.iter().find(|p| p.exists()) {
                existing.clone()
            } else {
                std::fs::create_dir_all(&opencode_dir)?;
                let default_config = serde_json::json!({
                    "mcp": {}
                });
                let target = opencode_dir.join("opencode.jsonc");
                std::fs::write(&target, serde_json::to_string_pretty(&default_config)?)?;
                target
            }
        }
        "cursor" => {
            let cursor_dir = root.join(".cursor");
            let config = cursor_dir.join("settings.json");
            if !config.exists() {
                std::fs::create_dir_all(&cursor_dir)?;
                let default_config = serde_json::json!({
                    "mcpServers": {}
                });
                std::fs::write(&config, serde_json::to_string_pretty(&default_config)?)?;
            }
            config
        }
        "windsurf" => {
            let windsurf_dir = root.join(".windsurf");
            let config = windsurf_dir.join("settings.json");
            if !config.exists() {
                std::fs::create_dir_all(&windsurf_dir)?;
                let default_config = serde_json::json!({
                    "mcpServers": {}
                });
                std::fs::write(&config, serde_json::to_string_pretty(&default_config)?)?;
            }
            config
        }
        "claude-code" => {
            let claude_dir = root.join(".claude");
            let config = claude_dir.join("settings.json");
            if !config.exists() {
                std::fs::create_dir_all(&claude_dir)?;
                let default_config = serde_json::json!({
                    "mcpServers": {}
                });
                std::fs::write(&config, serde_json::to_string_pretty(&default_config)?)?;
            }
            config
        }
        "codex" => {
            let codex_dir = root.join(".codex");
            let config = codex_dir.join("settings.json");
            if !config.exists() {
                std::fs::create_dir_all(&codex_dir)?;
                let default_config = serde_json::json!({
                    "mcpServers": {}
                });
                std::fs::write(&config, serde_json::to_string_pretty(&default_config)?)?;
            }
            config
        }
        c => {
            return Err(SafeselectError::Other(format!(
                "Local config not supported for {c}; use global install (without --local)"
            )))
        }
    };

    Ok(local_path)
}

fn detect_opencode_config() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    let home_config = dirs::home_dir()?.join(".config");
    for base in [&*config_dir, &home_config] {
        let dir = base.join("opencode");
        for name in ["opencode.jsonc", "opencode.json"] {
            let path = dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn detect_copilot_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let candidates = vec![
        home.join(".vscode").join("argv.json"),
        home.join(".config")
            .join("Code")
            .join("User")
            .join("argv.json"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn detect_cursor_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    [
        home.join(".cursor").join("config.json"),
        home.join(".config").join("cursor").join("config.json"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn detect_windsurf_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".windsurf").join("config.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn detect_claude_code_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("config.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn detect_codex_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".codex").join("config.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn detect_gemini_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".gemini").join("config.json");
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn verify_permissions(path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = path.metadata()?;
        if meta.file_type().is_symlink() {
            return Err(SafeselectError::Other(format!(
                "Config file is a symlink: {}",
                path.display()
            )));
        }
        let mode = meta.permissions().mode();
        if mode & 0o002 != 0 || mode & 0o020 != 0 {
            return Err(SafeselectError::Other(format!(
                "Config file has unsafe permissions (group/world writable): {}",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Strip JSONC comments (// and /* */) from a string, preserving string contents.
fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // string literal — copy verbatim
        if bytes[i] == b'"' {
            out.push('"');
            i += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                out.push(c);
                if c == '\\' && i + 1 < bytes.len() {
                    i += 1;
                    out.push(bytes[i] as char);
                } else if c == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // single-line comment
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // block comment
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Parse a JSON or JSONC string into a serde_json::Value.
fn parse_json_or_jsonc(input: &str) -> std::result::Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str(input).or_else(|_| {
        let cleaned = strip_jsonc_comments(input);
        serde_json::from_str(&cleaned)
    })
}

fn append_opencode_json(content: &str, entry: &serde_json::Value, name: &str) -> Result<String> {
    let mut config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;

    let servers = config.get_mut("mcp").and_then(|v| v.as_object_mut());

    match servers {
        Some(map) => {
            map.insert(name.to_string(), entry.clone());
        }
        None => {
            let mut map = serde_json::Map::new();
            map.insert(name.to_string(), entry.clone());
            config["mcp"] = serde_json::Value::Object(map);
        }
    }

    Ok(serde_json::to_string_pretty(&config)?)
}

fn replace_opencode_json(
    content: &str,
    entry: &serde_json::Value,
    current_name: &str,
    target_name: &str,
) -> Result<String> {
    if !json_config_has_entry(content, "mcp", current_name)? {
        return Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{current_name}' found in opencode config"
        )));
    }
    replace_json_entry(content, "mcp", entry, current_name, target_name)
}

fn append_mcp_json(content: &str, entry: &serde_json::Value, name: &str) -> Result<String> {
    let mut config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;

    let servers = config.get_mut("mcpServers").and_then(|v| v.as_object_mut());

    match servers {
        Some(map) => {
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

fn replace_mcp_json(
    content: &str,
    entry: &serde_json::Value,
    current_name: &str,
    target_name: &str,
) -> Result<String> {
    if !json_config_has_entry(content, "mcpServers", current_name)? {
        return Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{current_name}' found in client config"
        )));
    }
    replace_json_entry(content, "mcpServers", entry, current_name, target_name)
}

fn append_ini_entry(content: &str, name: &str, environment: &str) -> Result<String> {
    Ok(format!(
        "{}\n\n[mcpServers.{}]\ncommand = safeselect\nargs = [\"serve\", \"--environment\", \"{environment}\"]\n",
        content.trim(),
        name,
    ))
}

fn replace_ini_entry(
    content: &str,
    current_name: &str,
    target_name: &str,
    environment: &str,
) -> Result<String> {
    if !ini_config_has_entry(content, current_name) {
        return Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{current_name}' found in client config"
        )));
    }

    let without_entry = remove_ini_entry(content, current_name);
    append_ini_entry(&without_entry, target_name, environment)
}

fn replace_json_entry(
    content: &str,
    top_level_key: &str,
    entry: &serde_json::Value,
    current_name: &str,
    target_name: &str,
) -> Result<String> {
    let mut config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;

    let servers = config
        .get_mut(top_level_key)
        .and_then(|value| value.as_object_mut())
        .ok_or_else(|| SafeselectError::Other(format!("Missing '{top_level_key}' section")))?;

    servers.remove(current_name);
    servers.insert(target_name.to_string(), entry.clone());

    Ok(serde_json::to_string_pretty(&config)?)
}

fn resolve_upgrade_target(
    client: &str,
    entry_name: Option<&str>,
    environment: Option<&str>,
    repo_root: Option<&Path>,
    local: bool,
) -> Result<(PathBuf, String)> {
    if let Some(name) = entry_name {
        let config_path = resolve_upgrade_config_path_for_name(client, name, repo_root, local)?;
        return Ok((config_path, name.to_string()));
    }

    let project_name = repo_root
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            SafeselectError::Other(
                "Cannot infer entry name from PWD; use --project, --environment, or --name".into(),
            )
        })?;

    let configs = candidate_upgrade_config_paths(client, repo_root, local)?;
    let mut matches = Vec::new();
    for config_path in configs {
        let content = std::fs::read_to_string(&config_path)?;
        for candidate in candidate_entry_names(client, &content, project_name, environment)? {
            matches.push((config_path.clone(), candidate));
        }
    }

    matches.sort();
    matches.dedup();

    match matches.len() {
        0 => Err(SafeselectError::Other(format!(
            "No SafeSelect entry found for project '{project_name}'; use --name{}",
            if environment.is_none() {
                " or --environment"
            } else {
                ""
            }
        ))),
        1 => Ok(matches.remove(0)),
        _ => Err(SafeselectError::Other(format!(
            "Multiple SafeSelect entries found for project '{project_name}': {}; use --name{}",
            matches
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            if environment.is_none() {
                " or --environment"
            } else {
                ""
            }
        ))),
    }
}

fn resolve_upgrade_config_path_for_name(
    client: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
    local: bool,
) -> Result<PathBuf> {
    if local {
        let config_path = get_local_client_config(client, repo_root)?;
        let content = std::fs::read_to_string(&config_path)?;
        if config_has_entry(client, &content, entry_name)? {
            return Ok(config_path);
        }
        return Err(SafeselectError::Other(format!(
            "Entry '{entry_name}' not found in local {client} config"
        )));
    }

    if let Some(root) = repo_root {
        if let Some(local_path) = detect_local_client_config(client, root) {
            let local_content = std::fs::read_to_string(&local_path)?;
            if config_has_entry(client, &local_content, entry_name)? {
                return Ok(local_path);
            }
        }
    }

    let global_path = get_client_config(client)?;
    let global_content = std::fs::read_to_string(&global_path)?;
    if config_has_entry(client, &global_content, entry_name)? {
        return Ok(global_path);
    }

    Err(SafeselectError::Other(format!(
        "Entry '{entry_name}' not found for {client}"
    )))
}

fn candidate_upgrade_config_paths(
    client: &str,
    repo_root: Option<&Path>,
    local: bool,
) -> Result<Vec<PathBuf>> {
    if local {
        return Ok(vec![get_local_client_config(client, repo_root)?]);
    }

    let mut paths = Vec::new();
    if let Some(root) = repo_root {
        if let Some(local_path) = detect_local_client_config(client, root) {
            paths.push(local_path);
        }
    }
    paths.push(get_client_config(client)?);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn candidate_entry_names(
    client: &str,
    content: &str,
    project_name: &str,
    environment: Option<&str>,
) -> Result<Vec<String>> {
    let canonical_prefix = format!("safeselect-{project_name}-");
    let legacy_prefix = format!("{project_name}-");

    let all_names = match client {
        "opencode" => json_entry_names(content, "mcp")?,
        "cursor" | "windsurf" | "codex" | "claude-code" => json_entry_names(content, "mcpServers")?,
        "copilot" | "gemini-cli" => ini_entry_names(content),
        _ => return Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    };

    let matches = if let Some(env) = environment {
        let canonical = format!("safeselect-{project_name}-{env}");
        let legacy = format!("{project_name}-{env}");
        all_names
            .into_iter()
            .filter(|name| name == &canonical || name == &legacy)
            .collect()
    } else {
        all_names
            .into_iter()
            .filter(|name| name.starts_with(&canonical_prefix) || name.starts_with(&legacy_prefix))
            .collect()
    };

    Ok(matches)
}

fn json_entry_names(content: &str, key: &str) -> Result<Vec<String>> {
    let config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;
    Ok(config
        .get(key)
        .and_then(|v| v.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default())
}

fn ini_entry_names(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("[mcpServers.")
                .and_then(|rest| rest.strip_suffix(']'))
                .map(ToString::to_string)
        })
        .collect()
}

fn config_has_entry(client: &str, content: &str, name: &str) -> Result<bool> {
    match client {
        "opencode" => json_config_has_entry(content, "mcp", name),
        "cursor" | "windsurf" | "codex" | "claude-code" => {
            json_config_has_entry(content, "mcpServers", name)
        }
        "copilot" | "gemini-cli" => Ok(ini_config_has_entry(content, name)),
        _ => Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    }
}

fn json_config_has_entry(content: &str, key: &str, name: &str) -> Result<bool> {
    let config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;
    Ok(config
        .get(key)
        .and_then(|v| v.as_object())
        .is_some_and(|map| map.contains_key(name)))
}

fn detect_entry_environment(client: &str, content: &str, name: &str) -> Result<Option<String>> {
    match client {
        "opencode" => detect_json_entry_environment(content, "mcp", name, "command"),
        "cursor" | "windsurf" | "codex" | "claude-code" => {
            detect_json_entry_environment(content, "mcpServers", name, "args")
        }
        "copilot" | "gemini-cli" => Ok(detect_ini_entry_environment(content, name)),
        _ => Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    }
}

fn detect_json_entry_environment(
    content: &str,
    top_level_key: &str,
    name: &str,
    command_key: &str,
) -> Result<Option<String>> {
    let config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;
    let command = config
        .get(top_level_key)
        .and_then(|v| v.get(name))
        .and_then(|v| v.get(command_key))
        .and_then(|v| v.as_array());

    Ok(command.and_then(|args| extract_environment_from_args(args)))
}

fn canonical_entry_name(repo_root: Option<&Path>, environment: &str) -> Option<String> {
    let project_name = repo_root
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())?;
    Some(format!("safeselect-{project_name}-{environment}"))
}

fn extract_environment_from_args(args: &[serde_json::Value]) -> Option<String> {
    args.windows(2).find_map(|window| {
        if window[0].as_str() == Some("--environment") {
            window[1].as_str().map(ToString::to_string)
        } else {
            None
        }
    })
}

fn ini_config_has_entry(content: &str, name: &str) -> bool {
    let section = format!("[mcpServers.{name}]");
    content.lines().any(|line| line.trim() == section)
}

fn detect_ini_entry_environment(content: &str, name: &str) -> Option<String> {
    let section = format!("[mcpServers.{name}]");
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            continue;
        }
        if !in_section || !trimmed.starts_with("args") {
            continue;
        }
        if let Some((_, rhs)) = trimmed.split_once('=') {
            let values: Vec<serde_json::Value> = serde_json::from_str(rhs.trim()).ok()?;
            return extract_environment_from_args(&values);
        }
    }
    None
}

fn remove_ini_entry(content: &str, name: &str) -> String {
    let section = format!("[mcpServers.{name}]");
    let mut output = Vec::new();
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section;
            if !in_section {
                output.push(line);
            }
            continue;
        }
        if !in_section {
            output.push(line);
        }
    }

    output.join("\n").trim_end().to_string()
}

fn remove_mcp_entry(content: &str, name: &str) -> Result<String> {
    if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(content) {
        for key in &["mcp", "mcpServers"] {
            if let Some(servers) = config.get_mut(*key).and_then(|v| v.as_object_mut()) {
                servers.remove(name);
            }
        }
        Ok(serde_json::to_string_pretty(&config)?)
    } else {
        Ok(remove_text_block(content, name))
    }
}

fn remove_text_block(content: &str, name: &str) -> String {
    content
        .lines()
        .filter(|line| !line.contains(name) && !line.contains("safeselect"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_environment_from_opencode_entry() {
        let content = r#"{
          "mcp": {
            "safeselect-demo-pre": {
              "type": "local",
              "command": ["safeselect", "serve", "--environment", "pre"]
            }
          }
        }"#;

        let environment = detect_entry_environment("opencode", content, "safeselect-demo-pre")
            .expect("should parse");

        assert_eq!(environment.as_deref(), Some("pre"));
    }

    #[test]
    fn detects_environment_from_ini_entry() {
        let content = r#"
[mcpServers.safeselect-demo-pre]
command = safeselect
args = ["serve", "--environment", "pre"]
"#;

        let environment = detect_entry_environment("copilot", content, "safeselect-demo-pre")
            .expect("should parse");

        assert_eq!(environment.as_deref(), Some("pre"));
    }

    #[test]
    fn replaces_ini_entry_in_place() {
        let content = r#"
[mcpServers.safeselect-demo-pre]
command = safeselect
args = ["serve", "--environment", "old"]

[other]
value = true
"#;

        let replaced =
            replace_ini_entry(content, "safeselect-demo-pre", "safeselect-demo-pre", "pre")
                .expect("should replace entry");

        assert!(replaced.contains("args = [\"serve\", \"--environment\", \"pre\"]"));
        assert!(replaced.contains("[other]"));
        assert!(!replaced.contains("\"old\""));
    }

    #[test]
    fn renames_json_entry_to_canonical_name() {
        let content = r#"{
          "mcp": {
            "legacy-pre": {
              "type": "local",
              "command": ["safeselect", "serve", "--environment", "pre"]
            }
          }
        }"#;
        let entry = serde_json::json!({
            "type": "local",
            "command": ["safeselect", "serve", "--environment", "pre"]
        });

        let replaced = replace_opencode_json(content, &entry, "legacy-pre", "safeselect-demo-pre")
            .expect("should rename entry");

        assert!(replaced.contains("safeselect-demo-pre"));
        assert!(!replaced.contains("legacy-pre"));
    }

    #[test]
    fn prefers_local_uninstall_target_when_entry_exists() {
        let temp =
            std::env::temp_dir().join(format!("safeselect-agent-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let repo = temp.join("repo");
        let child = repo.join("nested");
        let opencode = repo.join(".opencode");
        std::fs::create_dir_all(&opencode).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            opencode.join("opencode.jsonc"),
            r#"{
  "mcp": {
    "safeselect-demo-pre": {
      "type": "local",
      "command": ["safeselect", "serve", "--environment", "pre"]
    }
  }
}"#,
        )
        .unwrap();

        let resolved =
            resolve_uninstall_target("opencode", "safeselect-demo-pre", Some(&child)).unwrap();

        assert_eq!(resolved, opencode.join("opencode.jsonc"));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn falls_back_to_global_when_local_entry_missing() {
        let content = r#"{
  "mcp": {
    "safeselect-demo-pre": {
      "type": "local",
      "command": ["safeselect", "serve", "--environment", "pre"]
    }
  }
}"#;

        let global_has_entry =
            config_has_entry("opencode", content, "safeselect-demo-pre").unwrap();

        assert!(global_has_entry);
    }
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
