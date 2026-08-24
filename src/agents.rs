use crate::error::{Result, SafeselectError};
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{value, Array, DocumentMut, Item, Table};

type ClientDetector = fn() -> Option<PathBuf>;

pub fn detect_clients() -> Result<Vec<ClientConfig>> {
    let mut clients = vec![];

    let candidates: Vec<(&str, ClientDetector)> = vec![
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
        let detected = path.is_some() || command_exists(client_executable(name));
        clients.push(ClientConfig {
            name: name.to_string(),
            config_path: path.unwrap_or_else(|| canonical_global_config(name).unwrap_or_default()),
            detected,
        });
    }

    Ok(clients)
}

fn client_executable(client: &str) -> &str {
    match client {
        "claude-code" => "claude",
        "gemini-cli" => "gemini",
        other => other,
    }
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(command).is_file()))
}

pub struct ClientConfig {
    pub name: String,
    pub config_path: PathBuf,
    pub detected: bool,
}

pub fn status_lines(repo_root: Option<&Path>) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for client in [
        "opencode",
        "codex",
        "claude-code",
        "cursor",
        "windsurf",
        "copilot",
        "gemini-cli",
    ] {
        lines.extend(client_status_lines(client, repo_root)?);
    }
    Ok(lines)
}

fn client_status_lines(client: &str, repo_root: Option<&Path>) -> Result<Vec<String>> {
    let global_config = detect_existing_global_config(client);
    let detected = command_exists(client_executable(client)) || global_config.is_some();
    let mut configs = global_config
        .map(|path| vec![("user", path)])
        .unwrap_or_default();
    if let Some(path) = repo_root.and_then(|root| detect_local_client_config(client, root)) {
        configs.push(("project", path));
    }
    let mut lines = Vec::new();
    for (scope, path) in configs {
        lines.extend(config_status_lines(client, scope, &path)?);
    }
    if lines.is_empty() {
        lines.push(format!(
            "  {} {client} (no SafeSelect entries)",
            if detected { "·" } else { "✗" }
        ));
    }
    Ok(lines)
}

fn config_status_lines(client: &str, scope: &str, path: &Path) -> Result<Vec<String>> {
    verify_permissions(path)?;
    let content = std::fs::read_to_string(path)?;
    Ok(safeselect_entries(client, &content)?
        .into_iter()
        .map(|(name, environment)| {
            format!(
                "  ✓ {client}: {name} [scope={scope}, environment={}, config={}]",
                environment.as_deref().unwrap_or("unknown"),
                path.display()
            )
        })
        .collect())
}

fn detect_existing_global_config(client: &str) -> Option<PathBuf> {
    if client == "opencode" {
        return detect_opencode_config();
    }
    if client == "codex" {
        return detect_codex_config();
    }
    detect_secondary_global_config(client)
}

fn detect_secondary_global_config(client: &str) -> Option<PathBuf> {
    match client {
        "claude-code" => detect_claude_code_config(),
        "cursor" => detect_cursor_config(),
        "windsurf" => detect_windsurf_config(),
        "copilot" => detect_copilot_config(),
        "gemini-cli" => detect_gemini_config(),
        _ => None,
    }
}

fn safeselect_entries(client: &str, content: &str) -> Result<Vec<(String, Option<String>)>> {
    let mut entries = Vec::new();
    for name in all_entry_names(client, content)? {
        if let Some(entry) = safeselect_entry(client, content, name)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn safeselect_entry(
    client: &str,
    content: &str,
    name: String,
) -> Result<Option<(String, Option<String>)>> {
    if !entry_uses_safeselect(client, content, &name)? {
        return Ok(None);
    }
    let environment = detect_entry_environment(client, content, &name)?;
    Ok(Some((name, environment)))
}

fn entry_uses_safeselect(client: &str, content: &str, name: &str) -> Result<bool> {
    if client == "codex" {
        let document = content.parse::<DocumentMut>().map_err(|error| {
            SafeselectError::Other(format!("Cannot parse Codex TOML config: {error}"))
        })?;
        return Ok(document["mcp_servers"][name]["command"].as_str() == Some("safeselect"));
    }
    let (key, command_key) = json_command_contract(client_format(client)?);
    let command = parse_json_or_jsonc(content)
        .map_err(|error| SafeselectError::Other(format!("Cannot parse JSON config: {error}")))?
        .get(key)
        .and_then(|value| value.get(name))
        .and_then(|value| value.get(command_key))
        .cloned();
    Ok(command_runs_safeselect(command))
}

fn json_command_contract(format: ConfigFormat) -> (&'static str, &'static str) {
    match format {
        ConfigFormat::OpenCode => ("mcp", "command"),
        ConfigFormat::McpServers | ConfigFormat::Claude => ("mcpServers", "command"),
        ConfigFormat::Copilot => ("servers", "command"),
        ConfigFormat::Codex => unreachable!(),
    }
}

fn command_runs_safeselect(command: Option<serde_json::Value>) -> bool {
    match command {
        Some(serde_json::Value::String(value)) => value == "safeselect",
        Some(serde_json::Value::Array(values)) => {
            values.first().and_then(serde_json::Value::as_str) == Some("safeselect")
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigFormat {
    OpenCode,
    McpServers,
    Copilot,
    Codex,
    Claude,
}

fn client_format(client: &str) -> Result<ConfigFormat> {
    match client {
        "opencode" => Ok(ConfigFormat::OpenCode),
        "cursor" | "windsurf" | "gemini-cli" => Ok(ConfigFormat::McpServers),
        "copilot" => Ok(ConfigFormat::Copilot),
        "codex" => Ok(ConfigFormat::Codex),
        "claude-code" => Ok(ConfigFormat::Claude),
        _ => Err(SafeselectError::Other(format!("Unknown client: {client}"))),
    }
}

fn serve_args(environment: &str, repo_root: Option<&Path>) -> Result<Vec<String>> {
    let root = repo_root.ok_or_else(|| {
        SafeselectError::Other(
            "no project root specified; use --project or run from a project directory".into(),
        )
    })?;
    let root = root.canonicalize().map_err(|error| {
        SafeselectError::Other(format!(
            "Cannot resolve project path {}: {error}",
            root.display()
        ))
    })?;
    Ok(vec![
        "serve".into(),
        "--project".into(),
        root.to_string_lossy().into_owned(),
        "--environment".into(),
        environment.into(),
    ])
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
    if client == "claude-code" {
        return install_claude_entry(environment, entry_name, repo_root, config_dir, local);
    }
    install_file_entry(
        client,
        environment,
        entry_name,
        repo_root,
        config_dir,
        mcp_timeout_ms,
        local,
    )
}

#[allow(clippy::too_many_arguments)]
fn install_file_entry(
    client: &str,
    environment: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    mcp_timeout_ms: u64,
    local: bool,
) -> Result<()> {
    let config_path = select_install_config(client, repo_root, local)?;
    let (config_existed, content) = read_or_initialize_config(client, &config_path)?;

    let new_content = build_entry_content(
        client,
        &content,
        entry_name,
        environment,
        repo_root,
        config_dir,
        mcp_timeout_ms,
        false,
        None,
    )?;

    if new_content == content {
        println!("Entry '{entry_name}' is already up to date for {client}");
        println!("Next: run `safeselect agent status` to verify it.");
        return Ok(());
    }

    backup_existing_config(&config_path, config_existed)?;
    print_install_diff(client, &config_path, &content, &new_content, config_existed);

    write_config_and_verify(&config_path, &content, &new_content, config_existed)?;

    println!("Entry '{entry_name}' installed for {client}");
    println!("Next: run `safeselect agent status` to verify it.");
    Ok(())
}

fn read_or_initialize_config(client: &str, path: &Path) -> Result<(bool, String)> {
    if path.exists() {
        verify_permissions(path)?;
        return Ok((true, std::fs::read_to_string(path)?));
    }
    prepare_config_parent(path)?;
    Ok((false, initial_config_content(client)?.to_string()))
}

fn backup_existing_config(path: &Path, existed: bool) -> Result<()> {
    if existed {
        std::fs::copy(path, path.with_extension("safeselect.bak"))?;
    }
    Ok(())
}

fn print_install_diff(client: &str, path: &Path, old: &str, new: &str, existed: bool) {
    println!("--- Config diff for {client} ({}) ---", path.display());
    show_diff(old, new);
    if existed {
        println!(
            "\nBackup saved to: {}",
            path.with_extension("safeselect.bak").display()
        );
    }
}

fn opencode_jsonc_alternative(client: &str, local_path: &Path) -> Option<PathBuf> {
    if client != "opencode" || local_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return None;
    }

    let jsonc_path = local_path.with_extension("jsonc");
    (!jsonc_path.exists()).then_some(jsonc_path)
}

fn select_install_config(client: &str, repo_root: Option<&Path>, local: bool) -> Result<PathBuf> {
    if local {
        return get_local_client_config(client, repo_root);
    }
    get_client_config(client)
}

#[allow(clippy::too_many_arguments)]
fn build_entry_content(
    client: &str,
    content: &str,
    entry_name: &str,
    environment: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    mcp_timeout_ms: u64,
    upgrade: bool,
    old_entry_name: Option<&str>,
) -> Result<String> {
    let args = serve_args(environment, repo_root)?;
    let entry = serde_json::json!({
        "command": "safeselect",
        "args": args,
    });
    let copilot_entry = serde_json::json!({
        "type": "stdio",
        "command": "safeselect",
        "args": serve_args(environment, repo_root)?,
    });
    let mut opencode_entry = serde_json::json!({
        "type": "local",
        "command": std::iter::once("safeselect".to_string())
            .chain(serve_args(environment, repo_root)?).collect::<Vec<_>>(),
        "timeout": mcp_timeout_ms,
        "enabled": true
    });
    if let Some(dir) = config_dir {
        opencode_entry["environment"] = serde_json::json!({
            "SAFESELECT_CONFIG_DIR": dir.to_string_lossy().to_string()
        });
    }

    let format = client_format(client)?;
    if format == ConfigFormat::Codex {
        return upsert_codex_toml(
            content,
            old_entry_name.filter(|_| upgrade),
            entry_name,
            &serve_args(environment, repo_root)?,
            config_dir,
            mcp_timeout_ms,
        );
    }
    if format == ConfigFormat::Claude {
        return Err(SafeselectError::Other(
            "Claude Code configuration must be changed through `claude mcp`".into(),
        ));
    }

    if upgrade {
        let old_entry_name = old_entry_name.expect("upgrade requires the previous entry name");
        match client {
            "opencode" => {
                replace_opencode_json(content, &opencode_entry, old_entry_name, entry_name)
            }
            "cursor" | "windsurf" | "gemini-cli" => {
                replace_mcp_json(content, &entry, old_entry_name, entry_name)
            }
            "copilot" => replace_json_entry(
                content,
                "servers",
                &copilot_entry,
                old_entry_name,
                entry_name,
            ),
            _ => Err(SafeselectError::Other(format!("Unknown client: {client}"))),
        }
    } else {
        match client {
            "opencode" => append_opencode_json(content, &opencode_entry, entry_name),
            "cursor" | "windsurf" | "gemini-cli" => append_mcp_json(content, &entry, entry_name),
            "copilot" => append_json_entry(content, "servers", &copilot_entry, entry_name),
            _ => Err(SafeselectError::Other(format!("Unknown client: {client}"))),
        }
    }
}

fn write_config_and_verify(
    path: &Path,
    original: &str,
    updated: &str,
    original_existed: bool,
) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        SafeselectError::Other(format!("Config path has no parent: {}", path.display()))
    })?;
    let temp = parent.join(format!(".safeselect-{}.tmp", uuid::Uuid::new_v4()));
    write_private_file(&temp, updated)?;
    std::fs::rename(&temp, path)?;
    if std::fs::read_to_string(path)? != updated {
        if original_existed {
            write_private_file(path, original)?;
        } else {
            let _ = std::fs::remove_file(path);
        }
        return Err(SafeselectError::Other(
            "Write verification failed, rolled back".into(),
        ));
    }
    Ok(())
}

fn write_private_file(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn prepare_config_parent(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        SafeselectError::Other(format!("Config path has no parent: {}", path.display()))
    })?;
    verify_config_parent(parent)?;
    std::fs::create_dir_all(parent)?;
    Ok(())
}

fn initial_config_content(client: &str) -> Result<&'static str> {
    Ok(match client_format(client)? {
        ConfigFormat::OpenCode => "{\n  \"mcp\": {}\n}\n",
        ConfigFormat::McpServers | ConfigFormat::Claude => "{\n  \"mcpServers\": {}\n}\n",
        ConfigFormat::Copilot => "{\n  \"servers\": {}\n}\n",
        ConfigFormat::Codex => "",
    })
}

fn verify_config_parent(parent: &Path) -> Result<()> {
    if !parent.exists() {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink() {
        return Err(SafeselectError::Other(format!(
            "Config directory is a symlink: {}",
            parent.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(SafeselectError::Other(format!(
                "Config directory has unsafe permissions: {}",
                parent.display()
            )));
        }
    }
    Ok(())
}

fn create_opencode_config(path: &Path) -> Result<()> {
    let default_config = serde_json::json!({
        "mcp": {}
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_private_file(path, &serde_json::to_string_pretty(&default_config)?)?;
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
    if client == "claude-code" {
        return upgrade_claude_entry(entry_name, environment, repo_root, config_dir, local);
    }
    upgrade_file_entry(
        client,
        entry_name,
        environment,
        repo_root,
        config_dir,
        mcp_timeout_ms,
        local,
    )
}

fn upgrade_claude_entry(
    entry_name: Option<&str>,
    environment: Option<&str>,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    local: bool,
) -> Result<()> {
    let name = entry_name.ok_or_else(|| {
        SafeselectError::Other(
            "Claude Code upgrades require --name so SafeSelect never changes an ambiguous entry"
                .into(),
        )
    })?;
    let environment = environment.ok_or_else(|| {
        SafeselectError::Other("Claude Code upgrades require --environment".into())
    })?;
    install_claude_entry(environment, name, repo_root, config_dir, local)
}

#[allow(clippy::too_many_arguments)]
fn upgrade_file_entry(
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

    let environment =
        resolve_upgrade_environment(client, environment, &content, &resolved_entry_name)?;
    let target_entry_name =
        resolve_upgrade_entry_name(repo_root, &environment, &resolved_entry_name);

    write_upgrade_config(
        client,
        &config_path,
        &content,
        &resolved_entry_name,
        &target_entry_name,
        &environment,
        repo_root,
        config_dir,
        mcp_timeout_ms,
    )?;

    print_upgrade_result(client, &resolved_entry_name, &target_entry_name);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_upgrade_config(
    client: &str,
    config_path: &Path,
    content: &str,
    resolved_entry_name: &str,
    target_entry_name: &str,
    environment: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    mcp_timeout_ms: u64,
) -> Result<()> {
    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(config_path, &backup_path)?;

    let new_content = build_entry_content(
        client,
        content,
        target_entry_name,
        environment,
        repo_root,
        config_dir,
        mcp_timeout_ms,
        true,
        Some(resolved_entry_name),
    )?;

    println!(
        "--- Config diff for {client} ({}) ---",
        config_path.display()
    );
    show_diff(content, &new_content);
    println!("\nBackup saved to: {}", backup_path.display());

    write_config_and_verify(config_path, content, &new_content, true)?;
    Ok(())
}

fn resolve_upgrade_environment(
    client: &str,
    environment: Option<&str>,
    content: &str,
    entry_name: &str,
) -> Result<String> {
    match environment {
        Some(value) => Ok(value.to_owned()),
        None => detect_entry_environment(client, content, entry_name)?.ok_or_else(|| {
            SafeselectError::Other(format!(
                "Cannot detect environment for entry '{entry_name}'; use --environment"
            ))
        }),
    }
}

fn print_upgrade_result(client: &str, resolved_entry_name: &str, target_entry_name: &str) {
    if target_entry_name == resolved_entry_name {
        println!("Entry '{resolved_entry_name}' upgraded for {client}");
    } else {
        println!(
            "Entry '{resolved_entry_name}' upgraded and renamed to '{target_entry_name}' for {client}"
        );
    }
}

fn resolve_upgrade_entry_name(
    repo_root: Option<&Path>,
    environment: &str,
    resolved_entry_name: &str,
) -> String {
    canonical_entry_name(repo_root, environment).unwrap_or_else(|| resolved_entry_name.to_string())
}

pub fn uninstall_entry(client: &str, entry_name: &str, repo_root: Option<&Path>) -> Result<()> {
    if client == "claude-code" {
        return uninstall_claude_entry(entry_name, repo_root);
    }
    uninstall_file_entry(client, entry_name, repo_root)
}

fn uninstall_claude_entry(entry_name: &str, repo_root: Option<&Path>) -> Result<()> {
    let local = repo_root
        .map(|root| root.join(".mcp.json").exists())
        .unwrap_or(false);
    run_claude_remove(entry_name, repo_root, local)
}

fn uninstall_file_entry(client: &str, entry_name: &str, repo_root: Option<&Path>) -> Result<()> {
    let config_path = resolve_uninstall_target(client, entry_name, repo_root)?;
    let content = std::fs::read_to_string(&config_path)?;
    validate_removal_target(client, &content, entry_name)?;

    let backup_path = config_path.with_extension("safeselect.bak");
    std::fs::copy(&config_path, &backup_path)?;

    verify_permissions(&config_path)?;
    let new_content = remove_client_entry(client, &content, entry_name)?;
    write_config_and_verify(&config_path, &content, &new_content, true)?;

    println!("Entry '{entry_name}' uninstalled from {client}");
    Ok(())
}

fn validate_removal_target(client: &str, content: &str, entry_name: &str) -> Result<()> {
    let exists = config_has_entry(client, content, entry_name)?;
    let is_safeselect = exists && entry_uses_safeselect(client, content, entry_name)?;
    if is_safeselect {
        return Ok(());
    }
    Err(SafeselectError::Other(format!(
        "No safeselect entry found in {client} config"
    )))
}

fn install_claude_entry(
    environment: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    local: bool,
) -> Result<()> {
    if !command_exists("claude") {
        return Err(SafeselectError::Other(
            "Claude Code CLI not found; install `claude` before configuring its MCP servers".into(),
        ));
    }
    install_claude_entry_with_program(
        Path::new("claude"),
        environment,
        entry_name,
        repo_root,
        config_dir,
        local,
    )
}

fn install_claude_entry_with_program(
    program: &Path,
    environment: &str,
    entry_name: &str,
    repo_root: Option<&Path>,
    config_dir: Option<&Path>,
    local: bool,
) -> Result<()> {
    let scope = if local { "project" } else { "user" };
    let args = serve_args(environment, repo_root)?;
    let mut entry = serde_json::json!({"command":"safeselect", "args": args});
    if let Some(dir) = config_dir {
        entry["env"] = serde_json::json!({
            "SAFESELECT_CONFIG_DIR": dir.to_string_lossy().to_string()
        });
    }
    let mut command = Command::new(program);
    command.args([
        "mcp",
        "add-json",
        entry_name,
        &entry.to_string(),
        "--scope",
        scope,
    ]);
    if let Some(root) = repo_root {
        command.current_dir(root);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(SafeselectError::Other(format!(
            "Claude Code rejected the MCP configuration: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    println!("Entry '{entry_name}' installed for claude-code ({scope} scope)");
    println!("Next: run `safeselect agent status` to verify it.");
    Ok(())
}

fn run_claude_remove(entry_name: &str, repo_root: Option<&Path>, local: bool) -> Result<()> {
    if !command_exists("claude") {
        return Err(SafeselectError::Other("Claude Code CLI not found".into()));
    }
    run_claude_remove_with_program(Path::new("claude"), entry_name, repo_root, local)
}

fn run_claude_remove_with_program(
    program: &Path,
    entry_name: &str,
    repo_root: Option<&Path>,
    local: bool,
) -> Result<()> {
    let scope = if local { "project" } else { "user" };
    let mut command = Command::new(program);
    command.args(["mcp", "remove", entry_name, "--scope", scope]);
    if let Some(root) = repo_root {
        command.current_dir(root);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(SafeselectError::Other(format!(
            "Claude Code could not remove the MCP entry: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    println!("Entry '{entry_name}' uninstalled from claude-code");
    println!("Next: run `safeselect agent status` to verify removal, then stop.");
    Ok(())
}

pub fn detect_uninstall_target(
    client: &str,
    repo_root: Option<&Path>,
) -> Result<(PathBuf, String)> {
    resolve_upgrade_target(client, None, None, repo_root, false)
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
    client_format(client)?;
    detect_existing_global_config(client)
        .or_else(|| canonical_global_config(client))
        .ok_or_else(|| {
            SafeselectError::Other(format!(
                "Cannot determine the global config path for {client}"
            ))
        })
}

fn canonical_global_config(client: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    if client == "codex" {
        return Some(
            std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
        );
    }
    canonical_home_config(client, &home)
}

fn canonical_home_config(client: &str, home: &Path) -> Option<PathBuf> {
    match client {
        "opencode" => Some(dirs::config_dir()?.join("opencode").join("opencode.jsonc")),
        "cursor" => Some(home.join(".cursor").join("mcp.json")),
        "windsurf" => Some(
            home.join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
        ),
        "claude-code" => Some(home.join(".claude.json")),
        "copilot" => Some(home.join(".copilot").join("mcp-config.json")),
        "gemini-cli" => Some(home.join(".gemini").join("settings.json")),
        _ => None,
    }
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
            let config = repo_root.join(".cursor").join("mcp.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "claude-code" => {
            let config = repo_root.join(".mcp.json");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "codex" => {
            let config = repo_root.join(".codex").join("config.toml");
            if config.exists() {
                Some(config)
            } else {
                None
            }
        }
        "copilot" => existing(repo_root.join(".vscode").join("mcp.json")),
        "gemini-cli" => existing(repo_root.join(".gemini").join("settings.json")),
        _ => None,
    }
}

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
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
                opencode_dir.join("opencode.jsonc")
            }
        }
        "cursor" => root.join(".cursor").join("mcp.json"),
        "claude-code" => root.join(".mcp.json"),
        "codex" => root.join(".codex").join("config.toml"),
        "copilot" => root.join(".vscode").join("mcp.json"),
        "gemini-cli" => root.join(".gemini").join("settings.json"),
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
    existing(home.join(".copilot").join("mcp-config.json"))
}

fn detect_cursor_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    existing(home.join(".cursor").join("mcp.json"))
}

fn detect_windsurf_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    existing(
        home.join(".codeium")
            .join("windsurf")
            .join("mcp_config.json"),
    )
}

fn detect_claude_code_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    existing(home.join(".claude.json"))
}

fn detect_codex_config() -> Option<PathBuf> {
    let base = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    existing(base.join("config.toml"))
}

fn detect_gemini_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    existing(home.join(".gemini").join("settings.json"))
}

fn verify_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)?;
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
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match (ch, chars.peek().copied()) {
            ('"', _) => copy_json_string(&mut chars, &mut out),
            ('/', Some('/')) => skip_line_comment(&mut chars),
            ('/', Some('*')) => skip_block_comment(&mut chars),
            _ => out.push(ch),
        }
    }
    out
}

fn copy_json_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, out: &mut String) {
    out.push('"');
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
        } else if ch == '"' {
            break;
        }
    }
}

fn skip_line_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    for ch in chars.by_ref() {
        if ch == '\n' {
            break;
        }
    }
}

fn skip_block_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    chars.next();
    let mut previous = ' ';
    for ch in chars.by_ref() {
        if previous == '*' && ch == '/' {
            break;
        }
        previous = ch;
    }
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
    append_json_entry(content, "mcpServers", entry, name)
}

fn append_json_entry(
    content: &str,
    top_level_key: &str,
    entry: &serde_json::Value,
    name: &str,
) -> Result<String> {
    let mut config: serde_json::Value = parse_json_or_jsonc(content)
        .map_err(|e| SafeselectError::Other(format!("Cannot parse JSON config: {e}")))?;

    let servers = config
        .get_mut(top_level_key)
        .and_then(|v| v.as_object_mut());

    match servers {
        Some(map) => {
            map.insert(name.to_string(), entry.clone());
        }
        None => {
            let mut map = serde_json::Map::new();
            map.insert(name.to_string(), entry.clone());
            config[top_level_key] = serde_json::Value::Object(map);
        }
    }

    Ok(serde_json::to_string_pretty(&config)?)
}

fn upsert_codex_toml(
    content: &str,
    old_name: Option<&str>,
    name: &str,
    args: &[String],
    config_dir: Option<&Path>,
    timeout_ms: u64,
) -> Result<String> {
    let mut document = content.parse::<DocumentMut>().map_err(|error| {
        SafeselectError::Other(format!("Cannot parse Codex TOML config: {error}"))
    })?;
    if !document.as_table().contains_key("mcp_servers") {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| SafeselectError::Other("'mcp_servers' must be a TOML table".into()))?;
    if let Some(old) = old_name {
        if !servers.contains_key(old) {
            return Err(SafeselectError::Other(format!(
                "No SafeSelect entry named '{old}' found in codex config"
            )));
        }
        servers.remove(old);
    }
    let mut table = Table::new();
    table["command"] = value("safeselect");
    let mut toml_args = Array::new();
    for arg in args {
        toml_args.push(arg.as_str());
    }
    table["args"] = value(toml_args);
    table["tool_timeout_sec"] = value(timeout_ms.div_ceil(1000) as i64);
    if let Some(dir) = config_dir {
        let mut env = Table::new();
        env["SAFESELECT_CONFIG_DIR"] = value(dir.to_string_lossy().as_ref());
        table["env"] = Item::Table(env);
    }
    servers[name] = Item::Table(table);
    Ok(document.to_string())
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
    // A project-local config is a valid standalone target. Do not require the
    // client to be installed globally when collecting upgrade candidates.
    let global = get_client_config(client)?;
    if global.exists() {
        paths.push(global);
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(SafeselectError::Other(format!(
            "No {client} configuration exists; install an entry before upgrading"
        )));
    }
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
    let all_names = all_entry_names(client, content)?;

    Ok(all_names
        .into_iter()
        .filter(|name| {
            candidate_name_matches(
                name,
                project_name,
                environment,
                &canonical_prefix,
                &legacy_prefix,
            )
        })
        .collect())
}

fn all_entry_names(client: &str, content: &str) -> Result<Vec<String>> {
    let names = match client_format(client)? {
        ConfigFormat::OpenCode => json_entry_names(content, "mcp")?,
        ConfigFormat::McpServers | ConfigFormat::Claude => json_entry_names(content, "mcpServers")?,
        ConfigFormat::Copilot => json_entry_names(content, "servers")?,
        ConfigFormat::Codex => toml_entry_names(content)?,
    };
    Ok(names)
}

fn candidate_name_matches(
    name: &str,
    project_name: &str,
    environment: Option<&str>,
    canonical_prefix: &str,
    legacy_prefix: &str,
) -> bool {
    if let Some(env) = environment {
        let canonical = format!("safeselect-{project_name}-{env}");
        let legacy = format!("{project_name}-{env}");
        name == canonical || name == legacy
    } else {
        name.starts_with(canonical_prefix) || name.starts_with(legacy_prefix)
    }
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

fn toml_entry_names(content: &str) -> Result<Vec<String>> {
    let document = content.parse::<DocumentMut>().map_err(|error| {
        SafeselectError::Other(format!("Cannot parse Codex TOML config: {error}"))
    })?;
    Ok(document["mcp_servers"]
        .as_table()
        .map(|servers| servers.iter().map(|(name, _)| name.to_string()).collect())
        .unwrap_or_default())
}

fn detect_toml_entry_environment(content: &str, name: &str) -> Result<Option<String>> {
    let document = content.parse::<DocumentMut>().map_err(|error| {
        SafeselectError::Other(format!("Cannot parse Codex TOML config: {error}"))
    })?;
    let args = document["mcp_servers"][name]["args"]
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str().map(|value| serde_json::json!(value)))
                .collect::<Vec<_>>()
        });
    Ok(args.as_deref().and_then(extract_environment_from_args))
}

fn config_has_entry(client: &str, content: &str, name: &str) -> Result<bool> {
    match client {
        "opencode" => json_config_has_entry(content, "mcp", name),
        "cursor" | "windsurf" | "claude-code" | "gemini-cli" => {
            json_config_has_entry(content, "mcpServers", name)
        }
        "copilot" => json_config_has_entry(content, "servers", name),
        "codex" => Ok(toml_entry_names(content)?.iter().any(|entry| entry == name)),
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
        "cursor" | "windsurf" | "claude-code" | "gemini-cli" => {
            detect_json_entry_environment(content, "mcpServers", name, "args")
        }
        "copilot" => detect_json_entry_environment(content, "servers", name, "args"),
        "codex" => detect_toml_entry_environment(content, name),
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

fn remove_client_entry(client: &str, content: &str, name: &str) -> Result<String> {
    match client_format(client)? {
        ConfigFormat::OpenCode => remove_json_key(content, "mcp", name),
        ConfigFormat::McpServers | ConfigFormat::Claude => {
            remove_json_key(content, "mcpServers", name)
        }
        ConfigFormat::Copilot => remove_json_key(content, "servers", name),
        ConfigFormat::Codex => remove_codex_entry(content, name),
    }
}

fn remove_codex_entry(content: &str, name: &str) -> Result<String> {
    let mut document = content.parse::<DocumentMut>().map_err(|error| {
        SafeselectError::Other(format!("Cannot parse Codex TOML config: {error}"))
    })?;
    let removed = document["mcp_servers"]
        .as_table_mut()
        .is_some_and(|servers| servers.remove(name).is_some());
    if !removed {
        return Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{name}' found in codex config"
        )));
    }
    Ok(document.to_string())
}

fn remove_json_key(content: &str, key: &str, name: &str) -> Result<String> {
    let mut config = parse_json_or_jsonc(content)
        .map_err(|error| SafeselectError::Other(format!("Cannot parse JSON config: {error}")))?;
    let removed = config
        .get_mut(key)
        .and_then(serde_json::Value::as_object_mut)
        .is_some_and(|entries| entries.remove(name).is_some());
    if !removed {
        return Err(SafeselectError::Other(format!(
            "No SafeSelect entry named '{name}' found in client config"
        )));
    }
    Ok(serde_json::to_string_pretty(&config)?)
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
    fn detects_environment_from_copilot_entry() {
        let content = r#"{"servers":{"safeselect-demo-pre":{"type":"stdio","command":"safeselect","args":["serve","--project","/tmp/demo","--environment","pre"]}}}"#;

        let environment = detect_entry_environment("copilot", content, "safeselect-demo-pre")
            .expect("should parse");

        assert_eq!(environment.as_deref(), Some("pre"));
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
    fn offers_jsonc_alternative_for_existing_opencode_json() {
        let temp = std::env::temp_dir().join(format!(
            "safeselect-agent-jsonc-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        let json_path = temp.join("opencode.json");
        std::fs::write(&json_path, "{}").unwrap();

        let alternative = opencode_jsonc_alternative("opencode", &json_path);

        assert_eq!(alternative, Some(temp.join("opencode.jsonc")));
        assert!(opencode_jsonc_alternative("cursor", &json_path).is_none());

        std::fs::write(temp.join("opencode.jsonc"), "{}").unwrap();
        assert!(opencode_jsonc_alternative("opencode", &json_path).is_none());
        let _ = std::fs::remove_dir_all(&temp);
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

    #[test]
    fn strips_jsonc_comments_without_touching_strings() {
        let input = r#"{
  // single-line comment
  "url": "https://example.test/*not a comment*/",
  "escaped": "quote: \" and slash: \\", /* block comment */
  "value": 1
}"#;

        let cleaned = strip_jsonc_comments(input);

        assert!(!cleaned.contains("single-line comment"));
        assert!(!cleaned.contains("block comment"));
        assert!(cleaned.contains("https://example.test/*not a comment*/"));
        assert!(cleaned.contains("quote:"));
        assert!(cleaned.contains("slash:"));
        assert!(cleaned.contains("\"value\": 1"));
    }

    #[test]
    fn detects_unknown_uninstall_client() {
        let result = detect_uninstall_target("unknown-client", None);

        assert!(result.is_err());
    }

    #[test]
    fn checks_entries_for_json_and_ini_clients() {
        let json = r#"{"mcp": {"alpha": {}}, "mcpServers": {"beta": {}}}"#;
        let copilot = r#"{"servers":{"gamma":{"command":"safeselect"}}}"#;

        assert!(config_has_entry("opencode", json, "alpha").unwrap());
        assert!(config_has_entry("cursor", json, "beta").unwrap());
        assert!(config_has_entry("copilot", copilot, "gamma").unwrap());
        assert!(config_has_entry("gemini-cli", json, "beta").unwrap());
        assert!(config_has_entry("unknown-client", json, "alpha").is_err());
    }

    #[test]
    fn creates_default_opencode_config() {
        let path =
            std::env::temp_dir().join(format!("safeselect-opencode-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        create_opencode_config(&path).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"mcp": {}}));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn builds_official_json_client_contracts_with_absolute_project() {
        let root =
            std::env::temp_dir().join(format!("safeselect-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cursor = build_entry_content(
            "cursor",
            "{}",
            "safe",
            "dev",
            Some(&root),
            None,
            90_000,
            false,
            None,
        )
        .unwrap();
        let cursor: serde_json::Value = serde_json::from_str(&cursor).unwrap();
        assert_eq!(cursor["mcpServers"]["safe"]["command"], "safeselect");
        assert_eq!(cursor["mcpServers"]["safe"]["args"][1], "--project");

        let copilot = build_entry_content(
            "copilot",
            "{}",
            "safe",
            "dev",
            Some(&root),
            None,
            90_000,
            false,
            None,
        )
        .unwrap();
        let copilot: serde_json::Value = serde_json::from_str(&copilot).unwrap();
        assert_eq!(copilot["servers"]["safe"]["type"], "stdio");
        assert_eq!(copilot["servers"]["safe"]["command"], "safeselect");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn manages_fresh_project_scoped_entries_end_to_end() {
        let root =
            std::env::temp_dir().join(format!("safeselect-agent-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".safeselect")).unwrap();
        let entry = format!(
            "safeselect-{}-dev",
            root.file_name().unwrap().to_string_lossy()
        );

        for client in ["opencode", "cursor", "copilot", "gemini-cli", "codex"] {
            install_entry(
                client,
                "dev",
                &entry,
                Some(&root),
                Some(&root),
                90_001,
                true,
            )
            .unwrap();
            install_entry(
                client,
                "dev",
                &entry,
                Some(&root),
                Some(&root),
                90_001,
                true,
            )
            .unwrap();
            upgrade_entry(
                client,
                Some(&entry),
                Some("dev"),
                Some(&root),
                Some(&root),
                90_001,
                true,
            )
            .unwrap();
        }

        let status = status_lines(Some(&root)).unwrap().join("\n");
        for client in ["opencode", "cursor", "copilot", "gemini-cli", "codex"] {
            assert!(status.contains(&format!("✓ {client}: {entry}")));
            uninstall_entry(client, &entry, Some(&root)).unwrap();
        }
        assert!(!status_lines(Some(&root))
            .unwrap()
            .join("\n")
            .contains(&entry));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn edits_codex_toml_without_losing_unrelated_comments() {
        let root = std::env::temp_dir().join(format!("safeselect-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let content = "# keep this comment\nmodel = \"gpt-5\"\n";
        let updated = build_entry_content(
            "codex",
            content,
            "safe",
            "dev",
            Some(&root),
            None,
            90_001,
            false,
            None,
        )
        .unwrap();
        assert!(updated.contains("# keep this comment"));
        assert!(updated.contains("[mcp_servers.safe]"));
        assert!(updated.contains("tool_timeout_sec = 91"));
        assert_eq!(
            detect_entry_environment("codex", &updated, "safe")
                .unwrap()
                .as_deref(),
            Some("dev")
        );
        assert!(upsert_codex_toml(
            &updated,
            Some("missing"),
            "safe",
            &["serve".into()],
            None,
            1_000,
        )
        .is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_project_scope_for_windsurf() {
        let root =
            std::env::temp_dir().join(format!("safeselect-windsurf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let error = get_local_client_config("windsurf", Some(&root)).unwrap_err();
        assert!(error.to_string().contains("without --local"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_agent_configs() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("safeselect-symlink-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target.json");
        let link = root.join("config.json");
        std::fs::write(&target, "{}").unwrap();
        symlink(&target, &link).unwrap();
        assert!(verify_permissions(&link)
            .unwrap_err()
            .to_string()
            .contains("symlink"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn validates_agent_config_directories() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root =
            std::env::temp_dir().join(format!("safeselect-config-parent-{}", uuid::Uuid::new_v4()));
        let safe = root.join("safe");
        std::fs::create_dir_all(&safe).unwrap();
        verify_config_parent(&safe).unwrap();

        let link = root.join("link");
        symlink(&safe, &link).unwrap();
        assert!(verify_config_parent(&link).is_err());

        std::fs::set_permissions(&safe, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(verify_config_parent(&safe).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn configures_claude_through_its_native_cli_contract() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("safeselect-claude-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("claude");
        std::fs::write(
            &program,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$PWD/claude.args\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(upgrade_claude_entry(None, Some("dev"), Some(&root), None, true).is_err());
        assert!(upgrade_claude_entry(Some("safe"), None, Some(&root), None, true).is_err());
        assert!(upgrade_claude_entry(Some("safe"), Some("dev"), Some(&root), None, true,).is_err());

        install_claude_entry_with_program(&program, "dev", "safe", Some(&root), Some(&root), true)
            .unwrap();
        let args = std::fs::read_to_string(root.join("claude.args")).unwrap();
        assert!(args.contains("add-json\nsafe\n"));
        assert!(args.contains("--scope\nproject"));
        assert!(args.contains("--project"));

        run_claude_remove_with_program(&program, "safe", Some(&root), true).unwrap();
        let args = std::fs::read_to_string(root.join("claude.args")).unwrap();
        assert!(args.contains("remove\nsafe\n--scope\nproject"));

        std::fs::write(&program, "#!/bin/sh\necho rejected >&2\nexit 1\n").unwrap();
        assert!(install_claude_entry_with_program(
            &program,
            "dev",
            "safe",
            Some(&root),
            None,
            false,
        )
        .unwrap_err()
        .to_string()
        .contains("rejected"));
        assert!(run_claude_remove_with_program(&program, "safe", Some(&root), false).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replaces_existing_mcp_json_entry() {
        let updated = replace_mcp_json(
            r#"{"mcpServers":{"old":{"command":"old"}}}"#,
            &serde_json::json!({"command": "new"}),
            "old",
            "new",
        )
        .unwrap();
        assert!(updated.contains("\"new\""));
        assert!(!updated.contains("\"old\""));
    }

    #[test]
    fn appends_entries_to_json_configs() {
        let entry = serde_json::json!({"command": "safeselect"});
        let opencode = append_opencode_json("{}", &entry, "safe").unwrap();
        assert!(opencode.contains("\"safe\""));
        let mcp = append_mcp_json("{}", &entry, "safe").unwrap();
        assert!(mcp.contains("\"safe\""));
    }

    #[test]
    fn rejects_unsafe_agent_config_permissions() {
        let path =
            std::env::temp_dir().join(format!("safeselect-permissions-{}", std::process::id()));
        std::fs::write(&path, "{}").unwrap();
        verify_permissions(&path).unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn renders_agent_diff_changes() {
        show_diff("old\n", "new\n");
    }

    #[test]
    fn selects_candidate_entries_for_supported_clients_and_environments() {
        let json = r#"{"mcpServers":{"safeselect-demo-pre":{},"demo-dev":{},"other":{}}}"#;
        assert_eq!(
            candidate_entry_names("cursor", json, "demo", Some("pre")).unwrap(),
            vec!["safeselect-demo-pre"]
        );
        assert_eq!(
            candidate_entry_names("cursor", json, "demo", None)
                .unwrap()
                .len(),
            2
        );
        assert!(candidate_entry_names("unknown", json, "demo", None).is_err());
    }

    #[test]
    fn resolves_local_agent_config_paths_and_upgrade_targets() {
        let root =
            std::env::temp_dir().join(format!("safeselect-agent-paths-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let opencode = get_local_client_config("opencode", Some(&root)).unwrap();
        assert_eq!(opencode, root.join(".opencode/opencode.jsonc"));
        let cursor = get_local_client_config("cursor", Some(&root)).unwrap();
        assert_eq!(cursor, root.join(".cursor/mcp.json"));
        std::fs::create_dir_all(opencode.parent().unwrap()).unwrap();
        std::fs::write(
            &opencode,
            serde_json::json!({"mcp":{"safe":{"type":"local","command":["safeselect"]}}})
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            candidate_upgrade_config_paths("opencode", Some(&root), true).unwrap(),
            vec![opencode.clone()]
        );
        let candidates = candidate_upgrade_config_paths("opencode", Some(&root), false).unwrap();
        assert!(candidates.contains(&opencode));
        assert_eq!(
            resolve_upgrade_config_path_for_name("opencode", "safe", Some(&root), true).unwrap(),
            opencode
        );
        assert!(
            resolve_upgrade_config_path_for_name("opencode", "missing", Some(&root), true).is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn covers_entry_builder_clients_and_selection_fallbacks() {
        let root =
            std::env::temp_dir().join(format!("safeselect-agent-builder-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let content = r#"{"mcpServers":{}}"#;
        for client in [
            "opencode",
            "cursor",
            "windsurf",
            "codex",
            "claude-code",
            "copilot",
            "gemini-cli",
            "unknown",
        ] {
            let _ = build_entry_content(
                client,
                content,
                "entry",
                "dev",
                Some(&root),
                None,
                1000,
                false,
                None,
            );
            let _ = build_entry_content(
                client,
                content,
                "entry-new",
                "dev",
                Some(&root),
                None,
                1000,
                true,
                Some("entry"),
            );
        }
        let _ = select_install_config("cursor", None, false);
        let _ = select_install_config("cursor", Some(&root), false);
        let config = get_local_client_config("cursor", Some(&root)).unwrap();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, r#"{"mcpServers":{}}"#).unwrap();
        install_entry("cursor", "dev", "entry", Some(&root), None, 1000, true).unwrap();
        upgrade_entry(
            "cursor",
            Some("entry"),
            Some("dev"),
            Some(&root),
            None,
            1000,
            true,
        )
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstalls_a_local_opencode_entry_and_creates_backup() {
        let root = std::env::temp_dir().join(format!("safeselect-agent-{}", uuid::Uuid::new_v4()));
        let dir = root.join(".opencode");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        std::fs::write(
            &path,
            r#"{"mcp":{"safeselect-demo-pre":{"command":"safeselect"}}}"#,
        )
        .unwrap();
        uninstall_entry("opencode", "safeselect-demo-pre", Some(&root)).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("safeselect-demo-pre"));
        assert!(path.with_extension("safeselect.bak").exists());
        std::fs::write(&path, "{}").unwrap();
        assert!(uninstall_entry("opencode", "missing", Some(&root)).is_err());
        std::fs::write(&path, r#"{"mcp":{"safeselect-demo-pre":{"command":}}}"#).unwrap();
        assert!(uninstall_entry("opencode", "safeselect-demo-pre", Some(&root)).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
