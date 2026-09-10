#![allow(dead_code)]

mod agents;
mod audit;
mod backend;
mod cli;
mod compass;
mod compose;
mod config;
mod dbeaver;
mod diagnostics;
mod error;
mod mcp;
mod posture;
mod security;
mod sidecar;

use clap::Parser;
use cli::{AgentAction, Cli, Command, ConfigAction, DriverAction};
use config::ConfigLoader;
use diagnostics::{DiagnosticCode, DiagnosticStatus};
use error::{Result, SafeselectError};
use sidecar::{format_elapsed, ResultLimits, SidecarProcess};
use std::path::{Path, PathBuf};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        tracing::error!("{}", redact_cli_error(&e));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let loader = ConfigLoader::new();

    match cli.command {
        Command::Serve {
            project,
            environment,
        } => match resolve_project_dir(&loader, project.clone()) {
            Ok(dir) => cmd_serve(&loader, &dir, &environment),
            Err(_) => {
                let cwd = project
                    .clone()
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                if !cwd.exists() {
                    return Err(SafeselectError::Other(
                        "Project path does not exist.".into(),
                    ));
                }
                cmd_serve_setup(&loader, &cwd)
            }
        },
        Command::Config { action } => cmd_config(&loader, action),
        Command::Driver { action } => cmd_driver(&loader, action),
        Command::Agent { action } => cmd_agent(action),
        Command::ImportDbeaver {
            path,
            non_interactive,
        } => cmd_import_dbeaver(&path, non_interactive),
        Command::ImportCompose {
            path,
            non_interactive,
        } => cmd_import_compose(path, non_interactive),
        Command::ImportCompass {
            path,
            non_interactive,
        } => cmd_import_compass(path, non_interactive),
        Command::Check {
            project,
            environment,
            verbose,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            run_checks(&dir, environment.as_deref(), verbose, true)
        }
        Command::Doctor {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            run_checks(&dir, environment.as_deref(), false, false)
        }
        Command::Posture {
            project,
            environment,
            format,
            strict,
            acknowledge,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            let environments = selected_environment_names(&dir, environment.as_deref())?;
            cmd_posture(
                &loader,
                &dir,
                &environments,
                &format,
                strict,
                acknowledge,
                environment.is_none(),
            )
        }
        Command::Query {
            project,
            environment,
            sql,
            verbose,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_query(&loader, &dir, &environment, sql.as_deref(), verbose)
        }
        Command::Disconnect {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_connectivity_action(&loader, &dir, &environment, "disconnect")
        }
        Command::Connect {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_connectivity_action(&loader, &dir, &environment, "connect")
        }
        Command::Reconnect {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            if let Some(environment) = environment {
                cmd_reconnect(&loader, &dir, &environment)
            } else {
                let env_names = list_environment_names(&dir)?;
                if env_names.is_empty() {
                    println!("No environments found in the selected project.");
                    return Ok(());
                }
                run_reconnects(&loader, &dir, &env_names)
            }
        }
        Command::Uninstall { force, binary_only } => cmd_uninstall(force, binary_only),
    }
}

fn resolve_project_dir(loader: &ConfigLoader, cli_project: Option<PathBuf>) -> Result<PathBuf> {
    match cli_project {
        Some(dir) => {
            if dir.join(".safeselect").is_dir() {
                Ok(dir)
            } else {
                Err(SafeselectError::LocalProjectNotFound(dir))
            }
        }
        None => {
            let cwd = std::env::current_dir()?;
            loader
                .find_local_project(&cwd)
                .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))
        }
    }
}

fn project_display_name(dir: &std::path::Path) -> String {
    config::project_account_prefix(dir)
}

fn redact_cli_error(error: &SafeselectError) -> String {
    match error {
        SafeselectError::LocalProjectNotFound(_) => {
            "Local SafeSelect project not found. Use --project or run from a project directory."
                .into()
        }
        error => error.to_string(),
    }
}

fn resolve_local_for_cli(
    loader: &ConfigLoader,
    repo_root: &Path,
    environment: &str,
) -> Result<config::ResolvedConfig> {
    loader
        .resolve_local(repo_root, environment)
        .map_err(redact_resolution_error)
}

fn redact_resolution_error(error: SafeselectError) -> SafeselectError {
    match error {
        SafeselectError::EnvVarNotSet(_)
        | SafeselectError::KeychainNotFound(_)
        | SafeselectError::Secret(_) => {
            SafeselectError::Other("Required secret could not be resolved.".into())
        }
        SafeselectError::Config(_)
        | SafeselectError::Toml(_)
        | SafeselectError::TomlSer(_)
        | SafeselectError::Io(_) => {
            SafeselectError::Other("Configuration could not be resolved.".into())
        }
        SafeselectError::EnvironmentNotFound(_, _) => {
            SafeselectError::Other("Requested environment configuration was not found.".into())
        }
        SafeselectError::DriverFileNotFound(_) | SafeselectError::InsecurePermissions(_) => {
            SafeselectError::Other("Configured driver file is unavailable or unsafe.".into())
        }
        error => error,
    }
}

fn redact_connection_start_error(error: SafeselectError) -> SafeselectError {
    match error {
        SafeselectError::Sidecar(_) | SafeselectError::SidecarJavaNotFound(_) => {
            SafeselectError::Other(
                "Database connection could not be started. Check the connection configuration and driver availability."
                    .into(),
            )
        }
        error => error,
    }
}

fn redact_audit_initialization_error(error: SafeselectError) -> SafeselectError {
    match error {
        SafeselectError::Audit(_) => SafeselectError::Other(
            "Audit logging could not be initialized. Check the audit configuration and permissions."
                .into(),
        ),
        error => error,
    }
}

fn list_environment_names(repo_root: &Path) -> Result<Vec<String>> {
    let env_dir = repo_root.join(".safeselect").join("environments");
    let mut env_names = Vec::new();
    let entries = std::fs::read_dir(&env_dir).map_err(|_| {
        SafeselectError::Config("Unable to read environment configurations.".into())
    })?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
            env_names.push(name.to_string());
        }
    }

    env_names.sort();
    Ok(env_names)
}

fn selected_environment_names(repo_root: &Path, environment: Option<&str>) -> Result<Vec<String>> {
    match environment {
        Some(environment) => Ok(vec![environment.to_string()]),
        None => list_environment_names(repo_root),
    }
}

fn print_no_environments(_repo_root: &Path) {
    println!("No environments found in the selected project.");
}

fn cmd_serve(loader: &ConfigLoader, repo_root: &std::path::Path, environment: &str) -> Result<()> {
    tracing::info!("Loading configuration for environment {environment}");

    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;
    let name = project_display_name(repo_root);

    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            tracing::warn!("SSH tunnel configured — ensure it is active before connecting");
        }
    }

    tracing::info!("Starting MCP server (sidecar will start lazily on first query)");

    let db_url = resolved.environment.database.url.clone();
    let db_username = resolved.environment.database.username.clone();
    let db_password = resolved.password.clone();
    let driver_path = resolved
        .driver
        .as_ref()
        .map(|driver| driver.path.clone())
        .unwrap_or_default();
    let driver_class = resolved
        .driver
        .as_ref()
        .map(|driver| driver.class.clone())
        .unwrap_or_default();

    let mut server = mcp::McpServer::new(
        resolved.project,
        resolved.environment,
        &name,
        environment,
        &driver_path,
        &driver_class,
        &db_url,
        &db_username,
        &db_password,
        repo_root,
        loader.config_dir(),
    )
    .map_err(redact_audit_initialization_error)?;

    server.run()?;

    Ok(())
}

fn cmd_config_show(
    loader: &ConfigLoader,
    project: Option<PathBuf>,
    environment: String,
) -> Result<()> {
    let dir = resolve_project_dir(loader, project)?;
    let resolved = resolve_local_for_cli(loader, &dir, &environment)?;
    println!("Project configuration: loaded");
    println!("Environment: {environment}");
    println!("Backend: {:?}", resolved.environment.database.kind);
    println!("Vendor: {}", resolved.environment.database.vendor());
    if resolved.driver.is_some() {
        println!("Driver: configured");
    }
    println!("Connection: configured (details redacted)");
    println!("Username: [redacted]");
    println!("Password: [redacted]");
    println!();
    println!("--- Security Policy ---");
    println!("Read only: enforced (cannot be disabled)");
    println!(
        "Allowed schemas: {}",
        resolved.project.security.allowed_schemas.join(", ")
    );
    println!(
        "Denied relations: {}",
        resolved.project.security.denied_relations.join(", ")
    );
    println!(
        "Single statement: {}",
        resolved.project.security.require_single_statement
    );
    println!();
    println!("--- Limits ---");
    println!(
        "Statement timeout: {}ms",
        resolved.project.limits.statement_timeout_ms
    );
    println!("Max rows: {}", resolved.project.limits.max_rows);
    println!(
        "Max result bytes: {}",
        resolved.project.limits.max_result_bytes
    );
    println!();
    println!("--- TLS ---");
    let tls_details = resolved
        .environment
        .tls
        .as_ref()
        .map(|tls| format!("Mode: {}", tls.mode))
        .unwrap_or_else(|| "TLS: disabled".to_string());
    println!("{tls_details}");
    println!();
    println!("--- SSH ---");
    let ssh_details = resolved
        .environment
        .ssh
        .as_ref()
        .map(|ssh| format!("Enabled: {}", ssh.enabled))
        .unwrap_or_else(|| "SSH: not configured".to_string());
    println!("{ssh_details}");

    Ok(())
}

fn cmd_config_validate(
    loader: &ConfigLoader,
    project: Option<PathBuf>,
    environment: Option<String>,
) -> Result<()> {
    match project {
        Some(dir) => validate_explicit_project(loader, &dir, environment.as_deref()),
        None => {
            let cwd = std::env::current_dir()?;
            validate_current_project(loader, &cwd, environment.as_deref())
        }
    }
}

fn validate_explicit_project(
    loader: &ConfigLoader,
    dir: &Path,
    environment: Option<&str>,
) -> Result<()> {
    if !dir.join(".safeselect").is_dir() {
        return Err(SafeselectError::LocalProjectNotFound(dir.to_path_buf()));
    }
    match environment {
        Some(env) => validate_environment_config(loader, dir, env),
        None => validate_all_environment_configs(loader, dir),
    }
}

fn validate_environment_config(loader: &ConfigLoader, dir: &Path, environment: &str) -> Result<()> {
    let _ = resolve_local_for_cli(loader, dir, environment)?;
    print_terminal_line(&format!("✓ Config valid: {environment}"));
    Ok(())
}

fn validate_all_environment_configs(loader: &ConfigLoader, dir: &Path) -> Result<()> {
    let environments = list_environment_names(dir)?;
    if environments.is_empty() {
        return Err(no_environments_error());
    }

    for env in environments {
        validate_environment_config(loader, dir, &env)?;
    }
    Ok(())
}

fn validate_current_project(
    loader: &ConfigLoader,
    cwd: &Path,
    environment: Option<&str>,
) -> Result<()> {
    let Some(dir) = loader.find_local_project(cwd) else {
        println!("No .safeselect/ directory found. Create one with:");
        println!("  safeselect import-dbeaver <export.zip>");
        println!("  mkdir -p .safeselect/environments && touch .safeselect/project.toml");
        return Ok(());
    };

    println!(".safeselect/ directory found.");
    validate_explicit_project(loader, &dir, environment)
}

fn delete_environment_config(
    loader: &ConfigLoader,
    name: String,
    project: Option<PathBuf>,
) -> Result<()> {
    let dir = resolve_project_dir(loader, project)?;
    let env_dir = dir.join(".safeselect").join("environments");
    let env_file = env_dir.join(format!("{name}.toml"));

    if !env_file.exists() {
        return Err(SafeselectError::EnvironmentNotFound(
            name,
            env_dir.display().to_string(),
        ));
    }

    let old_content = std::fs::read_to_string(&env_file).ok();
    let secret_source = old_content.as_ref().and_then(|content| {
        let env_config: config::EnvironmentConfig = toml::from_str(content).ok()?;
        env_config
            .database
            .secret
            .map(|secret| (secret.source, secret.account, secret.variable))
    });

    std::fs::remove_file(&env_file)?;
    let mut removed = format!("Deleted environment '{name}'");
    removed.push_str(&format!("\n  File: {}", env_file.display()));
    append_deleted_secret_message(&mut removed, secret_source)?;
    println!("{removed}");
    Ok(())
}

fn append_deleted_secret_message(
    removed: &mut String,
    secret_source: Option<(String, Option<String>, Option<String>)>,
) -> Result<()> {
    let Some((source, account, _variable)) = secret_source else {
        return Ok(());
    };
    match source.as_str() {
        "macos-keychain" if cfg!(target_os = "macos") => {
            if let Some(account) = account {
                compose::delete_password_from_keychain(&account)?;
                removed.push_str("\n  Keychain entry deleted.");
            }
        }
        "env" => removed.push_str(
            "\n  Environment variable was not removed — delete it manually if no longer needed.",
        ),
        _ => {}
    }
    Ok(())
}

fn cmd_config_reset(loader: &ConfigLoader, project: Option<PathBuf>) -> Result<()> {
    let dir = resolve_project_dir(loader, project)?;
    reset_project_config(&dir)
}

fn cmd_config_uninstall(loader: &ConfigLoader, project: Option<PathBuf>) -> Result<()> {
    let dir = resolve_project_dir(loader, project)?;
    uninstall_project_config(&dir)
}

fn set_password_for_environment(
    loader: &ConfigLoader,
    environment: String,
    password: Option<String>,
    project: Option<PathBuf>,
) -> Result<()> {
    set_password_for_environment_with_store(
        loader,
        environment,
        password,
        project,
        compose::store_password_in_keychain,
    )
}

fn set_password_for_environment_with_store<F>(
    loader: &ConfigLoader,
    environment: String,
    password: Option<String>,
    project: Option<PathBuf>,
    store_password: F,
) -> Result<()>
where
    F: FnOnce(&str, &str) -> Result<()>,
{
    let dir = resolve_project_dir(loader, project)?;
    let env_file = environment_config_file(&dir, &environment);
    if !env_file.exists() {
        return Err(SafeselectError::EnvironmentNotFound(
            environment,
            env_file.display().to_string(),
        ));
    }

    let content = std::fs::read_to_string(&env_file)?;
    let env_config: config::EnvironmentConfig = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid {}: {e}", env_file.display())))?;
    let account = config::preferred_keychain_account(&dir, &environment, &env_config);
    let password = resolve_password(password, &account)?;

    store_password(&account, &password)?;
    print_terminal_line(&format!("  ✓ Password stored in Keychain ({account})"));
    config::write_keychain_secret_to_env_file(&env_file, &account)?;
    print_terminal_line(&format!("  ✓ Updated {}", env_file.display()));
    println!("\nDone. Run: safeselect check --environment {environment}");
    Ok(())
}

fn resolve_password(password: Option<String>, account: &str) -> Result<String> {
    password.map(Ok).unwrap_or_else(|| {
        inquire::Password::new(&format!("Password for '{account}'"))
            .without_confirmation()
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Failed to read password: {e}")))
    })
}

fn set_ssh_password_for_environment(
    loader: &ConfigLoader,
    environment: String,
    password: Option<String>,
    project: Option<PathBuf>,
) -> Result<()> {
    set_ssh_password_for_environment_with_store(
        loader,
        environment,
        password,
        project,
        compose::store_password_in_keychain,
    )
}

fn set_ssh_password_for_environment_with_store<F>(
    loader: &ConfigLoader,
    environment: String,
    password: Option<String>,
    project: Option<PathBuf>,
    store_password: F,
) -> Result<()>
where
    F: FnOnce(&str, &str) -> Result<()>,
{
    let dir = resolve_project_dir(loader, project)?;
    let (env_file, mut env_config) = load_ssh_environment_config(&dir, &environment)?;
    let ssh = env_config.ssh.as_mut().ok_or_else(|| {
        SafeselectError::Config(format!(
            "environment '{environment}' has no SSH configuration"
        ))
    })?;
    let account = ssh
        .secret_account
        .clone()
        .unwrap_or_else(|| format!("{}/{environment}/ssh", project_display_name(&dir)));
    let password = resolve_ssh_password(password, &account)?;

    store_password(&account, &password)?;
    ssh.secret_account = Some(account.clone());
    ssh.auth_type = Some("PASSWORD".to_string());
    ssh.identity_file = None;
    let env_toml =
        toml::to_string_pretty(&env_config).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(&env_file, env_toml)?;
    print_terminal_line(&format!("  ✓ SSH password stored in Keychain ({account})"));
    print_terminal_line(&format!("  ✓ Updated {}", env_file.display()));
    println!("\nDone. Run: safeselect check --environment {environment}");
    Ok(())
}

fn load_ssh_environment_config(
    dir: &Path,
    environment: &str,
) -> Result<(PathBuf, config::EnvironmentConfig)> {
    let env_file = environment_config_file(dir, environment);
    if !env_file.exists() {
        return Err(SafeselectError::EnvironmentNotFound(
            environment.to_string(),
            env_file.display().to_string(),
        ));
    }
    let content = std::fs::read_to_string(&env_file)?;
    let env_config = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid {}: {e}", env_file.display())))?;
    Ok((env_file, env_config))
}

fn resolve_ssh_password(password: Option<String>, account: &str) -> Result<String> {
    password.map(Ok).unwrap_or_else(|| {
        inquire::Password::new(&format!("SSH password for '{account}'"))
            .without_confirmation()
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Failed to read SSH password: {e}")))
    })
}

fn environment_config_file(project: &Path, environment: &str) -> PathBuf {
    project
        .join(".safeselect")
        .join("environments")
        .join(format!("{environment}.toml"))
}

fn cmd_config(loader: &ConfigLoader, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Validate {
            project,
            environment,
        } => cmd_config_validate(loader, project, environment),
        ConfigAction::Show {
            project,
            environment,
        } => cmd_config_show(loader, project, environment),
        ConfigAction::RenameEnvironment { old, new, project } => {
            let dir = resolve_project_dir(loader, project)?;

            let env_dir = dir.join(".safeselect").join("environments");
            let old_file = env_dir.join(format!("{old}.toml"));
            let new_file = env_dir.join(format!("{new}.toml"));

            if !old_file.exists() {
                return Err(SafeselectError::EnvironmentNotFound(
                    old.clone(),
                    env_dir.display().to_string(),
                ));
            }
            if new_file.exists() {
                return Err(SafeselectError::Other(format!(
                    "Environment '{new}' already exists"
                )));
            }

            let project_name = project_display_name(&dir);
            let old_account = format!("{project_name}/{old}");
            let new_account = format!("{project_name}/{new}");

            // Read old config to check for secrets
            let old_content = std::fs::read_to_string(&old_file)?;
            let mut env_config: config::EnvironmentConfig = toml::from_str(&old_content)
                .map_err(|e| SafeselectError::Config(format!("invalid {old}.toml: {e}")))?;

            let mut needs_rewrite = false;

            // Migrate keychain secret
            if let Some(ref mut secret) = env_config.database.secret {
                match secret.source.as_str() {
                    "macos-keychain" if cfg!(target_os = "macos") => {
                        if let Ok(password) = compose::read_password_from_keychain(&old_account) {
                            compose::store_password_in_keychain(&new_account, &password)?;
                            compose::delete_password_from_keychain(&old_account)?;
                            secret.account = Some(new_account.clone());
                            needs_rewrite = true;
                        }
                    }
                    "env" => {
                        let var = format!(
                            "SAFESELECT_PASSWORD_{}",
                            new.to_uppercase().replace('-', "_")
                        );
                        secret.variable = Some(var.clone());
                        needs_rewrite = true;
                    }
                    _ => {}
                }
            }

            // Rename file
            std::fs::rename(&old_file, &new_file)?;

            // Rewrite with updated secret account/variable if needed
            if needs_rewrite {
                let new_content = toml::to_string_pretty(&env_config)
                    .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
                std::fs::write(&new_file, new_content)?;
            }

            println!("Renamed '{old}' → '{new}'");
            println!("  File: {} → {}", old_file.display(), new_file.display());

            if needs_rewrite {
                println!("  Secret migrated to new environment name.");
            } else if env_config.database.secret.is_some() {
                println!("  Secret NOT migrated — update it manually.");
            }

            Ok(())
        }
        ConfigAction::DeleteEnvironment { name, project } => {
            delete_environment_config(loader, name, project)
        }
        ConfigAction::SetPassword {
            environment,
            password,
            project,
        } => set_password_for_environment(loader, environment, password, project),
        ConfigAction::SetSshPassword {
            environment,
            password,
            project,
        } => set_ssh_password_for_environment(loader, environment, password, project),
        ConfigAction::Reset { project } => cmd_config_reset(loader, project),
        ConfigAction::Uninstall { project } => cmd_config_uninstall(loader, project),
    }
}

fn clear_project_config(repo_root: &Path, delete_dir: bool) -> Result<()> {
    let safeselect_dir = repo_root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    let project_name = project_display_name(repo_root);
    let has_env_dir = env_dir.is_dir();
    let project_file = safeselect_dir.join("project.toml");
    let has_project_file = project_file.exists();
    if !has_env_dir && !has_project_file {
        println!("  ◉ No environments or project config to clear.");
        return Ok(());
    }

    let prompt = if delete_dir {
        "This will remove the entire .safeselect directory and related keychain entries. Continue?"
    } else {
        "This will remove all environments, shared SSH bastions, and related keychain entries. Continue?"
    };
    let ans = inquire::Confirm::new(prompt)
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    if !ans {
        println!("Cancelled.");
        return Ok(());
    }

    let mut removed = 0u32;
    if has_env_dir {
        if let Ok(entries) = std::fs::read_dir(&env_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(env_cfg) = toml::from_str::<config::EnvironmentConfig>(&content) {
                            if let Some(ref secret) = env_cfg.database.secret {
                                if secret.source == "macos-keychain" {
                                    if let Some(ref acct) = secret.account {
                                        let _ = compose::delete_password_from_keychain(acct);
                                    }
                                }
                            }
                            if let Some(ref ssh) = env_cfg.ssh {
                                if let Some(ref bastion) = ssh.bastion {
                                    let ssh_account = format!("{project_name}/{bastion}/ssh");
                                    let _ = compose::delete_password_from_keychain(&ssh_account);
                                }
                            }
                        }
                    }
                    let _ = std::fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
    }

    if removed > 0 {
        print_terminal_line(&format!("  ✓ Removed {removed} environment(s)"));
    }

    // Reset generated_by in project.toml
    if project_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&project_file) {
            if let Ok(mut proj) = toml::from_str::<config::ProjectConfig>(&content) {
                proj.generated_by = Some(env!("CARGO_PKG_VERSION").to_string());
                proj.ssh_bastions.clear();
                if !delete_dir {
                    if let Ok(new_content) = toml::to_string_pretty(&proj) {
                        let _ = std::fs::write(&project_file, new_content);
                    }
                }
            }
        }
    }

    if delete_dir {
        if safeselect_dir.exists() {
            std::fs::remove_dir_all(&safeselect_dir)?;
            print_terminal_line(&format!("  ✓ Removed {}", safeselect_dir.display()));
        }
    } else if removed > 0 {
        println!("\nReset complete. Re-import with:");
        println!("  safeselect import-dbeaver <export.zip>");
        println!("  safeselect import-compose");
    } else if has_project_file {
        print_terminal_line("  ✓ Cleared shared SSH bastions from project config");
    } else {
        println!("  ◉ No environment files found.");
    }

    Ok(())
}

fn reset_project_config(repo_root: &Path) -> Result<()> {
    clear_project_config(repo_root, false)
}

fn uninstall_project_config(repo_root: &Path) -> Result<()> {
    clear_project_config(repo_root, true)
}

fn cmd_driver(loader: &ConfigLoader, action: DriverAction) -> Result<()> {
    match action {
        DriverAction::Add {
            vendor,
            path,
            class,
            sha256,
        } => {
            use sha2::{Digest, Sha256};

            let driver_path = std::path::Path::new(&path);
            if !driver_path.exists() {
                return Err(SafeselectError::DriverFileNotFound(
                    driver_path.to_path_buf(),
                ));
            }

            let checksum = match sha256 {
                Some(h) => h,
                None => {
                    let mut file = std::fs::File::open(driver_path)?;
                    let mut hasher = Sha256::new();
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut file, &mut buf)?;
                    hasher.update(&buf);
                    hex::encode(hasher.finalize())
                }
            };

            let config = config::DriverConfig {
                version: 1,
                vendor: vendor.clone(),
                path,
                class,
                sha256: checksum.clone(),
            };

            let driver_dir = loader.drivers_dir();
            std::fs::create_dir_all(driver_dir)?;
            let driver_file = driver_dir.join(format!("{vendor}.toml"));
            let content =
                toml::to_string(&config).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
            std::fs::write(&driver_file, content)?;

            println!("Driver '{vendor}' registered at {}", driver_file.display());
            println!("SHA-256: {checksum}");

            Ok(())
        }
        DriverAction::List => {
            let drivers = loader.list_drivers()?;
            if drivers.is_empty() {
                println!(
                    "No drivers registered in {}",
                    loader.drivers_dir().display()
                );
                println!("Use `safeselect driver add` or `safeselect driver download`");
            } else {
                for (name, config) in &drivers {
                    println!("  {name}: {} ({})", config.class, config.path);
                }
            }
            Ok(())
        }
        DriverAction::Download { vendor } => {
            let url = match vendor.as_str() {
                "postgresql" => "https://jdbc.postgresql.org/download/postgresql-42.7.4.jar",
                v => {
                    return Err(SafeselectError::Other(format!(
                        "Unknown vendor '{v}'. Use `safeselect driver add` for custom drivers."
                    )))
                }
            };

            let driver_dir = loader.drivers_dir();
            std::fs::create_dir_all(driver_dir)?;
            let jar_path = driver_dir.join(format!("{vendor}.jar"));

            println!("Downloading {vendor} driver from {url}...");

            let response = reqwest::blocking::get(url)?;
            let bytes = response.bytes()?;
            std::fs::write(&jar_path, &bytes)?;

            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let checksum = hex::encode(hasher.finalize());

            let config = config::DriverConfig {
                version: 1,
                vendor: vendor.clone(),
                path: jar_path.to_string_lossy().to_string(),
                class: format!("org.{}.Driver", vendor),
                sha256: checksum.clone(),
            };

            let config_path = driver_dir.join(format!("{vendor}.toml"));
            let content =
                toml::to_string(&config).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
            std::fs::write(&config_path, content)?;

            println!("Downloaded and registered '{vendor}' driver");
            println!("  Path: {}", jar_path.display());
            println!("  SHA-256: {checksum}");

            Ok(())
        }
    }
}

fn terminal_line(line: &str, color: bool) -> String {
    if color {
        if line == "OK" {
            return "\x1b[32mOK\x1b[0m".to_string();
        }
        if is_terminal_error_line(line) {
            return format!("\x1b[31m{line}\x1b[0m");
        }
        return line
            .replace('✓', "\x1b[32m✓\x1b[0m")
            .replace("FAILED", "\x1b[31mFAILED\x1b[0m");
    }
    line.to_string()
}

fn is_terminal_error_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("FAILED")
        || trimmed.starts_with("UNSAFE")
        || trimmed.starts_with("Sidecar error:")
        || trimmed.starts_with("SSH error:")
        || trimmed.starts_with("ERROR:")
        || trimmed.starts_with("Reconnect failed")
}

fn print_terminal_line(line: &str) {
    use std::io::IsTerminal;
    let color = std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
        && std::env::var("TERM").is_ok_and(|term| term != "dumb");
    println!("{}", terminal_line(line, color));
}

fn print_terminal_error_line(line: &str) {
    use std::io::IsTerminal;
    let color = std::io::stderr().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
        && std::env::var("TERM").is_ok_and(|term| term != "dumb");
    eprintln!("{}", terminal_line(line, color));
}

fn cmd_agent(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::Detect => {
            let clients = agents::detect_clients()?;
            println!("Detected MCP clients:");
            for client in &clients {
                let status = if client.detected { "✓" } else { "✗" };
                print_terminal_line(&format!("  {status} {}", client.name));
                if client.detected {
                    println!("    Config: {}", client.config_path.display());
                }
            }
            println!(
                "Next: choose one detected client and run `safeselect agent install <client>`."
            );
            Ok(())
        }
        AgentAction::Install {
            client,
            project,
            environment,
            name,
            local,
        } => {
            let loader = ConfigLoader::new();
            let (repo_root, project_dir) = match project {
                Some(dir) => {
                    if !dir.join(".safeselect").is_dir() {
                        return Err(SafeselectError::LocalProjectNotFound(dir.clone()));
                    }
                    (Some(dir.clone()), Some(dir))
                }
                None => {
                    let cwd = std::env::current_dir()?;
                    let found = loader.find_local_project(&cwd);
                    (found.clone(), found)
                }
            };
            let environments = match environment {
                Some(environment) => vec![environment],
                None => {
                    let root = project_dir.as_ref().ok_or_else(|| {
                        SafeselectError::Other(
                            "no .safeselect/ found; use --project or --environment".into(),
                        )
                    })?;
                    let environments = list_environment_names(root)?;
                    match environments.len() {
                        0 => {
                            return Err(SafeselectError::Other(
                                "no environments found; import or create one first".into(),
                            ));
                        }
                        1 => vec![environments[0].clone()],
                        _ => {
                            let selected = inquire::MultiSelect::new(
                                "Select environments to install (Space to toggle, Enter to confirm):",
                                environments,
                            )
                            .with_page_size(20)
                            .prompt()
                            .map_err(|e| {
                                SafeselectError::Other(format!("Cancelled: {e}"))
                            })?;
                            if selected.is_empty() {
                                println!("No environments selected. Nothing to install.");
                                return Ok(());
                            }
                            selected
                        }
                    }
                }
            };
            if environments.len() > 1 && name.is_some() {
                return Err(SafeselectError::Other(
                    "--name cannot be used when installing multiple environments".into(),
                ));
            }

            // Calculate MCP client timeout based on project's statement_timeout_ms
            let mcp_timeout_ms = if let Some(ref root) = repo_root {
                let project_file = root.join(".safeselect").join("project.toml");
                if project_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&project_file) {
                        if let Ok(project) = toml::from_str::<config::ProjectConfig>(&content) {
                            // MCP timeout = statement_timeout + 30s buffer
                            project.limits.statement_timeout_ms + 30_000
                        } else {
                            120_000 // Default 2 minutes if config parse fails
                        }
                    } else {
                        120_000 // Default 2 minutes if file read fails
                    }
                } else {
                    120_000 // Default 2 minutes if no project.toml
                }
            } else {
                120_000 // Default 2 minutes if no repo_root
            };

            let root = project_dir.ok_or_else(|| {
                SafeselectError::Other(
                    "no .safeselect/ found; use --project or run from a project directory".into(),
                )
            })?;
            for environment in environments {
                let entry_name = match &name {
                    Some(name) => name.clone(),
                    None => agents::canonical_entry_name(Some(&root), &environment).ok_or_else(
                        || {
                            SafeselectError::Other(
                                "could not derive a safe MCP entry name from the project path"
                                    .into(),
                            )
                        },
                    )?,
                };

                agents::install_entry(
                    &client,
                    &environment,
                    &entry_name,
                    repo_root.as_deref(),
                    Some(loader.config_dir()),
                    mcp_timeout_ms,
                    local,
                )?;
            }
            Ok(())
        }
        AgentAction::Upgrade {
            client,
            name,
            project,
            environment,
            local,
        } => {
            let loader = ConfigLoader::new();
            let (repo_root, _project_dir) = match project {
                Some(dir) => {
                    if !dir.join(".safeselect").is_dir() {
                        return Err(SafeselectError::LocalProjectNotFound(dir.clone()));
                    }
                    (Some(dir), ())
                }
                None => {
                    let cwd = std::env::current_dir()?;
                    (loader.find_local_project(&cwd), ())
                }
            };

            let mcp_timeout_ms = if let Some(ref root) = repo_root {
                let project_file = root.join(".safeselect").join("project.toml");
                if project_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&project_file) {
                        if let Ok(project) = toml::from_str::<config::ProjectConfig>(&content) {
                            project.limits.statement_timeout_ms + 30_000
                        } else {
                            120_000
                        }
                    } else {
                        120_000
                    }
                } else {
                    120_000
                }
            } else {
                120_000
            };

            agents::upgrade_entry(
                &client,
                name.as_deref(),
                environment.as_deref(),
                repo_root.as_deref(),
                Some(loader.config_dir()),
                mcp_timeout_ms,
                local,
            )
        }
        AgentAction::Uninstall { client, name } => {
            let loader = ConfigLoader::new();
            let cwd = std::env::current_dir()?;
            let repo_root = loader.find_local_project(&cwd);
            let entry_name = match name {
                Some(name) => name,
                None => {
                    let (_config_path, detected_name) =
                        agents::detect_uninstall_target(&client, repo_root.as_deref())?;
                    let confirm = inquire::Confirm::new(&format!(
                        "Uninstall '{detected_name}' from {client}?"
                    ))
                    .with_default(true)
                    .prompt()
                    .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
                    if !confirm {
                        println!("Cancelled.");
                        return Ok(());
                    }
                    detected_name
                }
            };
            agents::uninstall_entry(&client, &entry_name, repo_root.as_deref())
        }
        AgentAction::Status => {
            let loader = ConfigLoader::new();
            let cwd = std::env::current_dir()?;
            let repo_root = loader.find_local_project(&cwd);
            println!("Agent integration status:");
            for line in agents::status_lines(repo_root.as_deref())? {
                print_terminal_line(&line);
            }
            println!("Next: install or remove an entry only if the reported state differs from your intent.");
            Ok(())
        }
    }
}

fn check_gitignore(repo_root: &std::path::Path) {
    let gitignore = repo_root.join(".gitignore");
    if gitignore.exists() {
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            if !content
                .lines()
                .any(|l| l.trim() == ".safeselect/" || l.trim() == ".safeselect")
            {
                println!("  ⚠  .safeselect/ not found in .gitignore — consider adding it");
            }
        }
    } else {
        println!(
            "  ⚠  No .gitignore found at {} — consider adding .safeselect/ to it",
            gitignore.display()
        );
    }
}

fn write_project_toml(safeselect_dir: &Path) -> Result<()> {
    let project_file = safeselect_dir.join("project.toml");
    let config = config::ProjectConfig::default();
    let toml_str =
        toml::to_string_pretty(&config).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(&project_file, toml_str)?;
    Ok(())
}

fn load_project_config(safeselect_dir: &Path) -> Result<config::ProjectConfig> {
    let project_file = safeselect_dir.join("project.toml");
    if !project_file.exists() {
        return Ok(config::ProjectConfig::default());
    }
    let content = std::fs::read_to_string(&project_file)?;
    toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid project.toml: {e}")))
}

fn load_environment_config(repo_root: &Path, env_name: &str) -> Result<config::EnvironmentConfig> {
    let safeselect_dir = repo_root.join(".safeselect");
    let env_file = safeselect_dir
        .join("environments")
        .join(format!("{env_name}.toml"));
    let content = std::fs::read_to_string(&env_file)
        .map_err(|e| SafeselectError::Config(format!("cannot read {}: {e}", env_file.display())))?;
    let mut environment: config::EnvironmentConfig = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid {}: {e}", env_file.display())))?;
    let project = load_project_config(&safeselect_dir)?;
    config::merge_project_ssh(&project, &mut environment)?;
    Ok(environment)
}

fn save_project_config(safeselect_dir: &Path, project: &config::ProjectConfig) -> Result<()> {
    let project_file = safeselect_dir.join("project.toml");
    let content =
        toml::to_string_pretty(project).map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(&project_file, content)?;
    Ok(())
}

pub(crate) fn update_generated_by(safeselect_dir: &Path) -> Result<()> {
    let mut proj = load_project_config(safeselect_dir)?;
    proj.generated_by = Some(env!("CARGO_PKG_VERSION").to_string());
    save_project_config(safeselect_dir, &proj)
}

fn check_version_and_maybe_reset(repo_root: &Path) -> Result<()> {
    let project_file = repo_root.join(".safeselect").join("project.toml");
    if !project_file.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&project_file)?;
    let proj: config::ProjectConfig = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid project.toml: {e}")))?;
    let current = env!("CARGO_PKG_VERSION");
    match &proj.generated_by {
        Some(ver) if ver == current => return Ok(()),
        Some(old) => {
            println!("⚠  Existing config was generated by v{old}, current version is v{current}.");
        }
        None => {
            println!("⚠  Existing config was generated by an older version (no version field), current version is v{current}.");
        }
    }
    let ans = inquire::Confirm::new("Reset environments and re-import?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
    if ans {
        reset_project_config(repo_root)?;
    }
    Ok(())
}

/// Prompt user to verify/complete SSH configuration from a DBeaver connection.
fn load_reusable_ssh_configs(
    repo_root: &Path,
    current_env_name: &str,
) -> Result<Vec<(String, config::SshConfig)>> {
    let env_dir = repo_root.join(".safeselect").join("environments");
    let project = load_project_config(&repo_root.join(".safeselect"))?;
    let mut reusable = Vec::new();
    let entries = match std::fs::read_dir(env_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(reusable),
    };

    for entry in entries {
        let entry = entry?;
        if let Some(item) = reusable_ssh_entry(&project, &entry.path(), current_env_name) {
            reusable.push(item);
        }
    }

    reusable.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(reusable)
}

fn reusable_ssh_entry(
    project: &config::ProjectConfig,
    path: &Path,
    current_env_name: &str,
) -> Option<(String, config::SshConfig)> {
    let name = reusable_environment_name(path, current_env_name)?;
    let ssh = reusable_ssh_config(project, path)?;
    let bastion_name = ssh.bastion.clone().unwrap_or_else(|| name.clone());
    Some((bastion_name, ssh))
}

fn reusable_environment_name(path: &Path, current: &str) -> Option<String> {
    (path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .then(|| path.file_stem().and_then(|stem| stem.to_str()))
        .flatten()
        .filter(|name| *name != current)
        .map(str::to_owned)
}

fn reusable_ssh_config(project: &config::ProjectConfig, path: &Path) -> Option<config::SshConfig> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut environment = toml::from_str::<config::EnvironmentConfig>(&content).ok()?;
    config::merge_project_ssh(project, &mut environment).ok()?;
    let ssh = environment.ssh?;
    (ssh.enabled && ssh.host.as_deref().is_some_and(|host| !host.is_empty())).then_some(ssh)
}

fn collect_reusable_ssh_configs(
    repo_root: &Path,
    current_env_name: &str,
    current_batch: &[(String, config::SshConfig)],
) -> Result<Vec<(String, config::SshConfig)>> {
    let mut reusable = load_reusable_ssh_configs(repo_root, current_env_name)?;

    for (bastion_name, ssh) in current_batch {
        if ssh.bastion.as_deref() == Some(current_env_name) {
            continue;
        }
        if !ssh.enabled {
            continue;
        }
        if ssh.host.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        reusable.push((bastion_name.clone(), ssh.clone()));
    }

    reusable.sort_by(|left, right| left.0.cmp(&right.0));
    reusable.dedup_by(|left, right| left.0 == right.0);
    Ok(reusable)
}

fn select_reusable_ssh_config(
    repo_root: &Path,
    env_name: &str,
    conn: &dbeaver::DBeaverConnection,
    current_batch: &[(String, config::SshConfig)],
) -> Result<Option<config::SshConfig>> {
    let reusable = collect_reusable_ssh_configs(repo_root, env_name, current_batch)?;
    if reusable.is_empty() {
        return Ok(None);
    }

    let reuse = inquire::Confirm::new("Reuse an existing bastion configuration?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
    if !reuse {
        return Ok(None);
    }

    let options: Vec<String> = reusable
        .iter()
        .map(|(name, ssh)| {
            let host = ssh.host.as_deref().unwrap_or("unknown");
            let port = ssh.port.unwrap_or(22);
            let user = ssh.username.as_deref().unwrap_or("unknown");
            format!("{name} ({user}@{host}:{port})")
        })
        .collect();

    let selected = inquire::Select::new("  Reuse bastion:", options)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
    let selected_index = reusable
        .iter()
        .position(|(name, ssh)| {
            let host = ssh.host.as_deref().unwrap_or("unknown");
            let port = ssh.port.unwrap_or(22);
            let user = ssh.username.as_deref().unwrap_or("unknown");
            format!("{name} ({user}@{host}:{port})") == selected
        })
        .ok_or_else(|| SafeselectError::Other("selected bastion not found".to_string()))?;

    let (bastion_name, mut ssh) = reusable[selected_index].clone();
    ssh.enabled = true;
    ssh.bastion = Some(bastion_name);
    ssh.forward_host = Some(conn.host.clone());
    ssh.forward_port = Some(conn.port);
    ssh.local_port = None;
    if ssh.local_host.as_deref().is_none_or(str::is_empty) {
        ssh.local_host = Some("localhost".to_string());
    }
    Ok(Some(ssh))
}

fn prompt_ssh_config(
    conn: &dbeaver::DBeaverConnection,
    project_name: &str,
    env_name: &str,
    repo_root: &Path,
    current_batch: &[(String, config::SshConfig)],
) -> Result<config::SshConfig> {
    let default_host = conn.ssh_host.as_deref().unwrap_or("");
    let default_user = conn.ssh_user.as_deref().unwrap_or("");
    let default_key = conn.ssh_key_file.as_deref().unwrap_or("");
    let default_auth = conn.ssh_auth_type.as_deref().unwrap_or("KEY");

    println!();
    println!("── SSH Configuration ({env_name}) ───────────────────");
    println!();
    if let Some(warning) = dbeaver_shared_tunnel_warning(conn) {
        println!("  ⚠ {warning}");
        println!();
    }

    if let Some(ssh) = select_reusable_ssh_config(repo_root, env_name, conn, current_batch)? {
        print_terminal_line("  ✓ Reusing bastion configuration");
        return Ok(ssh);
    }

    let ans = inquire::Confirm::new("Configure SSH tunnel now?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    if !ans {
        // Store minimal SSH config with whatever DBeaver extracted
        return Ok(config::SshConfig {
            enabled: true,
            bastion: None,
            host: conn.ssh_host.clone(),
            port: conn.ssh_port,
            username: conn.ssh_user.clone(),
            secret_account: None,
            identity_file: conn.ssh_key_file.clone(),
            known_hosts: None,
            local_host: conn
                .ssh_local_host
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            local_port: conn.ssh_local_port,
            forward_host: Some(conn.host.clone()),
            forward_port: Some(conn.port),
            auth_type: conn.ssh_auth_type.clone(),
        });
    }

    let host = inquire::Text::new("  SSH bastion host:")
        .with_default(default_host)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .to_string();

    let port = inquire::Text::new("  SSH port:")
        .with_default(&conn.ssh_port.map_or("22".into(), |p| p.to_string()))
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .parse::<u16>()
        .unwrap_or(22);

    let user = inquire::Text::new("  SSH user:")
        .with_default(default_user)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .to_string();

    let (forward_host, forward_port) = prompt_dbeaver_forward_target(conn)?;

    let auth_method =
        inquire::Select::new("  Authentication method:", vec!["Key file", "Password"])
            .with_starting_cursor(if default_auth == "PASSWORD" { 1 } else { 0 })
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    let (key_file, auth_type) = match auth_method {
        "Key file" => {
            let kf = inquire::Text::new("  SSH key file path:")
                .with_default(default_key)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
                .trim()
                .to_string();
            (
                if kf.is_empty() { None } else { Some(kf) },
                Some("KEY".into()),
            )
        }
        _ => {
            let ssh_acct = format!("{project_name}/{env_name}/ssh");
            let pw = inquire::Password::new("  SSH password:")
                .without_confirmation()
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Failed to read SSH password: {e}")))?;
            if !pw.is_empty() {
                compose::store_password_in_keychain(&ssh_acct, &pw)?;
                print_terminal_line("  ✓ SSH password stored in Keychain");
            }
            (None, Some("PASSWORD".into()))
        }
    };

    Ok(config::SshConfig {
        enabled: true,
        bastion: None,
        host: Some(host),
        port: Some(port),
        username: Some(user),
        secret_account: if auth_type.as_deref() == Some("PASSWORD") {
            Some(format!("{project_name}/{env_name}/ssh"))
        } else {
            None
        },
        identity_file: key_file,
        known_hosts: None,
        local_host: conn
            .ssh_local_host
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        local_port: conn.ssh_local_port,
        forward_host: Some(forward_host),
        forward_port: Some(forward_port),
        auth_type,
    })
}

fn dbeaver_forward_target_defaults(conn: &dbeaver::DBeaverConnection) -> (String, u16) {
    let looks_like_local_endpoint = conn
        .ssh_local_host
        .as_deref()
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1")
        && conn.ssh_local_port == Some(conn.port);
    if looks_like_local_endpoint {
        (String::new(), conn.port)
    } else {
        (conn.host.clone(), conn.port)
    }
}

fn prompt_dbeaver_forward_target(conn: &dbeaver::DBeaverConnection) -> Result<(String, u16)> {
    let (default_host, default_port) = dbeaver_forward_target_defaults(conn);
    let host = inquire::Text::new("  Database target host through bastion:")
        .with_default(&default_host)
        .prompt()
        .unwrap_or_default()
        .trim()
        .to_string();
    let port = inquire::Text::new("  Database target port through bastion:")
        .with_default(&default_port.to_string())
        .prompt()
        .unwrap_or_default()
        .trim()
        .parse::<u16>()
        .unwrap_or(default_port);
    Ok((host, port))
}

fn dbeaver_shared_tunnel_warning(conn: &dbeaver::DBeaverConnection) -> Option<String> {
    let host = conn.ssh_host.as_deref()?.trim();
    let user = conn.ssh_user.as_deref().unwrap_or("").trim();
    let auth = conn.ssh_auth_type.as_deref().unwrap_or("").trim();
    let local_host = conn.ssh_local_host.as_deref().unwrap_or("").trim();
    let local_port = conn.ssh_local_port.unwrap_or(0);

    let looks_like_local_shared_tunnel =
        host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("127.0.0.1");
    let missing_identity = user.is_empty();
    let no_forward_target = local_host.is_empty() && local_port == 0;
    let password_tunnel = auth.eq_ignore_ascii_case("PASSWORD");

    if looks_like_local_shared_tunnel && missing_identity && no_forward_target && password_tunnel {
        return Some(
            "DBeaver exported a local shared tunnel placeholder (for example localhost:2222) \
instead of the real bastion/user. Enter the real SSH bastion and username manually."
                .to_string(),
        );
    }

    None
}

fn normalize_ssh_auth_type(auth_type: &str) -> String {
    let normalized = auth_type.trim().to_ascii_lowercase();
    if normalized.contains("password") {
        "PASSWORD".to_string()
    } else {
        "KEY".to_string()
    }
}

const DEFAULT_SSH_LOCAL_PORT: u16 = 15432;

fn ssh_local_port_from_config(config: &config::EnvironmentConfig) -> Option<u16> {
    let ssh = config.ssh.as_ref()?;
    if !ssh.enabled {
        return None;
    }

    ssh.local_port.or_else(|| {
        extract_host_port(&config.database.url).and_then(|(host, port)| {
            if host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" {
                Some(port)
            } else {
                None
            }
        })
    })
}

fn collect_used_ssh_local_ports(repo_root: &Path) -> std::collections::HashSet<u16> {
    let mut used = std::collections::HashSet::new();
    let env_dir = repo_root.join(".safeselect").join("environments");
    let entries = match std::fs::read_dir(env_dir) {
        Ok(entries) => entries,
        Err(_) => return used,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(config) = toml::from_str::<config::EnvironmentConfig>(&content) else {
            continue;
        };
        if let Some(port) = ssh_local_port_from_config(&config) {
            used.insert(port);
        }
    }

    used
}

fn next_available_ssh_local_port(used_ports: &std::collections::HashSet<u16>) -> Result<u16> {
    for port in DEFAULT_SSH_LOCAL_PORT..=u16::MAX {
        if !used_ports.contains(&port) {
            return Ok(port);
        }
    }

    Err(SafeselectError::Other(
        "no available SSH local port found".to_string(),
    ))
}

struct ImportedDbeaverEnv {
    env_name: String,
    conn_index: usize,
    ssh: Option<config::SshConfig>,
}

fn sanitize_bastion_name_part(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if normalized == '-' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push(normalized);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn default_bastion_name(ssh: &config::SshConfig) -> String {
    let user = ssh.username.as_deref().unwrap_or("ssh");
    let host = ssh.host.as_deref().unwrap_or("host");
    let port = ssh.port.unwrap_or(22);
    let user_part = sanitize_bastion_name_part(user);
    let host_part = sanitize_bastion_name_part(host);
    let base = if host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" {
        format!("{user_part}-{port}")
    } else {
        format!("{user_part}-{host_part}-{port}")
    };
    if base.trim_matches('-').is_empty() {
        "ssh-bastion".to_string()
    } else {
        base
    }
}

fn same_bastion_identity(shared: &config::SharedSshConfig, ssh: &config::SshConfig) -> bool {
    shared.host == ssh.host && shared.port == ssh.port && shared.username == ssh.username
}

fn find_matching_bastion_name(
    project: &config::ProjectConfig,
    ssh: &config::SshConfig,
) -> Option<String> {
    project
        .ssh_bastions
        .iter()
        .find(|(_, shared)| same_bastion_identity(shared, ssh))
        .map(|(name, _)| name.clone())
}

fn project_ssh_bastion_from_env(ssh: &config::SshConfig) -> config::SharedSshConfig {
    config::SharedSshConfig {
        host: ssh.host.clone(),
        port: ssh.port,
        username: ssh.username.clone(),
        secret_account: ssh.secret_account.clone(),
        identity_file: ssh.identity_file.clone(),
        known_hosts: ssh.known_hosts.clone(),
        auth_type: ssh.auth_type.clone(),
    }
}

fn environment_ssh_from_bastion(name: String, ssh: &config::SshConfig) -> config::SshConfig {
    config::SshConfig {
        enabled: ssh.enabled,
        bastion: Some(name),
        host: None,
        port: None,
        username: None,
        secret_account: None,
        identity_file: None,
        known_hosts: None,
        local_host: ssh.local_host.clone(),
        local_port: ssh.local_port,
        forward_host: ssh.forward_host.clone(),
        forward_port: ssh.forward_port,
        auth_type: None,
    }
}

fn cmd_import_dbeaver(path: &str, non_interactive: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    check_version_and_maybe_reset(&cwd)?;

    let zip_path = Path::new(path);
    if !zip_path.exists() {
        return Err(SafeselectError::Other(format!("File not found: {path}")));
    }

    let connections = dbeaver::import_zip(zip_path)?;

    if connections.is_empty() {
        println!("No database connections found in the DBeaver export.");
        return Ok(());
    }

    // Step 1: select connections
    let selected_indices: Vec<usize> = if non_interactive {
        (0..connections.len()).collect()
    } else {
        struct ConnLabel(usize, String);
        impl std::fmt::Display for ConnLabel {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.1)
            }
        }

        let options: Vec<ConnLabel> = connections
            .iter()
            .enumerate()
            .map(|(i, conn)| {
                let ssh = conn.ssh_host.as_deref().unwrap_or("-");
                ConnLabel(
                    i,
                    format!(
                        "{:<30}  {}:{:<6}  db={:<20}  ssh={}",
                        conn.name, conn.host, conn.port, conn.database, ssh,
                    ),
                )
            })
            .collect();

        let selected = inquire::MultiSelect::new(
            "Select connections to import (Space to toggle, Enter to confirm):",
            options,
        )
        .with_page_size(20)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Selection cancelled: {e}")))?;

        if selected.is_empty() {
            println!("No connections selected. Nothing to import.");
            return Ok(());
        }

        selected.iter().map(|l| l.0).collect()
    };

    // Step 2: choose environment names
    let mut to_import: Vec<(usize, String)> = Vec::with_capacity(selected_indices.len());
    for &idx in &selected_indices {
        let conn = &connections[idx];
        let default_env = conn
            .name
            .split_once(" (")
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or("default")
            .to_lowercase()
            .replace(' ', "-")
            .replace("--", "-");
        let env_name = if non_interactive {
            default_env
        } else {
            let prompt = format!(
                "Environment name for '{}' ({}:{}):",
                conn.name, conn.host, conn.port
            );
            inquire::Text::new(&prompt)
                .with_default(&default_env)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Input cancelled: {e}")))?
                .trim()
                .to_lowercase()
                .replace(' ', "-")
        };
        to_import.push((idx, env_name));
    }

    // Step 3: write config files silently
    let safeselect_dir = cwd.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir)?;
    update_generated_by(&safeselect_dir)?;
    let mut project_config = load_project_config(&safeselect_dir)?;

    let project_name = project_display_name(&cwd);
    let mut planned_envs: Vec<ImportedDbeaverEnv> = Vec::with_capacity(to_import.len());
    let mut used_ssh_local_ports = collect_used_ssh_local_ports(&cwd);
    let mut reusable_ssh_configs: Vec<(String, config::SshConfig)> = Vec::new();

    if !non_interactive {
        println!();
        println!("── SSH Setup ───────────────────────────────────");
    }

    for (idx, env_name) in &to_import {
        let conn = &connections[*idx];
        let has_ssh = conn.ssh_host.is_some();
        let ssh = if has_ssh {
            let mut ssh =
                prompt_ssh_config(conn, &project_name, env_name, &cwd, &reusable_ssh_configs)?;
            let local_port = ssh
                .local_port
                .filter(|port| !used_ssh_local_ports.contains(port))
                .unwrap_or(next_available_ssh_local_port(&used_ssh_local_ports)?);
            ssh.local_port = Some(local_port);
            if ssh.local_host.as_deref().is_none_or(str::is_empty) {
                ssh.local_host = Some("localhost".to_string());
            }
            used_ssh_local_ports.insert(local_port);
            let bastion_name = find_matching_bastion_name(&project_config, &ssh)
                .or_else(|| ssh.bastion.clone())
                .unwrap_or_else(|| default_bastion_name(&ssh));
            project_config
                .ssh_bastions
                .insert(bastion_name.clone(), project_ssh_bastion_from_env(&ssh));
            let env_ssh = environment_ssh_from_bastion(bastion_name.clone(), &ssh);
            let mut reusable_ssh = ssh.clone();
            reusable_ssh.bastion = Some(bastion_name.clone());
            reusable_ssh_configs.push((bastion_name, reusable_ssh));
            Some(env_ssh)
        } else {
            None
        };
        planned_envs.push(ImportedDbeaverEnv {
            env_name: env_name.clone(),
            conn_index: *idx,
            ssh,
        });
    }

    let mut imported_envs: Vec<(String, bool, bool)> = vec![];

    if !non_interactive {
        println!();
        println!("── Database Credentials ───────────────────────");
    }

    for planned in planned_envs {
        let conn = &connections[planned.conn_index];
        let env_name = &planned.env_name;
        let ssh = planned.ssh;

        // URL points through the SSH tunnel when one is configured
        // Preserve an explicit source SSL mode (for example Azure PostgreSQL)
        // while allowing the JDBC driver default when DBeaver did not specify one.
        let url = if let Some(ref ssh) = ssh {
            let local_forward_port = ssh.local_port.unwrap_or(DEFAULT_SSH_LOCAL_PORT);
            let sslmode = conn
                .sslmode
                .as_deref()
                .map(|mode| format!("?sslmode={mode}"))
                .unwrap_or_default();
            format!(
                "jdbc:postgresql://localhost:{}/{}{}",
                local_forward_port, conn.database, sslmode
            )
        } else {
            format!(
                "jdbc:postgresql://{}:{}/{}",
                conn.host, conn.port, conn.database
            )
        };

        let (secret, has_secret) = if let Some(ref pw) = conn.password {
            if !pw.is_empty() {
                let account = format!("{project_name}/{env_name}");
                compose::store_password_in_keychain(&account, pw)?;
                (
                    Some(config::SecretConfig {
                        source: "macos-keychain".to_string(),
                        service: Some("safeselect".to_string()),
                        account: Some(account),
                        variable: None,
                    }),
                    true,
                )
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        // Prompt for missing database username
        let db_username = if conn.username.is_empty() && !non_interactive {
            let prompt = format!("Database username ({env_name}):");
            inquire::Text::new(&prompt)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
                .trim()
                .to_string()
        } else {
            conn.username.clone()
        };

        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                kind: crate::backend::BackendKind::Jdbc,
                vendor: Some(conn.driver.clone()),
                driver: Some(conn.driver.clone()),
                url,
                username: db_username,
                secret,
            },
            tls: None,
            ssh,
            limits: config::LimitsOverride::default(),
        };
        let env_toml = toml::to_string_pretty(&env_config)
            .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        let env_file = env_dir.join(format!("{env_name}.toml"));
        let is_new = !env_file.exists();
        std::fs::write(&env_file, env_toml)?;
        imported_envs.push((env_name.clone(), has_secret, is_new));
    }
    save_project_config(&safeselect_dir, &project_config)?;

    // Step 4: print summary with next steps
    let env_names: Vec<String> = imported_envs.iter().map(|(n, _, _)| n.clone()).collect();
    let no_password_envs: Vec<String> = imported_envs
        .iter()
        .filter(|(_, has_secret, _)| !has_secret)
        .map(|(n, _, _)| n.clone())
        .collect();

    let created = imported_envs.iter().filter(|(_, _, new)| *new).count();
    if created > 0 {
        println!();
        println!("── Import Complete ──────────────────────────────");
        println!();
        print_terminal_line(&format!("  ✓ {created} environment(s) added"));
        check_gitignore(&cwd);
    } else {
        println!("  ◉ All environments already exist.");
    }

    let guidance =
        compose::build_guidance_from_parts(&project_name, &env_names, &no_password_envs, true);
    println!();
    println!("{}", guidance.text);

    // Step 5: shared helpers (driver, passwords, verify)
    setup_driver_if_missing()?;
    setup_passwords_for_missing(&cwd, &env_names)?;
    run_checks_for_environments(&cwd, &env_names, false, true, false)?;
    Ok(())
}

fn cmd_import_compose(path: Option<PathBuf>, non_interactive: bool) -> Result<()> {
    let scan_path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    if !scan_path.exists() {
        return Err(SafeselectError::Other(format!(
            "scan path does not exist: {}",
            scan_path.display()
        )));
    }

    check_version_and_maybe_reset(&scan_path)?;

    let groups = compose::scan_all(&scan_path)?;

    let all_connections: Vec<(String, compose::ComposeConnection)> = groups
        .into_iter()
        .flat_map(|(label, conns)| conns.into_iter().map(move |c| (label.clone(), c)))
        .collect();

    if all_connections.is_empty() {
        println!("No PostgreSQL services found in docker-compose files.");
        return Ok(());
    }

    let mut to_import: Vec<compose::ComposeConnection> = if non_interactive {
        all_connections.iter().map(|(_, c)| c).cloned().collect()
    } else {
        struct ConnLabel(usize, String);
        impl std::fmt::Display for ConnLabel {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.1)
            }
        }

        let options: Vec<ConnLabel> = all_connections
            .iter()
            .enumerate()
            .map(|(i, (label, conn))| {
                ConnLabel(
                    i,
                    format!(
                        "{:<20}  {}  {}:{:<5}  db={:<15}  user={}",
                        label, conn.service, conn.host, conn.port, conn.database, conn.username,
                    ),
                )
            })
            .collect();

        let selected = inquire::MultiSelect::new(
            "Select connections to import (Space to toggle, Enter to confirm):",
            options,
        )
        .with_page_size(20)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Selection cancelled: {e}")))?;

        if selected.is_empty() {
            println!("No connections selected. Nothing to import.");
            return Ok(());
        }

        selected
            .iter()
            .map(|label| all_connections[label.0].1.clone())
            .collect()
    };

    let dest_dir = &scan_path;
    let project_name = project_display_name(dest_dir);

    if !non_interactive {
        for conn in &mut to_import {
            let prompt = format!(
                "Environment name for '{}' ({}:{}):",
                conn.service, conn.host, conn.port
            );
            let new_name = inquire::Text::new(&prompt)
                .with_default(&conn.env_name)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Input cancelled: {e}")))?;
            conn.env_name = new_name.trim().to_lowercase().replace(' ', "-");
        }
    }

    let result = compose::write_config_files(dest_dir, &to_import, &project_name)?;
    update_generated_by(&dest_dir.join(".safeselect"))?;
    let imported_names: Vec<String> = to_import.iter().map(|c| c.env_name.clone()).collect();
    let guidance = compose::build_import_guidance(&project_name, &result, &imported_names, true);

    if result.created > 0 {
        println!();
        println!("── Import Complete ──────────────────────────────");
        println!();
        print_terminal_line(&format!("  ✓ {} environment(s) added", to_import.len()));
        check_gitignore(dest_dir);
    } else {
        println!("  ◉ All environments already exist.");
    }

    println!();
    println!("{}", guidance.text);

    let env_names = guidance.imported_env_names;
    setup_driver_if_missing()?;
    setup_passwords_for_missing(dest_dir, &env_names)?;
    run_checks_for_environments(dest_dir, &env_names, false, true, false)?;

    Ok(())
}

fn import_selected_connections(connections: &[compose::ComposeConnection]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = project_display_name(&cwd);
    let result = compose::write_config_files(&cwd, connections, &name)?;
    update_generated_by(&cwd.join(".safeselect"))?;
    let imported_names: Vec<String> = connections.iter().map(|c| c.env_name.clone()).collect();
    let guidance = compose::build_import_guidance(&name, &result, &imported_names, true);

    if result.created > 0 {
        println!(
            "\nImport complete. {} environment(s) added to .safeselect/.",
            connections.len()
        );
        check_gitignore(&cwd);
    } else {
        println!("All environments already exist. Nothing to import.");
    }

    println!();
    println!("{}", guidance.text);

    let env_names = guidance.imported_env_names;
    setup_driver_if_missing()?;
    setup_passwords_for_missing(&cwd, &env_names)?;
    run_checks_for_environments(&cwd, &env_names, false, true, false)?;

    Ok(())
}

fn cmd_import_compass(path: Option<PathBuf>, non_interactive: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    check_version_and_maybe_reset(&cwd)?;

    let import_path = path.unwrap_or_else(default_compass_path);
    if !import_path.exists() {
        return Err(SafeselectError::Other(format!(
            "Compass path does not exist: {}",
            import_path.display()
        )));
    }

    let connections = compass::import_path(&import_path)?;
    if connections.is_empty() {
        println!(
            "No MongoDB Compass connections found in {}",
            import_path.display()
        );
        return Ok(());
    }

    let selected_indices: Vec<usize> = if non_interactive {
        (0..connections.len()).collect()
    } else {
        struct ConnLabel(usize, String);
        impl std::fmt::Display for ConnLabel {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.1)
            }
        }

        let options: Vec<ConnLabel> = connections
            .iter()
            .enumerate()
            .map(|(i, conn)| ConnLabel(i, format!("{:<30}  {}", conn.name, conn.url)))
            .collect();
        let selected = inquire::MultiSelect::new(
            "Select MongoDB Compass connections to import (Space to toggle, Enter to confirm):",
            options,
        )
        .with_page_size(20)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Selection cancelled: {e}")))?;
        selected.iter().map(|l| l.0).collect()
    };

    if selected_indices.is_empty() {
        println!("No connections selected. Nothing to import.");
        return Ok(());
    }

    let safeselect_dir = cwd.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir)?;
    update_generated_by(&safeselect_dir)?;
    let mut project_config = load_project_config(&safeselect_dir)?;

    let project_name = project_display_name(&cwd);
    let mut imported = vec![];
    let mut used_ssh_local_ports = collect_used_ssh_local_ports(&cwd);
    let mut reusable_ssh_configs: Vec<(String, config::SshConfig)> = Vec::new();
    let mut warnings = vec![];
    for idx in selected_indices {
        let conn = &connections[idx];
        let default_env = slug_env_name(&conn.name);
        let env_name = if non_interactive {
            unique_env_name(&env_dir, &default_env)
        } else {
            let prompt = format!("Environment name for '{}':", conn.name);
            let requested = inquire::Text::new(&prompt)
                .with_default(&default_env)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Input cancelled: {e}")))?;
            unique_env_name(&env_dir, &slug_env_name(&requested))
        };

        let ssh = if non_interactive {
            if let Some(warning) = compass_shared_tunnel_warning(conn) {
                warnings.push(format!("{}: {warning}", conn.name));
            }
            compass_ssh_config(conn)
        } else if conn.ssh_host.is_some() {
            Some(prompt_compass_ssh_config(
                conn,
                &project_name,
                &env_name,
                &cwd,
                &reusable_ssh_configs,
            )?)
        } else {
            None
        };
        let ssh = if let Some(mut ssh) = ssh {
            let local_port = ssh
                .local_port
                .filter(|port| !used_ssh_local_ports.contains(port))
                .unwrap_or(next_available_ssh_local_port(&used_ssh_local_ports)?);
            ssh.local_port = Some(local_port);
            if ssh.local_host.as_deref().is_none_or(str::is_empty) {
                ssh.local_host = Some("localhost".to_string());
            }
            used_ssh_local_ports.insert(local_port);
            let bastion_name = find_matching_bastion_name(&project_config, &ssh)
                .or_else(|| ssh.bastion.clone())
                .unwrap_or_else(|| default_bastion_name(&ssh));
            project_config
                .ssh_bastions
                .insert(bastion_name.clone(), project_ssh_bastion_from_env(&ssh));
            let env_ssh = environment_ssh_from_bastion(bastion_name.clone(), &ssh);
            let mut reusable_ssh = ssh.clone();
            reusable_ssh.bastion = Some(bastion_name.clone());
            reusable_ssh_configs.push((bastion_name, reusable_ssh));
            Some(env_ssh)
        } else {
            None
        };

        let raw_url = if let Some(ref ssh) = ssh {
            rewrite_mongodb_url_for_local_endpoint(
                &conn.url,
                ssh.local_host.as_deref().unwrap_or("localhost"),
                ssh.local_port.unwrap_or(DEFAULT_SSH_LOCAL_PORT),
            )
            .unwrap_or_else(|| conn.url.clone())
        } else {
            conn.url.clone()
        };
        let (mut url, username, mut secret) =
            prepare_mongodb_url(&project_name, &env_name, &raw_url)?;
        if secret.is_none() && !username.is_empty() {
            if non_interactive {
                warnings.push(format!(
                    "{}: Compass did not export a database password; configure it with `safeselect config set-password --environment {}` after import.",
                    conn.name, env_name
                ));
            } else {
                println!();
                println!("── Database Password ({env_name}) ──────────────────");
                println!();
                let account = format!("{project_name}/{env_name}");
                let pw = rpassword::prompt_password(format!(
                    "  Password for '{account}' (leave empty to skip): "
                ))?;
                let pw = pw.trim().to_string();
                if !pw.is_empty() {
                    compose::store_password_in_keychain(&account, &pw)?;
                    url = inject_mongodb_password_placeholder(&raw_url, &username);
                    secret = Some(config::SecretConfig {
                        source: "macos-keychain".to_string(),
                        service: Some("safeselect".to_string()),
                        account: Some(account),
                        variable: None,
                    });
                }
            }
        }
        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                kind: crate::backend::BackendKind::Document,
                vendor: Some("mongodb".to_string()),
                driver: None,
                url,
                username,
                secret,
            },
            tls: None,
            ssh,
            limits: config::LimitsOverride::default(),
        };
        let env_toml = toml::to_string_pretty(&env_config)
            .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        std::fs::write(env_dir.join(format!("{env_name}.toml")), env_toml)?;
        imported.push(env_name);
    }
    save_project_config(&safeselect_dir, &project_config)?;

    println!("Imported MongoDB environments: {}", imported.join(", "));
    for warning in warnings {
        println!("Warning: {warning}");
    }
    if non_interactive {
        println!("Next: safeselect check --environment <name>");
    } else {
        run_checks_for_environments(&cwd, &imported, false, true, false)?;
    }
    Ok(())
}

fn prompt_compass_ssh_config(
    conn: &crate::compass::CompassConnection,
    project_name: &str,
    env_name: &str,
    repo_root: &Path,
    current_batch: &[(String, config::SshConfig)],
) -> Result<config::SshConfig> {
    let placeholder_warning = compass_shared_tunnel_warning(conn);
    let default_host = conn.ssh_host.as_deref().unwrap_or("");
    let default_user = conn.ssh_user.as_deref().unwrap_or("");
    let default_key = conn.ssh_key_file.as_deref().unwrap_or("");
    let default_auth = conn.ssh_auth_type.as_deref().unwrap_or("KEY");

    println!();
    println!("── SSH Configuration ({env_name}) ───────────────────");
    println!();
    if let Some(warning) = placeholder_warning.as_deref() {
        println!("  ⚠ {warning}");
        println!();
    }

    if let Some(ssh) = select_reusable_compass_ssh_config(repo_root, env_name, conn, current_batch)?
    {
        print_terminal_line("  ✓ Reusing bastion configuration");
        return Ok(ssh);
    }

    let ans = inquire::Confirm::new("Configure SSH tunnel now?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    if !ans {
        return Err(SafeselectError::Other(
            "SSH configuration is required".into(),
        ));
    }

    if compass_uses_local_tunnel_endpoint(conn) {
        let use_existing =
            inquire::Confirm::new("Use the existing local tunnel endpoint from Compass?")
                .with_default(true)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
        if use_existing {
            let (forward_host, forward_port) =
                compass_forward_target(conn).unwrap_or((String::new(), 27017));
            let auth_method =
                inquire::Select::new("  Authentication method:", vec!["Key file", "Password"])
                    .with_starting_cursor(if default_auth == "PASSWORD" { 1 } else { 0 })
                    .prompt()
                    .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
            let (key_file, auth_type, secret_account) = match auth_method {
                "Key file" => {
                    let kf = inquire::Text::new("  SSH key file path:")
                        .with_default(default_key)
                        .prompt()
                        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
                        .trim()
                        .to_string();
                    (
                        if kf.is_empty() { None } else { Some(kf) },
                        Some("KEY".into()),
                        None,
                    )
                }
                _ => {
                    let ssh_acct = format!("{project_name}/{env_name}/ssh");
                    let pw = inquire::Password::new("  SSH password:")
                        .without_confirmation()
                        .prompt()
                        .map_err(|e| {
                            SafeselectError::Other(format!("Failed to read SSH password: {e}"))
                        })?;
                    if !pw.is_empty() {
                        compose::store_password_in_keychain(&ssh_acct, &pw)?;
                        print_terminal_line("  ✓ SSH password stored in Keychain");
                    }
                    (None, Some("PASSWORD".into()), Some(ssh_acct))
                }
            };
            return Ok(config::SshConfig {
                enabled: true,
                bastion: None,
                host: conn.ssh_host.clone(),
                port: Some(conn.ssh_port.unwrap_or(22)),
                username: conn.ssh_user.clone(),
                secret_account,
                identity_file: key_file,
                known_hosts: None,
                local_host: Some("localhost".to_string()),
                local_port: None,
                forward_host: if forward_host.is_empty() {
                    None
                } else {
                    Some(forward_host)
                },
                forward_port: Some(forward_port),
                auth_type,
            });
        }
    }

    let host_prompt = if placeholder_warning.is_some() {
        "  SSH bastion host or local SSH endpoint:"
    } else {
        "  SSH bastion host:"
    };
    let host = inquire::Text::new(host_prompt)
        .with_default(default_host)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .to_string();

    let port = inquire::Text::new("  SSH port:")
        .with_default(&conn.ssh_port.map_or("22".into(), |p| p.to_string()))
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .parse::<u16>()
        .unwrap_or(22);

    let user = inquire::Text::new("  SSH user:")
        .with_default(default_user)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .to_string();

    let (default_forward_host, default_forward_port) =
        compass_forward_target(conn).unwrap_or((String::new(), 27017));
    let forward_host = inquire::Text::new("  Database target host through bastion:")
        .with_default(&default_forward_host)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .to_string();
    let forward_port = inquire::Text::new("  Database target port through bastion:")
        .with_default(&default_forward_port.to_string())
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
        .trim()
        .parse::<u16>()
        .unwrap_or(default_forward_port);

    let auth_method =
        inquire::Select::new("  Authentication method:", vec!["Key file", "Password"])
            .with_starting_cursor(if default_auth == "PASSWORD" { 1 } else { 0 })
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    let (key_file, auth_type) = match auth_method {
        "Key file" => {
            let kf = inquire::Text::new("  SSH key file path:")
                .with_default(default_key)
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
                .trim()
                .to_string();
            (
                if kf.is_empty() { None } else { Some(kf) },
                Some("KEY".into()),
            )
        }
        _ => {
            let ssh_acct = format!("{project_name}/{env_name}/ssh");
            let pw = inquire::Password::new("  SSH password:")
                .without_confirmation()
                .prompt()
                .map_err(|e| SafeselectError::Other(format!("Failed to read SSH password: {e}")))?;
            if !pw.is_empty() {
                compose::store_password_in_keychain(&ssh_acct, &pw)?;
                print_terminal_line("  ✓ SSH password stored in Keychain");
            }
            (None, Some("PASSWORD".into()))
        }
    };

    Ok(config::SshConfig {
        enabled: true,
        bastion: None,
        host: Some(host),
        port: Some(port),
        username: Some(user),
        secret_account: if auth_type.as_deref() == Some("PASSWORD") {
            Some(format!("{project_name}/{env_name}/ssh"))
        } else {
            None
        },
        identity_file: key_file,
        known_hosts: None,
        local_host: conn
            .ssh_local_host
            .clone()
            .or_else(|| Some("localhost".to_string())),
        local_port: conn.ssh_local_port,
        forward_host: Some(forward_host),
        forward_port: Some(forward_port),
        auth_type,
    })
}

fn select_reusable_compass_ssh_config(
    repo_root: &Path,
    env_name: &str,
    conn: &crate::compass::CompassConnection,
    current_batch: &[(String, config::SshConfig)],
) -> Result<Option<config::SshConfig>> {
    let reusable = collect_reusable_ssh_configs(repo_root, env_name, current_batch)?;
    if reusable.is_empty() {
        return Ok(None);
    }

    let reuse = inquire::Confirm::new("Reuse an existing bastion configuration?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
    if !reuse {
        return Ok(None);
    }

    let options: Vec<String> = reusable
        .iter()
        .map(|(name, ssh)| {
            let host = ssh.host.as_deref().unwrap_or("unknown");
            let port = ssh.port.unwrap_or(22);
            let user = ssh.username.as_deref().unwrap_or("unknown");
            format!("{name} ({user}@{host}:{port})")
        })
        .collect();

    let selected = inquire::Select::new("  Reuse bastion:", options)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;
    let selected_index = reusable
        .iter()
        .position(|(name, ssh)| {
            let host = ssh.host.as_deref().unwrap_or("unknown");
            let port = ssh.port.unwrap_or(22);
            let user = ssh.username.as_deref().unwrap_or("unknown");
            format!("{name} ({user}@{host}:{port})") == selected
        })
        .ok_or_else(|| SafeselectError::Other("selected bastion not found".to_string()))?;

    let (bastion_name, mut ssh) = reusable[selected_index].clone();
    ssh.enabled = true;
    ssh.bastion = Some(bastion_name);
    if let Some((forward_host, forward_port)) = compass_forward_target(conn) {
        ssh.forward_host = Some(forward_host);
        ssh.forward_port = Some(forward_port);
    } else {
        let forward_host = inquire::Text::new("  Database target host through bastion:")
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
            .trim()
            .to_string();
        let forward_port = inquire::Text::new("  Database target port through bastion:")
            .with_default("27017")
            .prompt()
            .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?
            .trim()
            .parse::<u16>()
            .unwrap_or(27017);
        ssh.forward_host = Some(forward_host);
        ssh.forward_port = Some(forward_port);
    }
    ssh.local_port = None;
    if ssh.local_host.as_deref().is_none_or(str::is_empty) {
        ssh.local_host = Some("localhost".to_string());
    }
    Ok(Some(ssh))
}

fn compass_ssh_config(conn: &compass::CompassConnection) -> Option<config::SshConfig> {
    let host = conn.ssh_host.clone()?;
    let auth_type = conn.ssh_auth_type.as_deref().map(normalize_ssh_auth_type);
    let (forward_host, forward_port) = compass_forward_target(conn)
        .map(|(host, port)| (Some(host), Some(port)))
        .unwrap_or((None, None));
    if compass_uses_local_tunnel_endpoint(conn) {
        return Some(config::SshConfig {
            enabled: true,
            bastion: None,
            host: Some(host.clone()),
            port: Some(conn.ssh_port.unwrap_or(22)),
            username: conn.ssh_user.clone(),
            secret_account: None,
            identity_file: conn.ssh_key_file.clone(),
            known_hosts: None,
            local_host: Some("localhost".to_string()),
            local_port: None,
            forward_host,
            forward_port,
            auth_type,
        });
    }

    Some(config::SshConfig {
        enabled: true,
        bastion: None,
        host: Some(host),
        port: Some(conn.ssh_port.unwrap_or(22)),
        username: conn.ssh_user.clone(),
        secret_account: None,
        identity_file: conn.ssh_key_file.clone(),
        known_hosts: None,
        local_host: conn
            .ssh_local_host
            .clone()
            .or_else(|| Some("localhost".to_string())),
        local_port: conn.ssh_local_port,
        forward_host,
        forward_port,
        auth_type,
    })
}

fn compass_shared_tunnel_warning(conn: &compass::CompassConnection) -> Option<String> {
    let host = conn.ssh_host.as_deref()?.trim();
    let user = conn.ssh_user.as_deref().unwrap_or("").trim();
    let port = conn.ssh_port.unwrap_or(0);

    if (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1")
        && !user.is_empty()
        && port > 0
    {
        return Some(
            "Compass exported a local SSH endpoint (for example localhost:2222). \
Keep it if you open the Azure Bastion tunnel yourself first; otherwise replace it with the real bastion."
                .to_string(),
        );
    }

    None
}

fn compass_forward_target(conn: &crate::compass::CompassConnection) -> Option<(String, u16)> {
    mongodb_srv_forward_target(&conn.url).or_else(|| crate::extract_tcp_host_port(&conn.url))
}

fn mongodb_srv_forward_target(url: &str) -> Option<(String, u16)> {
    let srv_host = url
        .strip_prefix("mongodb+srv://")?
        .split('/')
        .next()?
        .rsplit('@')
        .next()?;
    let output = std::process::Command::new("dig")
        .args(["+short", "SRV", &format!("_mongodb._tcp.{srv_host}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_mongodb_srv_record(&String::from_utf8_lossy(&output.stdout))
}

fn parse_mongodb_srv_record(output: &str) -> Option<(String, u16)> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let _priority = parts.next()?;
        let _weight = parts.next()?;
        let port = parts.next()?.parse::<u16>().ok()?;
        let host = parts.next()?.trim_end_matches('.').to_string();
        if host.is_empty() {
            None
        } else {
            Some((host, port))
        }
    })
}

fn compass_uses_local_tunnel_endpoint(conn: &crate::compass::CompassConnection) -> bool {
    conn.ssh_host
        .as_deref()
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1")
        && conn.ssh_port.unwrap_or(0) > 0
}

fn rewrite_mongodb_url_for_local_endpoint(
    url: &str,
    local_host: &str,
    local_port: u16,
) -> Option<String> {
    let scheme_end = url.find("://")?;
    let authority_start = scheme_end + 3;
    let path_start = url[authority_start..]
        .find('/')
        .map(|idx| authority_start + idx)
        .unwrap_or(url.len());
    let authority = &url[authority_start..path_start];
    let credentials_end = authority.rfind('@').map(|idx| idx + 1).unwrap_or(0);
    let credentials = &authority[..credentials_end];
    let suffix = &url[path_start..];
    let rewritten_suffix =
        normalize_mongodb_local_tunnel_suffix(if suffix.is_empty() { "/" } else { suffix });
    Some(format!(
        "mongodb://{}{}:{}{}",
        credentials, local_host, local_port, rewritten_suffix
    ))
}

fn normalize_mongodb_local_tunnel_suffix(suffix: &str) -> String {
    let Some((path, query)) = suffix.split_once('?') else {
        return format!("{suffix}?tls=true&tlsAllowInvalidHostnames=true&directConnection=true");
    };

    let mut params: Vec<String> = query.split('&').map(|part| part.to_string()).collect();

    if !query.contains("readPreference=") && query.contains("readPreferenceTags=") {
        params.insert(0, "readPreference=secondaryPreferred".to_string());
    }
    if !query.contains("tls=") && !query.contains("ssl=") {
        params.push("tls=true".to_string());
    }
    if !query.contains("tlsAllowInvalidHostnames=") {
        params.push("tlsAllowInvalidHostnames=true".to_string());
    }
    if !query.contains("directConnection=") {
        params.push("directConnection=true".to_string());
    }

    format!("{path}?{}", params.join("&"))
}

fn default_compass_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MongoDB Compass")
}

fn slug_env_name(name: &str) -> String {
    let slug = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn unique_env_name(env_dir: &Path, base: &str) -> String {
    let base = if base.is_empty() { "mongodb" } else { base };
    let mut candidate = base.to_string();
    let mut n = 2;
    while env_dir.join(format!("{candidate}.toml")).exists() {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

fn prepare_mongodb_url(
    project_name: &str,
    env_name: &str,
    url: &str,
) -> Result<(String, String, Option<config::SecretConfig>)> {
    let Some(scheme_end) = url.find("://") else {
        return Ok((url.to_string(), String::new(), None));
    };
    let authority_start = scheme_end + 3;
    let Some(relative_at) = url[authority_start..].find('@') else {
        return Ok((url.to_string(), String::new(), None));
    };
    let at = authority_start + relative_at;
    let credentials = &url[authority_start..at];
    let Some((username, password)) = credentials.split_once(':') else {
        return Ok((url.to_string(), credentials.to_string(), None));
    };
    let account = format!("{project_name}/{env_name}");
    compose::store_password_in_keychain(&account, password)?;
    let sanitized = format!(
        "{}{}:{}{}",
        &url[..authority_start],
        username,
        "__SAFESELECT_PASSWORD__",
        &url[at..]
    );
    Ok((
        sanitized,
        username.to_string(),
        Some(config::SecretConfig {
            source: "macos-keychain".to_string(),
            service: Some("safeselect".to_string()),
            account: Some(account),
            variable: None,
        }),
    ))
}

fn inject_mongodb_password_placeholder(url: &str, username: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    let Some(relative_at) = url[authority_start..].find('@') else {
        return url.to_string();
    };
    let at = authority_start + relative_at;
    format!(
        "{}{}:{}{}",
        &url[..authority_start],
        username,
        "__SAFESELECT_PASSWORD__",
        &url[at..]
    )
}

fn display_database_target(url: &str) -> String {
    if let Some(database) = url
        .split('/')
        .next_back()
        .and_then(|segment| segment.split('?').next())
        .filter(|segment| !segment.is_empty())
    {
        return database.to_string();
    }
    "?".to_string()
}

fn setup_driver_if_missing() -> Result<()> {
    let loader = config::ConfigLoader::new();
    if !drivers_missing(&loader) {
        return Ok(());
    }
    println!();
    println!("── JDBC Driver ──────────────────────────────────");
    println!();
    cmd_driver(
        &loader,
        DriverAction::Download {
            vendor: "postgresql".into(),
        },
    )?;
    Ok(())
}

fn drivers_missing(loader: &ConfigLoader) -> bool {
    loader
        .list_drivers()
        .map(|drivers| drivers.is_empty())
        .unwrap_or(true)
}

fn setup_passwords_for_missing(repo_root: &std::path::Path, env_names: &[String]) -> Result<()> {
    for env_name in env_names {
        let env_file = repo_root
            .join(".safeselect")
            .join("environments")
            .join(format!("{env_name}.toml"));
        if !env_file.exists() {
            continue;
        }
        let content = std::fs::read_to_string(&env_file)?;
        let config = match toml::from_str::<config::EnvironmentConfig>(&content) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let needs_password = match &config.database.secret {
            Some(secret) => {
                if cfg!(target_os = "macos") {
                    let account = secret.account.as_deref().unwrap_or("");
                    let service = secret.service.as_deref().unwrap_or("");
                    std::process::Command::new("security")
                        .args(["find-generic-password", "-a", account, "-s", service, "-w"])
                        .output()
                        .map(|o| !o.status.success())
                        .unwrap_or(true)
                } else {
                    let var = secret.variable.as_deref().unwrap_or("");
                    std::env::var(var).is_err()
                }
            }
            None => true,
        };

        if !needs_password {
            println!("  ◉ Password already configured for '{env_name}'");
            continue;
        }

        println!();
        println!("── Database Password ───────────────────────────");
        println!();

        let account = config::preferred_keychain_account(repo_root, env_name, &config);
        let pw = rpassword::prompt_password(format!("  Password for '{account}': "))?;
        let pw = pw.trim().to_string();
        if pw.is_empty() {
            println!("  ⚠ Skipped (empty password).");
            continue;
        }
        compose::store_password_in_keychain(&account, &pw)?;
        println!("  ● Password stored in Keychain");

        let secret_section = format!(
            "[database.secret]\nsource = \"macos-keychain\"\nservice = \"safeselect\"\naccount = \"{account}\"\n"
        );
        let updated = if config.database.secret.is_some() {
            let mut buf = String::with_capacity(content.len());
            let mut in_secret = false;
            for line in content.lines() {
                if line.trim() == "[database.secret]" {
                    in_secret = true;
                    continue;
                }
                if in_secret && (line.trim().starts_with('[') || line.trim().is_empty()) {
                    in_secret = false;
                }
                if in_secret {
                    continue;
                }
                buf.push_str(line);
                buf.push('\n');
            }
            while buf.ends_with('\n') {
                buf.pop();
            }
            buf.push('\n');
            buf.push('\n');
            buf.push_str(&secret_section);
            buf
        } else {
            let mut c = content;
            if !c.ends_with('\n') {
                c.push('\n');
            }
            c.push('\n');
            c.push_str(&secret_section);
            c
        };
        std::fs::write(&env_file, &updated)?;
        print_terminal_line(&format!("  ✓ Updated {env_name}.toml"));
    }
    Ok(())
}

fn ssh_uses_password(ssh: &config::SshConfig) -> bool {
    ssh.auth_type.as_deref() == Some("PASSWORD")
}

fn missing_ssh_fields(ssh: &config::SshConfig) -> Vec<&'static str> {
    [
        (ssh.host.as_deref().unwrap_or("").is_empty(), "host"),
        (ssh.username.as_deref().unwrap_or("").is_empty(), "username"),
        (
            ssh.forward_host.as_deref().unwrap_or("").is_empty(),
            "forward_host",
        ),
        (ssh.forward_port.unwrap_or(0) == 0, "forward_port"),
    ]
    .into_iter()
    .filter_map(|(missing, field)| missing.then_some(field))
    .collect()
}

fn build_tunnel_ssh_args(ssh: &config::SshConfig) -> Vec<String> {
    let local_host = ssh.local_host.as_deref().unwrap_or("localhost");
    let local_port = ssh.local_port.unwrap_or(15432);
    let forward_host = ssh.forward_host.as_deref().unwrap_or("");
    let forward_port = ssh.forward_port.unwrap_or(0);
    let user = ssh.username.as_deref().unwrap_or("");
    let bastion = ssh.host.as_deref().unwrap_or("");
    let mut args = vec![
        "-o".into(),
        "ConnectTimeout=15".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-N".into(),
        "-L".into(),
        format!("{local_host}:{local_port}:{forward_host}:{forward_port}"),
        format!("{user}@{bastion}"),
    ];
    if let Some(port) = ssh.port.filter(|port| *port != 22) {
        args.extend(["-p".into(), port.to_string()]);
    }
    if let Some(identity_file) = ssh.identity_file.as_deref() {
        args.extend(["-i".into(), identity_file.to_string()]);
    }
    if let Some(known_hosts) = ssh.known_hosts.as_deref() {
        args.extend(["-o".into(), format!("UserKnownHostsFile={known_hosts}")]);
    }
    args
}

/// Try to establish SSH tunnels for environments that need one.
/// Returns PIDs of tunnels started by this call.
pub(crate) fn setup_ssh_tunnels(repo_root: &Path, env_names: &[String]) -> Result<()> {
    use std::io::Write;
    use std::time::Duration;

    // Tunnel setup can be a prerequisite for commands that reserve stdout for
    // machine-readable output (such as `posture --format json`).
    macro_rules! print {
        ($($arg:tt)*) => { eprint!($($arg)*) };
    }
    macro_rules! println {
        ($($arg:tt)*) => { eprintln!($($arg)*) };
    }

    let mut failures = vec![];

    for env_name in env_names {
        let cfg = match load_environment_config(repo_root, env_name) {
            Ok(cfg) => cfg,
            Err(_) => continue,
        };
        let ssh = match &cfg.ssh {
            Some(s) if s.enabled => s,
            _ => continue,
        };

        // Database connection target (original host:port from config)
        // SSH bastion address (where we SSH to + tunnel endpoint)
        let bastion_host = ssh.host.as_deref().unwrap_or("");
        let bastion_port = ssh.port.unwrap_or(22);

        // Step 1: Check if the SSH bastion is reachable
        let bastion_up = check_tcp_endpoint(bastion_host, bastion_port, Duration::from_secs(3));

        let tunnel_local_host = ssh.local_host.as_deref().unwrap_or("localhost");
        let tunnel_local_port = ssh.local_port.unwrap_or(15432);

        let backend_via_tunnel = match cfg.database.kind {
            crate::backend::BackendKind::Jdbc => {
                check_postgres_endpoint(tunnel_local_host, tunnel_local_port)
            }
            crate::backend::BackendKind::Document => {
                check_tcp_endpoint(tunnel_local_host, tunnel_local_port, Duration::from_secs(2))
            }
        };
        let backend_via_direct = match cfg.database.kind {
            crate::backend::BackendKind::Jdbc => extract_host_port(&cfg.database.url)
                .map(|(host, port)| check_postgres_endpoint(&host, port))
                .unwrap_or(false),
            crate::backend::BackendKind::Document => extract_tcp_host_port(&cfg.database.url)
                .map(|(host, port)| check_tcp_endpoint(&host, port, Duration::from_secs(2)))
                .unwrap_or(false),
        };

        if backend_via_direct || backend_via_tunnel {
            print!("  ◉ Database reachable ({env_name})");
            std::io::stderr().flush()?;
            continue;
        }

        if bastion_up {
            print!("  ◇ Bastion reachable but database not responding ({env_name})");
            std::io::stderr().flush()?;
        }

        let use_password = ssh_uses_password(ssh);

        // Check if we CAN establish our own tunnel (sshpass or key available)
        let can_establish = if use_password {
            std::process::Command::new("sshpass")
                .arg("--help")
                .output()
                .is_ok()
        } else {
            ssh.identity_file.is_some()
        };

        if !can_establish && !bastion_up {
            // Can't establish and no existing tunnel — inform user with timeout details
            println!("  ⚠  SSH bastion unreachable (connect timed out after 3s)");
            if !use_password && ssh.identity_file.is_none() {
                println!("  ⚠  No SSH key or password configured");
            }
            if let Some(ref identity_file) = ssh.identity_file {
                if !std::path::Path::new(identity_file).exists() {
                    println!("  ⚠  SSH identity file not found");
                }
            }
            print_manual_tunnel_hint();
            failures.push(format!(
                "{env_name}: SSH bastion unreachable and no active PostgreSQL tunnel"
            ));
            continue;
        }

        // Try to establish it
        let missing = missing_ssh_fields(ssh);
        if !missing.is_empty() {
            println!(
                "  ⚠  Incomplete SSH config for '{env_name}': missing {}",
                missing.join(", ")
            );
            std::io::stderr().flush()?;
            failures.push(format!(
                "{env_name}: incomplete SSH config, missing {}",
                missing.join(", ")
            ));
            continue;
        }

        print!("  ● Establishing SSH tunnel ({env_name}) ... ");
        std::io::stderr().flush()?;

        // Use the DBeaver-exported local endpoint when available; otherwise keep the
        // historical SafeSelect default to avoid changing existing behavior.
        let tunnel_local_host = ssh.local_host.as_deref().unwrap_or("localhost");
        let tunnel_local_port = ssh.local_port.unwrap_or(15432);

        let use_password = ssh_uses_password(ssh);

        // Use a different local port (15432) for forwarding, not the SSH server port
        // Build SSH args
        let ssh_args = build_tunnel_ssh_args(ssh);

        // Pass the password through the environment so it is not exposed in process arguments.
        let spawn_sshpass = |password: &str| -> std::io::Result<std::process::Child> {
            let mut full = vec!["-e".into(), "ssh".into()];
            full.extend(ssh_args.clone());
            std::process::Command::new("sshpass")
                .args(&full)
                .env("SSHPASS", password)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
        };

        // Helper: spawn ssh <ssh_args> with optional extra flags
        let spawn_ssh = |extra: Vec<String>| -> std::io::Result<std::process::Child> {
            let mut full = extra;
            full.extend(ssh_args.clone());
            std::process::Command::new("ssh")
                .args(&full)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
        };

        let mut child = if use_password {
            let ssh_acct = ssh
                .secret_account
                .clone()
                .unwrap_or_else(|| format!("{}/{env_name}/ssh", project_display_name(repo_root)));
            let pw = match compose::read_password_from_keychain(&ssh_acct) {
                Ok(p) => p,
                Err(_) => {
                    println!("NO PASSWORD");
                    print_manual_tunnel_hint();
                    failures.push(format!("{env_name}: SSH password not found in Keychain"));
                    continue;
                }
            };
            match spawn_sshpass(&pw) {
                Ok(c) => c,
                Err(_) => {
                    println!("sshpass is required for this password-based SSH tunnel but is not installed.");
                    println!("  macOS/Homebrew: brew install sshpass");
                    println!("  Other systems: install sshpass with your package manager.");
                    println!("  Prefer SSH key authentication when possible.");
                    println!("  Then run:  safeselect check --environment {env_name}");
                    print_manual_tunnel_hint();
                    failures.push(format!("{env_name}: sshpass not installed"));
                    continue;
                }
            }
        } else {
            let extra = vec!["-o".into(), "BatchMode=yes".into()];
            match spawn_ssh(extra) {
                Ok(c) => c,
                Err(_) => {
                    print_terminal_error_line("FAILED: unable to start SSH command");
                    println!("  Check that ssh is installed and the identity file is accessible.");
                    print_manual_tunnel_hint();
                    failures.push(format!("{env_name}: failed to start SSH command"));
                    continue;
                }
            }
        };

        // Wait briefly: if the bastion/tunnel is down, fail fast like JDBC clients do.
        let tunnel_wait = Duration::from_secs(20);
        let deadline = std::time::Instant::now() + tunnel_wait;
        let mut backend_ok = false;
        while std::time::Instant::now() < deadline {
            backend_ok = match cfg.database.kind {
                crate::backend::BackendKind::Jdbc => {
                    check_postgres_endpoint(tunnel_local_host, tunnel_local_port)
                        || extract_host_port(&cfg.database.url)
                            .map(|(host, port)| check_postgres_endpoint(&host, port))
                            .unwrap_or(false)
                }
                crate::backend::BackendKind::Document => {
                    check_tcp_endpoint(tunnel_local_host, tunnel_local_port, Duration::from_secs(2))
                        || extract_tcp_host_port(&cfg.database.url)
                            .map(|(host, port)| {
                                check_tcp_endpoint(&host, port, Duration::from_secs(2))
                            })
                            .unwrap_or(false)
                }
            };
            if backend_ok {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if backend_ok {
            print_terminal_error_line("OK");
            // Detach child so it survives after we exit
            let _ = std::thread::spawn(move || {
                let _ = child.wait_with_output();
            });
        } else {
            let _ = child.kill();
            let ssh_error = child.wait_with_output().ok().and_then(|output| {
                let detail = String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" | ");
                (!detail.is_empty()).then_some(detail)
            });
            print_terminal_error_line("FAILED");
            if ssh_error.is_some() {
                print_terminal_error_line(
                    "  SSH command failed; inspect the configured SSH connection.",
                );
            }
            println!(
                "  Database not reachable through SSH tunnel (polled for up to {}s)",
                tunnel_wait.as_secs()
            );
            println!("  Possible causes:");
            println!("    - Database connection settings are wrong");
            println!("    - Database is not running or not accepting connections");
            println!("    - SSH tunnel failed to forward (check bastion logs)");
            print_manual_tunnel_hint();
            failures.push(format!(
                "{env_name}: database not reachable through SSH tunnel after {}s",
                tunnel_wait.as_secs()
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(SafeselectError::Other(format!(
            "SSH tunnel setup failed: {}",
            failures.join("; ")
        )))
    }
}

/// Run `safeselect check` for each environment and report results.
fn run_checks(
    repo_root: &std::path::Path,
    environment: Option<&str>,
    verbose: bool,
    show_progress: bool,
) -> Result<()> {
    let env_names = selected_environment_names(repo_root, environment)?;
    run_checks_for_environments(repo_root, &env_names, verbose, show_progress, true)
}

fn run_checks_for_environments(
    repo_root: &std::path::Path,
    env_names: &[String],
    verbose: bool,
    show_progress: bool,
    fail_on_error: bool,
) -> Result<()> {
    if env_names.is_empty() {
        print_no_environments(repo_root);
        if fail_on_error {
            return Err(no_environments_error());
        }
        return Ok(());
    }
    println!("── Verification ──────────────────────────────────");
    println!();
    let mut all_ok = true;
    let mut failed_environments = Vec::new();
    for (index, env_name) in env_names.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("  • {env_name}");
        let loader = config::ConfigLoader::new();
        match cmd_check(&loader, repo_root, env_name, verbose, show_progress) {
            Ok(()) => print_terminal_line("OK"),
            Err(e) => {
                print_terminal_line("FAILED");
                print_terminal_line(&format!("    ERROR: {e}"));
                all_ok = false;
                failed_environments.push(env_name.clone());
            }
        }
    }
    if all_ok {
        println!();
        print_terminal_line("  ✓ All environments ready.");
        return Ok(());
    }
    if fail_on_error {
        return Err(SafeselectError::Other(format!(
            "checks failed for environment(s): {}",
            failed_environments.join(", ")
        )));
    }
    Ok(())
}

/// Run `safeselect reconnect` for each environment and report results.
fn run_reconnects(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    env_names: &[String],
) -> Result<()> {
    let failures: Vec<String> = env_names
        .iter()
        .filter_map(|env_name| {
            cmd_reconnect(loader, repo_root, env_name)
                .err()
                .map(|error| {
                    print_terminal_line(&format!("Reconnect failed for {env_name}: {error}"));
                    format!("{env_name}: {error}")
                })
        })
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(SafeselectError::Other(format!(
            "reconnect failed for {} environment(s): {}",
            failures.len(),
            failures.join("; ")
        )))
    }
}

fn cmd_serve_setup(_loader: &ConfigLoader, repo_root: &Path) -> Result<()> {
    tracing::info!("No .safeselect/ found — entering setup mode");

    let dirs = compose::scan_all(repo_root)?;
    let total: usize = dirs.iter().map(|(_, cs)| cs.len()).sum();

    if total == 0 {
        let msg = concat!(
            "No .safeselect/ configuration found and no PostgreSQL docker-compose services detected.\n",
            "\n",
            "To get started:\n",
            "  1. Create a docker-compose.yml with a PostgreSQL service, or\n",
            "  2. Run: safeselect import-compose [--path <dir>]\n",
            "  3. Run: safeselect serve --environment <name>\n",
        );
        tracing::info!("{}", msg);
        eprintln!("{msg}");
        return Ok(());
    }

    tracing::info!(
        "Found {} PostgreSQL service(s) in docker-compose — starting setup MCP server",
        total
    );
    eprintln!(
        "INFO: {} PostgreSQL service(s) found in docker-compose — auto-importing",
        total
    );

    let auto_import: Vec<compose::ComposeConnection> =
        dirs.into_iter().flat_map(|(_, conns)| conns).collect();

    let project_name = project_display_name(repo_root);
    let _result = compose::write_config_files(repo_root, &auto_import, &project_name)?;

    let env_names: Vec<&str> = auto_import.iter().map(|c| c.env_name.as_str()).collect();

    tracing::info!("Setup complete — starting setup MCP server");
    eprintln!(
        "INFO: .safeselect/ created. Environments: {}",
        env_names.join(", ")
    );
    eprintln!("INFO: Restart with: safeselect serve --environment <name>");

    mcp::run_setup_server(repo_root)
}

pub(crate) fn extract_host_port(url: &str) -> Option<(String, u16)> {
    let without_prefix = url.strip_prefix("jdbc:postgresql://")?;
    let host_port = without_prefix.split('/').next()?;
    let (host, port_str) = host_port.split_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    Some((host.to_string(), port))
}

pub(crate) fn extract_tcp_host_port(url: &str) -> Option<(String, u16)> {
    extract_host_port(url).or_else(|| parse_mongodb_tcp_host_port(url))
}

fn parse_mongodb_tcp_host_port(url: &str) -> Option<(String, u16)> {
    let without_prefix = url
        .strip_prefix("mongodb://")
        .or_else(|| url.strip_prefix("mongodb+srv://"))?;
    let authority = without_prefix.split('/').next()?.rsplit('@').next()?;
    let first_host = authority.split(',').next()?;
    match first_host.split_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None => Some((first_host.to_string(), 27017)),
    }
}

pub(crate) fn check_tcp_endpoint(host: &str, port: u16, timeout: std::time::Duration) -> bool {
    use std::net::ToSocketAddrs;

    format!("{host}:{port}")
        .to_socket_addrs()
        .map(|mut addrs| {
            addrs.any(|addr| std::net::TcpStream::connect_timeout(&addr, timeout).is_ok())
        })
        .unwrap_or(false)
}

pub(crate) fn check_postgres_endpoint(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;

    format!("{host}:{port}")
        .to_socket_addrs()
        .map(|addrs| addrs.into_iter().any(|addr| check_postgres(&addr)))
        .unwrap_or(false)
}

pub(crate) fn is_ssh_ready_for_query(ssh: &config::SshConfig, jdbc_url: &str) -> bool {
    let bastion_host = ssh.host.as_deref().unwrap_or("");
    let bastion_port = ssh.port.unwrap_or(22);
    if !check_tcp_endpoint(
        bastion_host,
        bastion_port,
        std::time::Duration::from_secs(3),
    ) {
        return false;
    }

    extract_host_port(jdbc_url)
        .map(|(host, port)| check_postgres_endpoint(&host, port))
        .unwrap_or(false)
}

/// Quick check if a TCP endpoint responds like a PostgreSQL server.
pub(crate) fn check_postgres(addr: &std::net::SocketAddr) -> bool {
    use std::io::{Read, Write};
    use std::time::Duration;
    let mut stream = match std::net::TcpStream::connect_timeout(addr, Duration::from_secs(3)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    // PostgreSQL SSLRequest: int32(8) + int32(80877103)
    let ssl_request: [u8; 8] = [0, 0, 0, 8, 4, 210, 22, 47];
    if stream.write_all(&ssl_request).is_err() {
        return false;
    }
    let mut resp = [0u8; 1];
    match stream.read_exact(&mut resp) {
        Ok(_) => resp[0] == b'S' || resp[0] == b'N',
        Err(_) => false,
    }
}

/// Kill any process listening on the given local port (macOS via lsof).
/// Returns true if a process was killed.
fn kill_process_on_port(port: u16) -> bool {
    let output = match std::process::Command::new("lsof")
        .args(["-ti", &format!(":{}", port)])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    kill_processes(&String::from_utf8_lossy(&output.stdout))
}

fn kill_processes(pids: &str) -> bool {
    let mut killed = false;
    for line in pids.lines() {
        let pid = match line.trim().parse::<i32>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Kill any process on this port (stale SSH tunnel, DBeaver JSch, etc.)
        let _ = std::process::Command::new("kill")
            .args([&pid.to_string()])
            .output();
        killed = true;
    }
    killed
}

fn print_manual_tunnel_hint() {
    print_terminal_error_line("  Establish the tunnel manually using the configured SSH settings.");
}

fn print_check_verbose(resolved: &config::ResolvedConfig, environment: &str) {
    println!("  · environment={environment}");
    println!("  · database=configured (details redacted)");
    if resolved.environment.database.secret.is_some() {
        println!("  · db_secret=configured");
    }
    if let Some(ssh) = resolved.environment.ssh.as_ref() {
        println!(
            "  · ssh={}",
            if ssh.enabled {
                "configured"
            } else {
                "disabled"
            }
        );
    }
}

fn cmd_check(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    verbose: bool,
    show_progress: bool,
) -> Result<()> {
    if show_progress {
        println!("Checking configuration for environment {environment}...");
    }

    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;

    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::ConfigResolved,
        "Config resolved",
    );
    if let Some(driver) = resolved.driver.as_ref() {
        diagnostics::print(
            DiagnosticStatus::Ok,
            DiagnosticCode::DriverVerified,
            format!("Driver '{}' found and checksum OK", driver.vendor),
        );
    }
    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::SecretResolved,
        "Secret resolved",
    );
    if verbose {
        print_check_verbose(&resolved, environment);
    }

    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            let bastion_host = ssh.host.as_deref().unwrap_or("unknown");
            let bastion_port = ssh.port.unwrap_or(22);
            println!("  SSH tunnel: configured");

            let mut postgres_reachable = false;
            if let Some((host, port)) = extract_host_port(&resolved.environment.database.url) {
                postgres_reachable = check_postgres_endpoint(&host, port);
                if postgres_reachable {
                    diagnostics::print(
                        DiagnosticStatus::Ok,
                        DiagnosticCode::PostgresReachable,
                        "PostgreSQL endpoint reachable",
                    );
                }
            }

            // If the local PostgreSQL endpoint is already reachable, an external
            // tunnel (for example DBeaver) is active and that is good enough.
            if !postgres_reachable {
                if check_tcp_endpoint(
                    bastion_host,
                    bastion_port,
                    std::time::Duration::from_secs(3),
                ) {
                    diagnostics::print(
                        DiagnosticStatus::Ok,
                        DiagnosticCode::SshBastionReachable,
                        "SSH bastion reachable",
                    );
                } else {
                    diagnostics::print(
                        DiagnosticStatus::Fail,
                        DiagnosticCode::SshBastionUnreachable,
                        "SSH bastion unreachable (connect timed out after 3s)",
                    );
                    if let Some(ref identity_file) = ssh.identity_file {
                        if !std::path::Path::new(identity_file).exists() {
                            diagnostics::print(
                                DiagnosticStatus::Fail,
                                DiagnosticCode::SshIdentityMissing,
                                "SSH identity file not found",
                            );
                        }
                    }
                    print_manual_tunnel_hint();
                    return Err(SafeselectError::Other(
                        "SSH bastion not reachable (connect timed out after 3s).".into(),
                    ));
                }
            }

            // 2) Establish SSH tunnel if needed, then check PostgreSQL reachability
            if let Some((host, port)) = extract_host_port(&resolved.environment.database.url) {
                // If SSH is enabled and PostgreSQL is not already reachable, try establishing the tunnel
                let (pg_reachable, tunnel_attempt_elapsed) = if postgres_reachable {
                    (true, None)
                } else {
                    let tunnel_attempt_started = std::time::Instant::now();
                    if show_progress {
                        diagnostics::print(
                            DiagnosticStatus::Info,
                            DiagnosticCode::SshTunnelAttempt,
                            "Establishing SSH tunnel...",
                        );
                    }
                    let _ = setup_ssh_tunnels(repo_root, &[environment.to_string()]);
                    let reachable = check_postgres_endpoint(&host, port);
                    (reachable, Some(tunnel_attempt_started.elapsed()))
                };

                match pg_reachable {
                    true => diagnostics::print(
                        DiagnosticStatus::Ok,
                        DiagnosticCode::PostgresReachable,
                        "PostgreSQL endpoint reachable",
                    ),
                    _ => {
                        diagnostics::print(
                            DiagnosticStatus::Fail,
                            DiagnosticCode::PostgresUnreachable,
                            "PostgreSQL endpoint unreachable",
                        );
                        println!("  Possible causes:");
                        println!("    - Database connection settings are wrong");
                        println!("    - Database is not running or not accepting connections");
                        println!("    - SSH tunnel is not established or not forwarding correctly");
                        print_manual_tunnel_hint();
                        let elapsed = tunnel_attempt_elapsed.unwrap_or_default();
                        return Err(SafeselectError::Other(ssh_tunnel_failure_message(elapsed)));
                    }
                }
            }

            if resolved.environment.database.kind == crate::backend::BackendKind::Document {
                let Some((host, port)) = extract_tcp_host_port(&resolved.environment.database.url)
                else {
                    return Err(SafeselectError::Other(
                        "Cannot determine document database endpoint from URL".into(),
                    ));
                };
                let document_reachable =
                    check_tcp_endpoint(&host, port, std::time::Duration::from_secs(3));
                let document_reachable = if document_reachable {
                    true
                } else {
                    if show_progress {
                        diagnostics::print(
                            DiagnosticStatus::Info,
                            DiagnosticCode::SshTunnelAttempt,
                            "Establishing SSH tunnel...",
                        );
                    }
                    let _ = setup_ssh_tunnels(repo_root, &[environment.to_string()]);
                    check_tcp_endpoint(&host, port, std::time::Duration::from_secs(3))
                };
                if !document_reachable {
                    diagnostics::print(
                        DiagnosticStatus::Fail,
                        DiagnosticCode::SshTunnelFailed,
                        "Document database tunnel not reachable",
                    );
                    print_manual_tunnel_hint();
                    return Err(SafeselectError::Other(
                        "Cannot reach document database through SSH tunnel.".into(),
                    ));
                }
            }
        }
    }

    if show_progress {
        diagnostics::print(
            DiagnosticStatus::Info,
            DiagnosticCode::SidecarStartAttempt,
            "Attempting sidecar connection...",
        );
        println!("    connection parameters loaded (redacted)");
    }

    let limits = ResultLimits {
        max_rows: resolved.project.limits.max_rows,
        max_result_bytes: resolved.project.limits.max_result_bytes,
    };
    match resolved.environment.database.kind {
        crate::backend::BackendKind::Jdbc => {
            let driver = resolved.driver.as_ref().ok_or_else(|| {
                SafeselectError::Config("missing JDBC driver configuration".into())
            })?;
            let mut sidecar = SidecarProcess::start_with_timeout(
                &driver.path,
                &driver.class,
                &resolved.environment.database.url,
                &resolved.environment.database.username,
                &resolved.password,
                0,
                resolved.project.limits.statement_timeout_ms,
                limits,
                false,
            )
            .map_err(redact_connection_start_error)?;

            sidecar.ping()?;
            diagnostics::print(
                DiagnosticStatus::Ok,
                DiagnosticCode::SidecarBackendOk,
                "Sidecar JDBC connection OK",
            );

            let result = sidecar.execute("SELECT 1 AS connection_test")?;
            diagnostics::print(
                DiagnosticStatus::Ok,
                DiagnosticCode::BackendVerificationOk,
                format!(
                    "Connection verified: SELECT 1 returned {} row(s)",
                    result.row_count
                ),
            );
            sidecar.shutdown()?;
        }
        crate::backend::BackendKind::Document => {
            let mut sidecar = SidecarProcess::start_document_with_timeout(
                resolved.environment.database.vendor(),
                &resolved.environment.database.url,
                &resolved.environment.database.username,
                &resolved.password,
                0,
                resolved.project.limits.statement_timeout_ms,
                limits,
                false,
            )
            .map_err(redact_connection_start_error)?;

            sidecar.ping()?;
            diagnostics::print(
                DiagnosticStatus::Ok,
                DiagnosticCode::SidecarBackendOk,
                "Sidecar document connection OK",
            );

            sidecar.verify_document_connection()?;
            diagnostics::print(
                DiagnosticStatus::Ok,
                DiagnosticCode::BackendVerificationOk,
                "Connection verified: MongoDB ping succeeded",
            );
            sidecar.shutdown()?;
        }
    }
    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::AllChecksPassed,
        format!("All checks passed for environment {environment}"),
    );

    Ok(())
}

fn ssh_tunnel_failure_message(elapsed: std::time::Duration) -> String {
    format!(
        "Cannot reach PostgreSQL through SSH tunnel after {} (the final PostgreSQL probe timed out after 2s).",
        format_elapsed(elapsed.as_millis() as u64)
    )
}

fn cmd_query(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    sql: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;

    let sql = match sql {
        Some(s) => s.to_string(),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            let trimmed = buf.trim().to_string();
            if trimmed.is_empty() {
                return Err(SafeselectError::Other(
                    "No SQL provided. Use --sql or pipe a query.".into(),
                ));
            }
            trimmed
        }
    };

    let security = security::SecurityEngine::new(
        resolved.project.security.clone(),
        resolved.project.limits.clone(),
    );
    security.validate(&sql)?;

    if resolved
        .environment
        .ssh
        .as_ref()
        .is_some_and(|ssh| ssh.enabled)
    {
        setup_ssh_tunnels(repo_root, &[environment.to_string()])?;
    }

    let driver = resolved.driver.as_ref().ok_or_else(|| {
        SafeselectError::Config("query currently supports only JDBC environments".into())
    })?;
    let mut sidecar = SidecarProcess::start_with_timeout(
        &driver.path,
        &driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
        0,
        resolved.project.limits.statement_timeout_ms,
        ResultLimits {
            max_rows: resolved.project.limits.max_rows,
            max_result_bytes: resolved.project.limits.max_result_bytes,
        },
        verbose,
    )
    .map_err(redact_connection_start_error)?;

    let result = match sidecar.execute(&sql) {
        Ok(result) => result,
        Err(SafeselectError::SqlError(message)) | Err(SafeselectError::Sidecar(message)) => {
            eprintln!("ERROR: SQL query failed: {message}");
            return Err(SafeselectError::Sidecar(message));
        }
        Err(error) => return Err(error),
    };
    security.check_result_size(result.row_count, result.byte_count)?;

    sidecar.shutdown()?;

    if result.columns.is_empty() {
        println!(
            "Read completed. {} rows returned. ({})",
            result.row_count,
            format_elapsed(result.elapsed_ms)
        );
        return Ok(());
    }

    let col_widths: Vec<usize> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let max_data = result
                .rows
                .iter()
                .filter_map(|row| row.get(i))
                .filter_map(|v| v.as_str())
                .map(|s| s.len())
                .max()
                .unwrap_or(0);
            col.len().max(max_data).min(80)
        })
        .collect();

    let print_row = |cells: &[String]| {
        let parts: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let width = col_widths.get(i).copied().unwrap_or(20);
                format!(" {:width$} ", cell, width = width)
            })
            .collect();
        println!("|{}|", parts.join("|"));
    };

    let separator = || {
        let parts: Vec<String> = col_widths
            .iter()
            .map(|w| format!("-{:-<width$}-", "", width = w))
            .collect();
        println!("|{}|", parts.join("+"));
    };

    separator();
    print_row(&result.columns);
    separator();
    for row in &result.rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "NULL".into(),
                    other => other.to_string(),
                };
                let width = col_widths.get(i).copied().unwrap_or(20);
                if s.len() > width {
                    format!("{}…", &s[..width.saturating_sub(1)])
                } else {
                    s
                }
            })
            .collect();
        print_row(&cells);
    }
    separator();
    println!(
        "({} rows, {} bytes, {})",
        result.row_count,
        result.byte_count,
        format_elapsed(result.elapsed_ms)
    );

    Ok(())
}

fn cmd_posture(
    loader: &ConfigLoader,
    repo_root: &Path,
    environments: &[String],
    format: &str,
    strict: bool,
    acknowledge: bool,
    skip_unsupported: bool,
) -> Result<()> {
    validate_posture_format(format)?;
    let collection = collect_posture_reports(
        loader,
        repo_root,
        environments,
        acknowledge,
        skip_unsupported,
    )?;
    render_posture_collection(format, &collection, skip_unsupported)?;
    enforce_posture_strict(strict, &collection.reports)
}

fn render_posture_collection(
    format: &str,
    collection: &PostureCollection<'_>,
    aggregate: bool,
) -> Result<()> {
    if collection.reports.is_empty() {
        return render_empty_posture_collection(format, &collection.failures);
    }
    print_posture_reports(format, &collection.reports, aggregate)?;
    render_posture_failures(format, &collection.failures)
}

fn render_posture_failures(format: &str, failures: &[(&String, String)]) -> Result<()> {
    if failures.is_empty() {
        if format == "text" {
            println!();
            print_terminal_line("  ✓ All environments inspected.");
        }
        return Ok(());
    }
    if format == "text" {
        print_posture_failures_text(failures, true);
    }
    Err(posture_failures_error(failures))
}

fn render_empty_posture_collection(format: &str, failures: &[(&String, String)]) -> Result<()> {
    if failures.is_empty() {
        return Err(SafeselectError::Config(
            "no PostgreSQL environments available for posture inspection".into(),
        ));
    }
    if format == "text" {
        print_posture_header();
        print_posture_failures_text(failures, false);
    }
    Err(posture_failures_error(failures))
}

type EnvironmentPostureReport<'a> = (&'a String, posture::Report);

struct PostureCollection<'a> {
    reports: Vec<EnvironmentPostureReport<'a>>,
    failures: Vec<(&'a String, String)>,
}

fn posture_failures_error(failures: &[(&String, String)]) -> SafeselectError {
    let environments = failures
        .iter()
        .map(|(environment, error)| format!("{environment}: {error}"))
        .collect::<Vec<_>>()
        .join(", ");
    SafeselectError::Other(format!(
        "posture inspection failed for environment(s): {environments}"
    ))
}

fn validate_posture_format(format: &str) -> Result<()> {
    match format {
        "text" | "json" => Ok(()),
        _ => Err(SafeselectError::Other(
            "--format must be text or json".into(),
        )),
    }
}

fn collect_posture_reports<'a>(
    loader: &ConfigLoader,
    repo_root: &Path,
    environments: &'a [String],
    acknowledge: bool,
    skip_unsupported: bool,
) -> Result<PostureCollection<'a>> {
    let mut tunnel_endpoints = Vec::new();
    let mut reports = Vec::new();
    let mut failures = Vec::new();

    for environment in environments {
        let outcome = inspect_posture_environment(
            loader,
            repo_root,
            environment,
            acknowledge,
            skip_unsupported,
            &mut tunnel_endpoints,
        );
        match outcome {
            Ok(Some(report)) => reports.push(report),
            Ok(None) => {}
            Err(error) => failures.push((
                environment,
                redact_connection_start_error(error).to_string(),
            )),
        }
    }

    Ok(PostureCollection { reports, failures })
}

fn inspect_posture_environment<'a>(
    loader: &ConfigLoader,
    repo_root: &Path,
    environment: &'a String,
    acknowledge: bool,
    skip_unsupported: bool,
    tunnel_endpoints: &mut Vec<((String, u16), &'a String)>,
) -> Result<Option<EnvironmentPostureReport<'a>>> {
    if posture_environment_is_unsupported(repo_root, environment, skip_unsupported)? {
        return Ok(None);
    }
    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;
    prepare_posture_tunnel(repo_root, environment, &resolved, tunnel_endpoints)?;
    let report = posture::inspect(&resolved, loader.config_dir())?;
    acknowledge_posture_report(loader, &report, acknowledge)?;
    Ok(Some((environment, report)))
}

fn acknowledge_posture_report(
    loader: &ConfigLoader,
    report: &posture::Report,
    acknowledge: bool,
) -> Result<()> {
    if acknowledge && report.status == "warning" {
        posture::acknowledge(loader.config_dir(), &report.fingerprint)?;
    }
    Ok(())
}

fn posture_environment_is_unsupported(
    repo_root: &Path,
    environment: &str,
    skip_unsupported: bool,
) -> Result<bool> {
    let environment_config =
        load_environment_config(repo_root, environment).map_err(redact_resolution_error)?;
    Ok(skip_unsupported && !supports_posture(&environment_config))
}

fn no_environments_error() -> SafeselectError {
    SafeselectError::Config(
        "No environment configurations found. Create or import an environment before retrying."
            .into(),
    )
}

fn prepare_posture_tunnel<'a>(
    repo_root: &Path,
    environment: &'a String,
    resolved: &config::ResolvedConfig,
    tunnel_endpoints: &mut Vec<((String, u16), &'a String)>,
) -> Result<()> {
    let Some(endpoint) = posture_tunnel_endpoint(resolved) else {
        return Ok(());
    };
    if let Some((_, other)) = tunnel_endpoints
        .iter()
        .find(|(existing, _)| tunnel_endpoints_overlap(existing, &endpoint))
    {
        return Err(SafeselectError::Config(format!(
            "posture cannot inspect '{environment}' and '{other}' because their SSH local endpoints overlap"
        )));
    }
    // Posture uses its own short-lived sidecar, so it cannot rely on a tunnel
    // established by an earlier `check` or `reconnect` command.
    setup_ssh_tunnels(repo_root, std::slice::from_ref(environment))?;
    tunnel_endpoints.push((endpoint, environment));
    Ok(())
}

fn posture_tunnel_endpoint(resolved: &config::ResolvedConfig) -> Option<(String, u16)> {
    let ssh = resolved
        .environment
        .ssh
        .as_ref()
        .filter(|ssh| ssh.enabled)?;
    let endpoint = (
        canonical_tunnel_host(ssh.local_host.as_deref().unwrap_or("localhost")),
        ssh.local_port.unwrap_or(15432),
    );
    let url_uses_tunnel_endpoint = postgres_jdbc_host_ports(&resolved.environment.database.url)
        .into_iter()
        .map(|(host, port)| (canonical_tunnel_host(&host), port))
        .any(|url_endpoint| tunnel_endpoints_overlap(&url_endpoint, &endpoint));
    if !url_uses_tunnel_endpoint && direct_postgres_reachable(&resolved.environment.database.url) {
        // SSH is configured, but the database URL is directly reachable (for
        // example through a VPN), so no local tunnel endpoint is consumed.
        return None;
    }
    Some(endpoint)
}

fn direct_postgres_reachable(url: &str) -> bool {
    postgres_jdbc_host_ports(url)
        .into_iter()
        .any(|(host, port)| check_postgres_endpoint(&host, port))
}

fn postgres_jdbc_host_ports(url: &str) -> Vec<(String, u16)> {
    postgres_jdbc_authority(url)
        .map(|authority| {
            authority
                .split(',')
                .filter_map(|endpoint| parse_postgres_jdbc_authority(endpoint.trim()))
                .collect()
        })
        .unwrap_or_default()
}

fn postgres_jdbc_authority(url: &str) -> Option<&str> {
    url.strip_prefix("jdbc:postgresql://")?
        .split(['/', '?', '#'])
        .next()?
        .rsplit('@')
        .next()
}

fn parse_postgres_jdbc_authority(authority: &str) -> Option<(String, u16)> {
    if let Some(bracketed_host) = authority.strip_prefix('[') {
        return parse_postgres_ipv6_authority(bracketed_host);
    }
    match authority.split_once(':') {
        Some((host, port)) => Some((host.into(), port.parse().ok()?)),
        None if !authority.is_empty() => Some((authority.into(), 5432)),
        None => None,
    }
}

fn parse_postgres_ipv6_authority(authority: &str) -> Option<(String, u16)> {
    let (host, remainder) = authority.split_once(']')?;
    let port = match remainder {
        "" => 5432,
        _ => remainder.strip_prefix(':')?.parse().ok()?,
    };
    Some((host.into(), port))
}

fn tunnel_endpoints_overlap(first: &(String, u16), second: &(String, u16)) -> bool {
    first.1 == second.1 && tunnel_hosts_overlap(&first.0, &second.0)
}

fn tunnel_hosts_overlap(first: &str, second: &str) -> bool {
    if first == "wildcard" || second == "wildcard" || first == second {
        return true;
    }
    let first_addresses = tunnel_host_addresses(first);
    let second_addresses = tunnel_host_addresses(second);
    first_addresses
        .iter()
        .any(|address| second_addresses.contains(address))
}

fn tunnel_host_addresses(host: &str) -> Vec<std::net::IpAddr> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

    if host == "local" {
        return vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ];
    }
    let mut addresses: Vec<IpAddr> = (host, 0)
        .to_socket_addrs()
        .map(|addresses| addresses.map(|address| address.ip()).collect())
        .unwrap_or_default();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

fn canonical_tunnel_host(host: &str) -> String {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized == "localhost" {
        "local".into()
    } else if let Ok(address) = normalized.parse::<std::net::IpAddr>() {
        if address.is_unspecified() {
            "wildcard".into()
        } else if address.is_loopback() {
            "local".into()
        } else {
            address.to_string()
        }
    } else {
        canonical_numeric_tunnel_host(&normalized)
    }
}

fn canonical_numeric_tunnel_host(host: &str) -> String {
    use std::net::ToSocketAddrs;

    if !host
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
    {
        return host.into();
    }
    match format!("{host}:0")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
    {
        Some(address) if address.ip().is_unspecified() => "wildcard".into(),
        Some(address) if address.ip().is_loopback() => "local".into(),
        Some(address) => address.ip().to_string(),
        None => host.into(),
    }
}

fn supports_posture(environment: &config::EnvironmentConfig) -> bool {
    environment.database.kind == crate::backend::BackendKind::Jdbc
        && matches!(
            environment.database.vendor().to_ascii_lowercase().as_str(),
            "postgresql" | "postgres"
        )
}

fn print_posture_reports(
    format: &str,
    reports: &[EnvironmentPostureReport<'_>],
    aggregate: bool,
) -> Result<()> {
    match format {
        "json" => print_posture_json(reports, aggregate),
        "text" => print_posture_text(reports, aggregate),
        _ => unreachable!("format validated before rendering"),
    }
}

fn print_posture_json(reports: &[EnvironmentPostureReport<'_>], aggregate: bool) -> Result<()> {
    let payload = if !aggregate && reports.len() == 1 {
        serde_json::to_string_pretty(&reports[0].1)
    } else {
        serde_json::to_string_pretty(
            &reports
                .iter()
                .map(|(environment, report)| {
                    serde_json::json!({
                        "environment": environment,
                        "report": report,
                    })
                })
                .collect::<Vec<_>>(),
        )
    };
    println!(
        "{}",
        payload.map_err(|error| SafeselectError::Other(error.to_string()))?
    );
    Ok(())
}

fn print_posture_text(reports: &[EnvironmentPostureReport<'_>], _aggregate: bool) -> Result<()> {
    print_posture_header();
    for (index, (environment, report)) in reports.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("  • {environment}");
        println!("Checking PostgreSQL posture for {environment}...");
        println!("PostgreSQL posture: {}", report.status);
        println!("Role: {}  Database: {}", report.role, report.database);
        for finding in &report.findings {
            println!("- [{}] {}", finding.severity, finding.message);
        }
        print_terminal_line(posture_completion_marker(report.status));
    }
    Ok(())
}

fn posture_completion_marker(status: &str) -> &str {
    match status {
        "safe" | "accepted" => "OK",
        "warning" => "WARNING",
        "unsafe" => "UNSAFE",
        _ => "INSPECTED",
    }
}

fn print_posture_header() {
    println!("── PostgreSQL posture ────────────────────────────");
    println!();
}

fn print_posture_failures_text(failures: &[(&String, String)], has_reports: bool) {
    for (index, (environment, error)) in failures.iter().enumerate() {
        if has_reports || index > 0 {
            println!();
        }
        println!("  • {environment}");
        println!("Checking PostgreSQL posture for {environment}...");
        print_terminal_line("FAILED");
        print_terminal_line(&format!("    ERROR: {error}"));
    }
}

fn enforce_posture_strict(strict: bool, reports: &[EnvironmentPostureReport<'_>]) -> Result<()> {
    if strict && reports.iter().any(|(_, report)| report.status == "unsafe") {
        return Err(SafeselectError::Other("security posture is unsafe".into()));
    }
    Ok(())
}

fn cmd_connectivity_action(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    action: &str,
) -> Result<()> {
    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;

    let driver = resolved.driver.as_ref().ok_or_else(|| {
        SafeselectError::Config(
            "connectivity actions currently support only JDBC environments".into(),
        )
    })?;
    let mut sidecar = SidecarProcess::start_with_timeout(
        &driver.path,
        &driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
        0,
        resolved.project.limits.statement_timeout_ms,
        ResultLimits {
            max_rows: resolved.project.limits.max_rows,
            max_result_bytes: resolved.project.limits.max_result_bytes,
        },
        false,
    )
    .map_err(redact_connection_start_error)?;

    match action {
        "disconnect" => {
            sidecar.disconnect()?;
            println!("Disconnected from environment {environment}.");
            println!("  The AI agent can reconnect via the 'connect' MCP tool.");
        }
        "connect" => {
            sidecar.connect()?;
            println!("Connected to environment {environment}.");
        }
        _ => unreachable!(),
    }

    sidecar.shutdown()?;
    Ok(())
}

fn cmd_reconnect(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
) -> Result<()> {
    println!("Reconnecting to environment {environment}...");

    let resolved = resolve_local_for_cli(loader, repo_root, environment)?;

    // Establish SSH tunnel if configured
    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            println!("  ◇ Establishing SSH tunnel...");
            setup_ssh_tunnels(repo_root, &[environment.to_string()])?;
        }
    }

    let limits = ResultLimits {
        max_rows: resolved.project.limits.max_rows,
        max_result_bytes: resolved.project.limits.max_result_bytes,
    };
    let mut sidecar = match resolved.environment.database.kind {
        crate::backend::BackendKind::Jdbc => {
            let driver = resolved.driver.as_ref().ok_or_else(|| {
                SafeselectError::Config(
                    "reconnect requires a JDBC driver for JDBC environments".into(),
                )
            })?;
            SidecarProcess::start_with_timeout(
                &driver.path,
                &driver.class,
                &resolved.environment.database.url,
                &resolved.environment.database.username,
                &resolved.password,
                0,
                resolved.project.limits.statement_timeout_ms,
                limits,
                false,
            )
            .map_err(redact_connection_start_error)?
        }
        crate::backend::BackendKind::Document => SidecarProcess::start_document_with_timeout(
            resolved.environment.database.vendor(),
            &resolved.environment.database.url,
            &resolved.environment.database.username,
            &resolved.password,
            0,
            resolved.project.limits.statement_timeout_ms,
            limits,
            false,
        )
        .map_err(redact_connection_start_error)?,
    };

    sidecar.ping()?;
    print_terminal_line("  ✓ Sidecar started and pinged");

    match resolved.environment.database.kind {
        crate::backend::BackendKind::Jdbc => {
            let result = sidecar.execute("SELECT 1 AS connection_test")?;
            print_terminal_line(&format!(
                "  ✓ Connection verified: SELECT 1 returned {} row(s)",
                result.row_count
            ));
        }
        crate::backend::BackendKind::Document => {
            sidecar.verify_document_connection()?;
            print_terminal_line("  ✓ Connection verified: MongoDB ping succeeded");
        }
    }

    sidecar.shutdown()?;
    print_terminal_line(&format!(
        "  ✓ Reconnection successful to environment {environment}"
    ));

    Ok(())
}

fn cmd_uninstall(force: bool, binary_only: bool) -> Result<()> {
    if !force {
        if binary_only {
            println!(
                "This will remove only safeselect binaries from ~/.local/bin and ~/.cargo/bin."
            );
        } else {
            println!("This will remove: safeselect binary, global config, data, audit logs, and keychain entries.");
            println!("Local .safeselect/ directories in repos will NOT be removed.");
        }
        print!("Continue? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => {}
            _ => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    }

    let mut removed_anything = false;

    for path in uninstall_binary_paths() {
        if path.exists() {
            std::fs::remove_file(&path)?;
            print_terminal_line(&format!("  ✓ Removed {}", path.display()));
            removed_anything = true;
        }
    }

    if binary_only {
        if !removed_anything {
            println!("  No user-local safeselect binaries found.");
        }
        println!("  Binary uninstall complete. Configuration was preserved.");
        return Ok(());
    }

    let config_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        std::path::PathBuf::from(home).join(".config/safeselect")
    };
    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)?;
        print_terminal_line(&format!("  ✓ Removed {}", config_dir.display()));
        removed_anything = true;
    }

    if let Some(data_dir) = dirs::data_dir().map(|d| d.join("safeselect")) {
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir)?;
            print_terminal_line(&format!("  ✓ Removed {}", data_dir.display()));
            removed_anything = true;
        }
    }

    let audit_dir = dirs::home_dir().map(|h| h.join(".local").join("state").join("safeselect"));
    if let Some(ref path) = audit_dir {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
            print_terminal_line(&format!("  ✓ Removed {}", path.display()));
            removed_anything = true;
        }
    }

    let backup_paths = [
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support/opencode/opencode.json.safeselect.bak")),
        dirs::config_dir().map(|d| d.join("opencode/opencode.json.safeselect.bak")),
        Some(std::path::PathBuf::from(
            "~/.config/opencode/opencode.json.safeselect.bak",
        )),
    ];
    for path in backup_paths.into_iter().flatten() {
        if path.exists() {
            std::fs::remove_file(&path)?;
            print_terminal_line(&format!("  ✓ Removed backup {}", path.display()));
        }
    }

    let keychain_result = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", "safeselect"])
        .output();
    if let Ok(output) = keychain_result {
        if output.status.success() {
            print_terminal_line("  ✓ Removed macOS Keychain entries for 'safeselect'");
            removed_anything = true;
        }
    }

    let agent_configs = [
        dirs::config_dir().map(|d| d.join("opencode").join("opencode.json")),
        Some(std::path::PathBuf::from("~/.cursor/config.json")),
        Some(std::path::PathBuf::from("~/.windsurf/config.json")),
    ];
    for config in agent_configs.into_iter().flatten() {
        if config.exists() {
            if let Ok(content) = std::fs::read_to_string(&config) {
                if content.contains("safeselect") {
                    println!(
                        "  ⚠  Remove safeselect entries from {} manually",
                        config.display()
                    );
                }
            }
        }
    }

    if !removed_anything {
        println!("  Nothing to remove.");
    }

    println!("  Uninstall complete.");
    Ok(())
}

pub(crate) fn uninstall_binary_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![
        home.join(".local/bin/safeselect"),
        home.join(".cargo/bin/safeselect"),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminal_checks_are_green_only_when_color_is_enabled() {
        let line = "  ✓ opencode: safe";
        assert_eq!(
            super::terminal_line(line, true),
            "  \x1b[32m✓\x1b[0m opencode: safe"
        );
        assert_eq!(super::terminal_line(line, false), line);
        assert_eq!(
            super::terminal_line("  ✓ One ✓ Two", true),
            "  \x1b[32m✓\x1b[0m One \x1b[32m✓\x1b[0m Two"
        );
        assert_eq!(
            super::terminal_line("FAILED: connection refused", true),
            "\x1b[31mFAILED: connection refused\x1b[0m"
        );
        assert_eq!(super::terminal_line("OK", true), "\x1b[32mOK\x1b[0m");
        assert_eq!(
            super::terminal_line("    Sidecar error: connection failed", true),
            "\x1b[31m    Sidecar error: connection failed\x1b[0m"
        );
        assert_eq!(
            super::terminal_line("    ERROR: connection failed", true),
            "\x1b[31m    ERROR: connection failed\x1b[0m"
        );
        for line in ["  ⚠ copilot config could not be inspected", "  ✗ cursor"] {
            assert_eq!(super::terminal_line(line, true), line);
        }
    }

    #[test]
    fn redacts_sensitive_configuration_resolution_errors() {
        let secret = super::redact_resolution_error(super::SafeselectError::EnvVarNotSet(
            "DATABASE_PASSWORD".into(),
        ));
        assert_eq!(secret.to_string(), "Required secret could not be resolved.");
        assert!(!secret.to_string().contains("DATABASE_PASSWORD"));

        let config = super::redact_resolution_error(super::SafeselectError::Config(
            "invalid configuration in /tmp/project/.safeselect/environments/dev.toml".into(),
        ));
        assert_eq!(config.to_string(), "Configuration could not be resolved.");
        assert!(!config.to_string().contains(".safeselect"));

        let environment =
            super::redact_resolution_error(super::SafeselectError::EnvironmentNotFound(
                "production".into(),
                "/tmp/project/.safeselect/environments".into(),
            ));
        assert_eq!(
            environment.to_string(),
            "Requested environment configuration was not found."
        );
        assert!(!environment.to_string().contains(".safeselect"));

        let driver = super::redact_resolution_error(super::SafeselectError::DriverFileNotFound(
            std::path::PathBuf::from("/tmp/project/.safeselect/drivers/postgresql.jar"),
        ));
        assert_eq!(
            driver.to_string(),
            "Configured driver file is unavailable or unsafe."
        );
        assert!(!driver.to_string().contains("postgresql.jar"));

        let permissions =
            super::redact_resolution_error(super::SafeselectError::InsecurePermissions(
                std::path::PathBuf::from("/tmp/project/.safeselect/drivers/postgresql.jar"),
            ));
        assert_eq!(
            permissions.to_string(),
            "Configured driver file is unavailable or unsafe."
        );
        assert!(!permissions.to_string().contains("postgresql.jar"));
    }

    #[test]
    fn redacts_project_lookup_and_connection_start_errors() {
        let project = super::redact_cli_error(&super::SafeselectError::LocalProjectNotFound(
            std::path::PathBuf::from("/tmp/private-project"),
        ));
        assert_eq!(
            project,
            "Local SafeSelect project not found. Use --project or run from a project directory."
        );
        assert!(!project.contains("private-project"));

        let connection = super::redact_connection_start_error(super::SafeselectError::Sidecar(
            "startup failed for jdbc:postgresql://db.internal/app".into(),
        ));
        assert_eq!(
            connection.to_string(),
            "Database connection could not be started. Check the connection configuration and driver availability."
        );
        assert!(!connection.to_string().contains("db.internal"));

        let unsupported = super::redact_cli_error(&super::SafeselectError::Config(
            "connectivity actions currently support only JDBC environments".into(),
        ));
        assert_eq!(
            unsupported,
            "Config error: connectivity actions currently support only JDBC environments"
        );

        let audit = super::redact_audit_initialization_error(super::SafeselectError::Audit(
            "cannot create audit file /private/project/audit/project/dev/log.jsonl: permission denied".into(),
        ));
        assert_eq!(
            audit.to_string(),
            "Audit logging could not be initialized. Check the audit configuration and permissions."
        );
        assert!(!audit.to_string().contains("/private/project"));
    }

    #[test]
    fn doctor_fails_when_no_environments_are_available() {
        let root =
            std::env::temp_dir().join(format!("safeselect-doctor-empty-{}", uuid::Uuid::new_v4()));
        let env_dir = root.join(".safeselect/environments");
        std::fs::create_dir_all(&env_dir).unwrap();

        let check_error = run_checks_for_environments(&root, &[], false, false, true).unwrap_err();
        assert_eq!(
            check_error.to_string(),
            "Config error: No environment configurations found. Create or import an environment before retrying."
        );
        assert!(!check_error
            .to_string()
            .contains(root.to_string_lossy().as_ref()));

        let validation_error =
            validate_all_environment_configs(&super::ConfigLoader::new(), &root).unwrap_err();
        assert_eq!(validation_error.to_string(), check_error.to_string());
        assert!(!validation_error
            .to_string()
            .contains(root.to_string_lossy().as_ref()));

        assert!(run_checks_for_environments(&root, &[], false, false, false).is_ok());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn redacts_posture_environment_load_failures() {
        let root = std::env::temp_dir().join(format!(
            "safeselect-posture-invalid-environment-{}",
            uuid::Uuid::new_v4()
        ));
        let env_dir = root.join(".safeselect/environments");
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(env_dir.join("broken.toml"), "not valid TOML = [").unwrap();

        let error = posture_environment_is_unsupported(&root, "broken", true).unwrap_err();
        assert_eq!(error.to_string(), "Configuration could not be resolved.");
        assert!(!error.to_string().contains(root.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reports_ssh_tunnel_duration_without_endpoint_details() {
        let message = ssh_tunnel_failure_message(std::time::Duration::from_secs(20));
        assert!(message.contains("after 20.0s"));
        assert!(message.contains("final PostgreSQL probe timed out after 2s"));
        assert!(!message.contains("localhost"));
        assert!(!message.contains("15432"));
    }

    #[test]
    fn renders_single_and_multiple_posture_reports() {
        let report = || posture::Report {
            version: 1,
            backend: "postgresql",
            role: "reader".into(),
            database: "app".into(),
            status: "safe",
            findings: vec![],
            fingerprint: "test".into(),
            acknowledged: false,
        };
        let first = "dev".to_string();
        let second = "prod".to_string();
        let single = vec![(&first, report())];
        let multiple = vec![(&first, report()), (&second, report())];

        assert!(validate_posture_format("text").is_ok());
        assert!(validate_posture_format("json").is_ok());
        assert!(validate_posture_format("yaml").is_err());
        assert!(print_posture_reports("text", &single, false).is_ok());
        assert!(print_posture_reports("json", &single, false).is_ok());
        assert!(print_posture_reports("text", &multiple, true).is_ok());
        assert!(print_posture_reports("json", &multiple, true).is_ok());
        assert!(enforce_posture_strict(false, &single).is_ok());

        let loader = ConfigLoader::new();
        let environments = Vec::new();
        assert!(cmd_posture(
            &loader,
            Path::new("."),
            &environments,
            "text",
            false,
            false,
            true,
        )
        .is_err());
        assert!(cmd_posture(
            &loader,
            Path::new("."),
            &environments,
            "json",
            false,
            false,
            true,
        )
        .is_err());
    }

    #[test]
    fn posture_failure_labels_each_environment() {
        let first = "pre".to_string();
        let second = "pro".to_string();
        let error = posture_failures_error(&[
            (&first, "connection refused".into()),
            (&second, "SSH bastion unreachable".into()),
        ]);
        let message = error.to_string();

        assert_eq!(
            message,
            "posture inspection failed for environment(s): pre: connection refused, pro: SSH bastion unreachable"
        );
    }

    #[test]
    fn posture_collection_continues_after_skipped_and_invalid_environments() {
        let root = std::env::temp_dir().join(format!(
            "safeselect-posture-collection-{}",
            uuid::Uuid::new_v4()
        ));
        let environments_dir = root.join(".safeselect/environments");
        std::fs::create_dir_all(&environments_dir).unwrap();
        std::fs::write(
            environments_dir.join("mongo.toml"),
            "version = 1\n[database]\nvendor = \"mongodb\"\nurl = \"mongodb://localhost/test\"\n",
        )
        .unwrap();
        std::fs::write(
            environments_dir.join("postgres.toml"),
            "version = 1\n[database]\nvendor = \"postgresql\"\nurl = \"jdbc:postgresql://localhost/test\"\n",
        )
        .unwrap();
        let environments = vec![
            "mongo".to_string(),
            "postgres".to_string(),
            "missing".to_string(),
        ];

        let collection =
            collect_posture_reports(&ConfigLoader::new(), &root, &environments, false, true)
                .unwrap();

        assert!(collection.reports.is_empty());
        assert_eq!(collection.failures.len(), 2);
        assert_eq!(collection.failures[0].0, "postgres");
        assert_eq!(collection.failures[1].0, "missing");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn posture_failure_output_handles_each_environment() {
        let environment = "pre".to_string();
        print_terminal_error_line("OK");
        print_posture_failures_text(&[(&environment, "connection refused".into())], false);
        assert!(render_posture_failures("text", &[]).is_ok());
        assert!(
            render_posture_failures("json", &[(&environment, "connection refused".into())])
                .is_err()
        );
        let report = posture::Report {
            version: 1,
            backend: "postgresql",
            role: "reader".into(),
            database: "app".into(),
            status: "safe",
            findings: vec![],
            fingerprint: "test".into(),
            acknowledged: false,
        };
        assert!(acknowledge_posture_report(&ConfigLoader::new(), &report, false).is_ok());
        assert_eq!(canonical_tunnel_host("localhost"), "local");
        assert_eq!(canonical_tunnel_host("127.0.0.1"), "local");
        assert_eq!(canonical_tunnel_host("127.1"), "local");
        assert_eq!(canonical_tunnel_host("0.0.0.0"), "wildcard");
        assert_eq!(canonical_tunnel_host("::"), "wildcard");
        assert_eq!(canonical_tunnel_host("db.internal"), "db.internal");
        assert_eq!(canonical_tunnel_host("999.999"), "999.999");
        assert!(!direct_postgres_reachable(
            "jdbc:postgresql://127.0.0.1:1/test"
        ));
        assert!(!direct_postgres_reachable("not-a-jdbc-url"));
        assert_eq!(
            postgres_jdbc_host_ports("jdbc:postgresql://db.example/app"),
            vec![("db.example".into(), 5432)]
        );
        assert_eq!(
            postgres_jdbc_host_ports("jdbc:postgresql://[::1]:15432/app"),
            vec![("::1".into(), 15432)]
        );
        assert_eq!(
            postgres_jdbc_host_ports("jdbc:postgresql://[::1]/app"),
            vec![("::1".into(), 5432)]
        );
        assert_eq!(
            postgres_jdbc_host_ports("jdbc:postgresql://db.example:15432/app"),
            vec![("db.example".into(), 15432)]
        );
        assert_eq!(
            postgres_jdbc_host_ports("jdbc:postgresql://db1:5432,db2:5433/app"),
            vec![("db1".into(), 5432), ("db2".into(), 5433)]
        );
        assert!(postgres_jdbc_host_ports("not-a-jdbc-url").is_empty());
        assert!(tunnel_endpoints_overlap(
            &("wildcard".into(), 15432),
            &("192.0.2.1".into(), 15432)
        ));
        assert!(!tunnel_endpoints_overlap(
            &("wildcard".into(), 15432),
            &("192.0.2.1".into(), 15433)
        ));
        assert!(tunnel_endpoints_overlap(
            &("localhost".into(), 15432),
            &("local".into(), 15432)
        ));
        assert_eq!(posture_completion_marker("safe"), "OK");
        assert_eq!(posture_completion_marker("warning"), "WARNING");
        assert_eq!(posture_completion_marker("unsafe"), "UNSAFE");
        assert_eq!(terminal_line("UNSAFE", true), "\x1b[31mUNSAFE\x1b[0m");
        for line in [
            "FAILED",
            "UNSAFE",
            "Sidecar error:",
            "SSH error:",
            "ERROR:",
            "Reconnect failed",
        ] {
            assert!(is_terminal_error_line(line));
        }
        assert!(!is_terminal_error_line("OK"));
        let environment = "pre".to_string();
        let resolved = config::ResolvedConfig {
            project: config::ProjectConfig::default(),
            environment: config::EnvironmentConfig {
                version: 1,
                database: config::DatabaseConfig {
                    kind: backend::BackendKind::Jdbc,
                    vendor: Some("postgresql".into()),
                    driver: Some("postgresql".into()),
                    url: "jdbc:postgresql://localhost/test".into(),
                    username: "reader".into(),
                    secret: None,
                },
                tls: None,
                ssh: None,
                limits: Default::default(),
            },
            driver: None,
            password: String::new(),
            repo_root: Path::new(".").into(),
        };
        let mut endpoints = Vec::new();
        assert!(prepare_posture_tunnel(
            &PathBuf::from("."),
            &environment,
            &resolved,
            &mut endpoints
        )
        .is_ok());
        let mut candidate = resolved;
        candidate.environment.database.url = "jdbc:postgresql://127.0.0.1:1/test".into();
        candidate.environment.ssh = Some(config::SshConfig {
            enabled: true,
            bastion: None,
            host: Some("localhost".into()),
            port: Some(22),
            username: Some("ssh".into()),
            secret_account: None,
            identity_file: None,
            known_hosts: None,
            local_host: Some("127.0.0.1".into()),
            local_port: Some(15432),
            forward_host: Some("db.internal".into()),
            forward_port: Some(5432),
            auth_type: None,
        });
        assert_eq!(
            posture_tunnel_endpoint(&candidate),
            Some(("local".into(), 15432))
        );
        candidate.environment.database.url = "jdbc:postgresql://localhost:15432/test".into();
        assert_eq!(
            posture_tunnel_endpoint(&candidate),
            Some(("local".into(), 15432))
        );

        let other_environment = "other".to_string();
        let mut occupied = vec![(("local".to_string(), 15432), &other_environment)];
        let error =
            prepare_posture_tunnel(&PathBuf::from("."), &environment, &candidate, &mut occupied)
                .unwrap_err();
        assert!(error.to_string().contains("SSH local endpoints overlap"));
        assert!(!error.to_string().contains("15432"));
        assert!(!error.to_string().contains("127.0.0.1"));
    }

    use super::*;

    #[test]
    fn covers_small_configuration_helpers() {
        let root = std::env::temp_dir().join(format!("safeselect-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".safeselect/environments")).unwrap();
        std::fs::write(
            root.join(".safeselect/environments/dev.toml"),
            "version = 1\n[database]\nurl = \"jdbc:postgresql://localhost/db\"\n",
        )
        .unwrap();
        assert!(environment_config_file(&root, "dev").exists());
        assert!(load_environment_config(&root, "dev").is_ok());
        assert!(check_version_and_maybe_reset(&root).is_ok());
        let mut removed = String::new();
        append_deleted_secret_message(
            &mut removed,
            Some(("env".into(), Some("SAFESELECT_PASSWORD".into()), None)),
        )
        .unwrap();
        assert!(removed.contains("not removed"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn covers_local_connectivity_failure_helpers() {
        let addr: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert!(!check_postgres(&addr));
        assert!(!kill_processes("not-a-pid"));
    }

    #[test]
    fn builds_ssh_tunnel_arguments_and_reports_missing_fields() {
        let mut ssh = config::SshConfig {
            enabled: true,
            bastion: None,
            host: Some("jump.example.com".into()),
            port: Some(2222),
            username: Some("tunnel".into()),
            secret_account: None,
            identity_file: Some("/tmp/demo_ed25519".into()),
            known_hosts: Some("/tmp/known_hosts".into()),
            local_host: Some("127.0.0.1".into()),
            local_port: Some(15432),
            forward_host: Some("db.internal".into()),
            forward_port: Some(5432),
            auth_type: Some("KEY".into()),
        };

        assert!(missing_ssh_fields(&ssh).is_empty());
        assert!(!ssh_uses_password(&ssh));
        let args = build_tunnel_ssh_args(&ssh);
        assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
        assert!(args
            .iter()
            .any(|arg| arg == "127.0.0.1:15432:db.internal:5432"));
        assert!(args.iter().any(|arg| arg == "tunnel@jump.example.com"));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-i", "/tmp/demo_ed25519"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "UserKnownHostsFile=/tmp/known_hosts"]));

        let mut password_ssh = ssh.clone();
        password_ssh.auth_type = Some("PASSWORD".into());
        password_ssh.identity_file = None;
        password_ssh.known_hosts = None;
        assert!(ssh_uses_password(&password_ssh));
        let password_args = build_tunnel_ssh_args(&password_ssh);
        assert!(!password_args.iter().any(|arg| arg == "-i"));
        assert!(!password_args
            .iter()
            .any(|arg| arg == "UserKnownHostsFile=/tmp/known_hosts"));
        assert!(password_args
            .iter()
            .any(|arg| arg == "127.0.0.1:15432:db.internal:5432"));

        ssh.forward_host = None;
        ssh.forward_port = None;
        assert_eq!(missing_ssh_fields(&ssh), ["forward_host", "forward_port"]);
    }

    #[test]
    fn slugs_environment_names() {
        assert_eq!(slug_env_name(" Production / EU "), "production-eu");
        assert_eq!(slug_env_name("already-valid"), "already-valid");
    }

    #[test]
    fn resolves_default_compass_path() {
        assert!(default_compass_path().ends_with("MongoDB Compass"));
    }

    #[test]
    fn chooses_unique_environment_names() {
        let dir = std::env::temp_dir().join(format!("safeselect-env-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mongodb.toml"), "").unwrap();
        std::fs::write(dir.join("mongodb-2.toml"), "").unwrap();

        assert_eq!(unique_env_name(&dir, ""), "mongodb-3");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compares_shared_bastion_identity() {
        let shared = config::SharedSshConfig {
            host: Some("bastion".into()),
            port: Some(22),
            username: Some("jump".into()),
            secret_account: None,
            identity_file: None,
            known_hosts: None,
            auth_type: None,
        };
        let ssh = config::SshConfig {
            enabled: true,
            bastion: None,
            host: shared.host.clone(),
            port: shared.port,
            username: shared.username.clone(),
            secret_account: None,
            identity_file: None,
            known_hosts: None,
            local_host: None,
            local_port: None,
            forward_host: None,
            forward_port: None,
            auth_type: None,
        };

        assert!(same_bastion_identity(&shared, &ssh));
    }

    #[test]
    fn writes_default_project_configuration() {
        let root = std::env::temp_dir().join(format!("safeselect-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        write_project_toml(&root).unwrap();

        let project: config::ProjectConfig =
            toml::from_str(&std::fs::read_to_string(root.join("project.toml")).unwrap()).unwrap();
        assert_eq!(project.version, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn skips_driver_setup_when_a_driver_is_available() {
        let root =
            std::env::temp_dir().join(format!("safeselect-driver-setup-{}", std::process::id()));
        let drivers = root.join("drivers");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&drivers).unwrap();
        std::fs::write(
            drivers.join("postgresql.toml"),
            "version = 1\nvendor = \"postgresql\"\npath = \"/tmp/driver.jar\"\nclass = \"org.postgresql.Driver\"\nsha256 = \"abc\"\n",
        )
        .unwrap();
        let previous = std::env::var_os("SAFESELECT_CONFIG_DIR");
        std::env::set_var("SAFESELECT_CONFIG_DIR", &root);

        assert!(setup_driver_if_missing().is_ok());

        if let Some(value) = previous {
            std::env::set_var("SAFESELECT_CONFIG_DIR", value);
        } else {
            std::env::remove_var("SAFESELECT_CONFIG_DIR");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checks_gitignore_variants_without_failing() {
        let root =
            std::env::temp_dir().join(format!("safeselect-gitignore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        check_gitignore(&root);
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        check_gitignore(&root);
        std::fs::write(root.join(".gitignore"), ".safeselect/\n").unwrap();
        check_gitignore(&root);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn displays_database_target_from_urls() {
        assert_eq!(
            display_database_target("postgresql://db/app?sslmode=require"),
            "app"
        );
        assert_eq!(display_database_target("postgresql://db/"), "?");
    }

    #[test]
    fn rewrites_mongodb_url_for_local_endpoint() {
        let rewritten = rewrite_mongodb_url_for_local_endpoint(
            "mongodb://user:secret@remote:27017/app",
            "localhost",
            2222,
        )
        .unwrap();
        assert!(rewritten.contains("localhost:2222"));
        assert!(rewritten.contains("user:secret@localhost:2222"));
        assert!(rewritten.contains("directConnection=true"));
    }

    #[test]
    fn extracts_tcp_host_and_port_variants() {
        assert_eq!(
            extract_tcp_host_port("mongodb://db.example:27018/app"),
            Some(("db.example".to_string(), 27018))
        );
        assert_eq!(
            extract_tcp_host_port("mongodb://db.example/app"),
            Some(("db.example".to_string(), 27017))
        );
        assert_eq!(extract_tcp_host_port("not-a-mongodb-url"), None);
    }

    #[test]
    fn config_show_displays_document_environment() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-config-show-test-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(
            repo_root.join(".safeselect/project.toml"),
            "version = 1\ndisplay_name = \"Config Show Test\"\n",
        )
        .unwrap();
        std::fs::write(
            env_dir.join("local.toml"),
            r#"
version = 1

[database]
kind = "document"
vendor = "mongodb"
url = "mongodb://localhost:27017/test"
username = "test-user"

[tls]
mode = "REQUIRED"

[ssh]
enabled = true
"#,
        )
        .unwrap();

        let result = cmd_config_show(
            &ConfigLoader::new(),
            Some(repo_root.clone()),
            "local".to_string(),
        );

        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn config_validate_handles_explicit_and_current_projects() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-config-validate-test-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(repo_root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
        std::fs::write(
            env_dir.join("local.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n",
        )
        .unwrap();

        let loader = ConfigLoader::new();
        assert!(validate_explicit_project(&loader, &repo_root, None).is_ok());
        assert!(validate_explicit_project(&loader, &repo_root, Some("local")).is_ok());
        assert!(validate_current_project(&loader, &repo_root, None).is_ok());
        assert!(validate_current_project(&loader, &repo_root, Some("local")).is_ok());
        assert!(validate_current_project(&loader, &repo_root.join("missing"), None).is_ok());
        assert!(validate_explicit_project(&loader, &repo_root.join("missing"), None).is_err());

        std::fs::write(env_dir.join("broken.toml"), "[database\n").unwrap();
        assert!(validate_explicit_project(&loader, &repo_root, None).is_err());

        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn delete_environment_removes_file_and_preserves_env_secret_notice() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-config-delete-test-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(repo_root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
        std::fs::write(
            env_dir.join("local.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n[database.secret]\nsource = \"env\"\nvariable = \"SAFESELECT_TEST_PASSWORD\"\n",
        )
        .unwrap();

        let loader = ConfigLoader::new();
        assert!(
            delete_environment_config(&loader, "local".to_string(), Some(repo_root.clone()))
                .is_ok()
        );
        assert!(!env_dir.join("local.toml").exists());
        assert!(
            delete_environment_config(&loader, "missing".to_string(), Some(repo_root.clone()))
                .is_err()
        );

        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn password_commands_validate_environment_before_keychain_access() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-password-validation-test-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(repo_root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
        std::fs::write(
            env_dir.join("without-ssh.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n",
        )
        .unwrap();

        let loader = ConfigLoader::new();
        assert!(set_password_for_environment(
            &loader,
            "missing".to_string(),
            Some("secret".to_string()),
            Some(repo_root.clone()),
        )
        .is_err());
        assert!(set_ssh_password_for_environment(
            &loader,
            "without-ssh".to_string(),
            Some("secret".to_string()),
            Some(repo_root.clone()),
        )
        .is_err());

        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn password_commands_store_explicit_passwords_through_injected_keychain() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-password-store-test-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(repo_root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
        std::fs::write(
            env_dir.join("database.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n",
        )
        .unwrap();
        std::fs::write(
            env_dir.join("ssh.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n[ssh]\nenabled = true\n",
        )
        .unwrap();

        let loader = ConfigLoader::new();
        set_password_for_environment_with_store(
            &loader,
            "database".to_string(),
            Some("database-secret".to_string()),
            Some(repo_root.clone()),
            |account, password| {
                assert!(account.ends_with("/database"));
                assert_eq!(password, "database-secret");
                Ok(())
            },
        )
        .unwrap();
        set_ssh_password_for_environment_with_store(
            &loader,
            "ssh".to_string(),
            Some("ssh-secret".to_string()),
            Some(repo_root.clone()),
            |account, password| {
                assert!(account.ends_with("/ssh/ssh"));
                assert_eq!(password, "ssh-secret");
                Ok(())
            },
        )
        .unwrap();

        let database = std::fs::read_to_string(env_dir.join("database.toml")).unwrap();
        let ssh = std::fs::read_to_string(env_dir.join("ssh.toml")).unwrap();
        assert!(database.contains("source = \"macos-keychain\""));
        assert!(ssh.contains("auth_type = \"PASSWORD\""));
        assert!(ssh.contains("secret_account"));

        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn ssh_password_command_rejects_missing_or_unconfigured_ssh() {
        let repo_root = std::env::temp_dir().join(format!(
            "safeselect-ssh-password-errors-{}",
            std::process::id()
        ));
        let env_dir = repo_root.join(".safeselect/environments");
        let _ = std::fs::remove_dir_all(&repo_root);
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(repo_root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
        std::fs::write(
            env_dir.join("no-ssh.toml"),
            "version = 1\n[database]\nkind = \"document\"\nurl = \"mongodb://localhost\"\n",
        )
        .unwrap();

        let loader = ConfigLoader::new();
        assert!(set_ssh_password_for_environment_with_store(
            &loader,
            "missing".into(),
            Some("secret".into()),
            Some(repo_root.clone()),
            |_, _| Ok(())
        )
        .is_err());
        assert!(set_ssh_password_for_environment_with_store(
            &loader,
            "no-ssh".into(),
            Some("secret".into()),
            Some(repo_root.clone()),
            |_, _| Ok(())
        )
        .is_err());

        let _ = std::fs::remove_dir_all(repo_root);
    }

    #[test]
    fn all_diagnostic_codes_have_stable_names() {
        let codes = [
            DiagnosticCode::ConfigResolved,
            DiagnosticCode::DriverVerified,
            DiagnosticCode::SecretResolved,
            DiagnosticCode::SshBastionReachable,
            DiagnosticCode::SshBastionUnreachable,
            DiagnosticCode::SshBastionUnresolved,
            DiagnosticCode::SshIdentityMissing,
            DiagnosticCode::SshTunnelAttempt,
            DiagnosticCode::SshTunnelFailed,
            DiagnosticCode::PostgresReachable,
            DiagnosticCode::PostgresUnreachable,
            DiagnosticCode::SidecarStartAttempt,
            DiagnosticCode::SidecarBackendOk,
            DiagnosticCode::SidecarConnectionFailed,
            DiagnosticCode::BackendVerificationOk,
            DiagnosticCode::BackendVerificationFailed,
            DiagnosticCode::AllChecksPassed,
            DiagnosticCode::ConnectionLost,
            DiagnosticCode::SshTunnelRecoveryAttempt,
            DiagnosticCode::JdbcReconnectAttempt,
            DiagnosticCode::SidecarRestartAttempt,
            DiagnosticCode::RecoveryOk,
            DiagnosticCode::RecoveryFailed,
        ];

        let expected = [
            "SAFESELECT_CONFIG_RESOLVED",
            "SAFESELECT_DRIVER_VERIFIED",
            "SAFESELECT_SECRET_RESOLVED",
            "SAFESELECT_SSH_BASTION_REACHABLE",
            "SAFESELECT_SSH_BASTION_UNREACHABLE",
            "SAFESELECT_SSH_BASTION_UNRESOLVED",
            "SAFESELECT_SSH_IDENTITY_MISSING",
            "SAFESELECT_SSH_TUNNEL_ATTEMPT",
            "SAFESELECT_SSH_TUNNEL_FAILED",
            "SAFESELECT_POSTGRES_REACHABLE",
            "SAFESELECT_POSTGRES_UNREACHABLE",
            "SAFESELECT_SIDECAR_START_ATTEMPT",
            "SAFESELECT_SIDECAR_BACKEND_OK",
            "SAFESELECT_SIDECAR_CONNECTION_FAILED",
            "SAFESELECT_BACKEND_VERIFICATION_OK",
            "SAFESELECT_BACKEND_VERIFICATION_FAILED",
            "SAFESELECT_ALL_CHECKS_PASSED",
            "SAFESELECT_CONNECTION_LOST",
            "SAFESELECT_SSH_TUNNEL_RECOVERY_ATTEMPT",
            "SAFESELECT_JDBC_RECONNECT_ATTEMPT",
            "SAFESELECT_SIDECAR_RESTART_ATTEMPT",
            "SAFESELECT_RECOVERY_OK",
            "SAFESELECT_RECOVERY_FAILED",
        ];

        assert_eq!(codes.len(), expected.len());
        for (code, expected_name) in codes.iter().zip(expected) {
            assert_eq!(code.as_str(), expected_name);
        }
    }

    #[test]
    fn uninstall_checks_supported_user_binary_locations() {
        let home = dirs::home_dir().expect("home directory should be available");

        assert_eq!(
            uninstall_binary_paths(),
            vec![
                home.join(".local/bin/safeselect"),
                home.join(".cargo/bin/safeselect"),
            ]
        );
    }

    fn sample_dbeaver_connection() -> dbeaver::DBeaverConnection {
        dbeaver::DBeaverConnection {
            name: "sample".to_string(),
            host: "db.example.com".to_string(),
            port: 5432,
            database: "app".to_string(),
            driver: "postgresql".to_string(),
            username: "postgres".to_string(),
            password: None,
            sslmode: None,
            ssh_host: Some("localhost".to_string()),
            ssh_port: Some(2222),
            ssh_user: None,
            ssh_local_host: None,
            ssh_local_port: None,
            ssh_key_file: None,
            ssh_auth_type: Some("PASSWORD".to_string()),
        }
    }

    #[test]
    fn warns_when_dbeaver_export_looks_like_shared_local_tunnel() {
        let conn = sample_dbeaver_connection();

        let warning = dbeaver_shared_tunnel_warning(&conn);

        assert!(warning.is_some());
        assert!(warning.unwrap().contains("localhost:2222"));
    }

    #[test]
    fn does_not_warn_when_real_ssh_user_is_present() {
        let mut conn = sample_dbeaver_connection();
        conn.ssh_user = Some("antonio".to_string());

        let warning = dbeaver_shared_tunnel_warning(&conn);

        assert!(warning.is_none());
    }

    #[test]
    fn dbeaver_forward_target_defaults_ignore_local_tunnel_endpoint() {
        let mut conn = sample_dbeaver_connection();
        conn.ssh_local_host = Some("localhost".to_string());
        conn.ssh_local_port = Some(conn.port);

        assert_eq!(
            dbeaver_forward_target_defaults(&conn),
            (String::new(), 5432)
        );

        conn.ssh_local_port = Some(15432);
        assert_eq!(
            dbeaver_forward_target_defaults(&conn),
            ("db.example.com".to_string(), 5432)
        );
    }

    #[test]
    fn falls_back_to_legacy_default_port_when_no_ports_are_used() {
        let port = next_available_ssh_local_port(&std::collections::HashSet::new()).unwrap();

        assert_eq!(port, DEFAULT_SSH_LOCAL_PORT);
    }

    #[test]
    fn allocates_distinct_ports_for_multiple_ssh_environments() {
        let mut used = std::collections::HashSet::new();

        let first = next_available_ssh_local_port(&used).unwrap();
        used.insert(first);
        let second = next_available_ssh_local_port(&used).unwrap();

        assert_eq!(first, DEFAULT_SSH_LOCAL_PORT);
        assert_eq!(second, DEFAULT_SSH_LOCAL_PORT + 1);
    }

    #[test]
    fn detects_used_ports_from_legacy_environment_urls() {
        let temp =
            std::env::temp_dir().join(format!("safeselect-ssh-port-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let env_dir = temp.join(".safeselect").join("environments");
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(
            env_dir.join("pre.toml"),
            r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://localhost:15432/app?sslmode=require"
username = "usr_app"

[ssh]
enabled = true
host = "localhost"
port = 2222
username = "jumpboxdev"
forward_host = "db.example.com"
forward_port = 5432
auth_type = "PASSWORD"
"#,
        )
        .unwrap();

        let used = collect_used_ssh_local_ports(&temp);
        let next = next_available_ssh_local_port(&used).unwrap();

        assert!(used.contains(&DEFAULT_SSH_LOCAL_PORT));
        assert_eq!(next, DEFAULT_SSH_LOCAL_PORT + 1);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn loads_reusable_ssh_configs_from_other_environments() {
        let temp =
            std::env::temp_dir().join(format!("safeselect-ssh-reuse-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let env_dir = temp.join(".safeselect").join("environments");
        std::fs::create_dir_all(&env_dir).unwrap();

        std::fs::write(
            env_dir.join("pre.toml"),
            r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://localhost:15432/app?sslmode=require"
username = "usr_app"

[ssh]
enabled = true
host = "bastion.example.com"
port = 2222
username = "jumpboxdev"
local_host = "localhost"
local_port = 15432
forward_host = "db.example.com"
forward_port = 5432
auth_type = "PASSWORD"
"#,
        )
        .unwrap();

        std::fs::write(
            env_dir.join("local.toml"),
            r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://db.example.com:5432/app?sslmode=require"
username = "usr_app"
"#,
        )
        .unwrap();

        let reusable = load_reusable_ssh_configs(&temp, "new-env").unwrap();

        assert_eq!(reusable.len(), 1);
        assert_eq!(reusable[0].0, "pre");
        assert_eq!(reusable[0].1.host.as_deref(), Some("bastion.example.com"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn includes_current_batch_ssh_configs_in_reuse_candidates() {
        let temp = std::env::temp_dir().join(format!(
            "safeselect-ssh-batch-reuse-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join(".safeselect").join("environments")).unwrap();

        let current_batch = vec![(
            "pre-usr".to_string(),
            config::SshConfig {
                enabled: true,
                bastion: Some("pre-int".to_string()),
                host: Some("bastion.example.com".to_string()),
                port: Some(2222),
                username: Some("jumpboxdev".to_string()),
                secret_account: Some("mic-icifqaproc/pre-usr/ssh".to_string()),
                identity_file: None,
                known_hosts: None,
                local_host: Some("localhost".to_string()),
                local_port: Some(15432),
                forward_host: Some("db.example.com".to_string()),
                forward_port: Some(5432),
                auth_type: Some("PASSWORD".to_string()),
            },
        )];

        let reusable = collect_reusable_ssh_configs(&temp, "pre-dba", &current_batch).unwrap();

        assert_eq!(reusable.len(), 1);
        assert_eq!(reusable[0].0, "pre-usr");
        assert_eq!(reusable[0].1.host.as_deref(), Some("bastion.example.com"));
        assert_eq!(
            reusable[0].1.secret_account.as_deref(),
            Some("mic-icifqaproc/pre-usr/ssh")
        );

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn merges_project_bastion_into_environment_ssh() {
        let mut project = config::ProjectConfig::default();
        project.ssh_bastions.insert(
            "pre-int".to_string(),
            config::SharedSshConfig {
                host: Some("bastion.example.com".to_string()),
                port: Some(2222),
                username: Some("jumpboxdev".to_string()),
                secret_account: Some("mic-icifqaproc/pre-int/ssh".to_string()),
                identity_file: None,
                known_hosts: None,
                auth_type: Some("PASSWORD".to_string()),
            },
        );

        let mut environment = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                kind: crate::backend::BackendKind::Jdbc,
                vendor: Some("postgresql".to_string()),
                driver: Some("postgresql".to_string()),
                url: "jdbc:postgresql://localhost:15433/app?sslmode=require".to_string(),
                username: "usr_app".to_string(),
                secret: None,
            },
            tls: None,
            ssh: Some(config::SshConfig {
                enabled: true,
                bastion: Some("pre-int".to_string()),
                host: None,
                port: None,
                username: None,
                secret_account: None,
                identity_file: None,
                known_hosts: None,
                local_host: Some("localhost".to_string()),
                local_port: Some(15433),
                forward_host: Some("db.example.com".to_string()),
                forward_port: Some(5432),
                auth_type: None,
            }),
            limits: config::LimitsOverride::default(),
        };

        config::merge_project_ssh(&project, &mut environment).unwrap();

        let ssh = environment.ssh.unwrap();
        assert_eq!(ssh.bastion.as_deref(), Some("pre-int"));
        assert_eq!(ssh.host.as_deref(), Some("bastion.example.com"));
        assert_eq!(ssh.port, Some(2222));
        assert_eq!(ssh.username.as_deref(), Some("jumpboxdev"));
        assert_eq!(
            ssh.secret_account.as_deref(),
            Some("mic-icifqaproc/pre-int/ssh")
        );
        assert_eq!(ssh.local_port, Some(15433));
    }

    #[test]
    fn environment_ssh_from_bastion_keeps_only_reference_and_forwarding() {
        let ssh = config::SshConfig {
            enabled: true,
            bastion: None,
            host: Some("bastion.example.com".to_string()),
            port: Some(2222),
            username: Some("jumpboxdev".to_string()),
            secret_account: Some("mic-icifqaproc/pre-int/ssh".to_string()),
            identity_file: Some("/tmp/id_ed25519".to_string()),
            known_hosts: Some("/tmp/known_hosts".to_string()),
            local_host: Some("localhost".to_string()),
            local_port: Some(15435),
            forward_host: Some("db.example.com".to_string()),
            forward_port: Some(5432),
            auth_type: Some("PASSWORD".to_string()),
        };

        let env_ssh = environment_ssh_from_bastion("jumpboxdev-localhost-2222".to_string(), &ssh);

        assert!(env_ssh.enabled);
        assert_eq!(
            env_ssh.bastion.as_deref(),
            Some("jumpboxdev-localhost-2222")
        );
        assert!(env_ssh.host.is_none());
        assert!(env_ssh.port.is_none());
        assert!(env_ssh.username.is_none());
        assert!(env_ssh.secret_account.is_none());
        assert!(env_ssh.identity_file.is_none());
        assert!(env_ssh.known_hosts.is_none());
        assert!(env_ssh.auth_type.is_none());
        assert_eq!(env_ssh.local_host.as_deref(), Some("localhost"));
        assert_eq!(env_ssh.local_port, Some(15435));
        assert_eq!(env_ssh.forward_host.as_deref(), Some("db.example.com"));
        assert_eq!(env_ssh.forward_port, Some(5432));
    }

    #[test]
    fn default_bastion_name_omits_localhost_for_shorter_aliases() {
        let ssh = config::SshConfig {
            enabled: true,
            bastion: None,
            host: Some("localhost".to_string()),
            port: Some(2222),
            username: Some("jumpboxdev".to_string()),
            secret_account: None,
            identity_file: None,
            known_hosts: None,
            local_host: None,
            local_port: None,
            forward_host: None,
            forward_port: None,
            auth_type: None,
        };

        assert_eq!(default_bastion_name(&ssh), "jumpboxdev-2222");
    }

    #[test]
    fn compass_local_tunnel_endpoint_is_imported_for_reuse() {
        let conn = crate::compass::CompassConnection {
            name: "iopcompclopre002 (pre)".to_string(),
            url: "mongodb+srv://user@cluster.mongodb.net".to_string(),
            ssh_host: Some("localhost".to_string()),
            ssh_port: Some(2222),
            ssh_user: Some("jumpboxdev".to_string()),
            ssh_local_host: None,
            ssh_local_port: None,
            ssh_key_file: None,
            ssh_auth_type: None,
        };

        let ssh = compass_ssh_config(&conn).expect("expected ssh config");
        assert_eq!(ssh.host.as_deref(), Some("localhost"));
        assert_eq!(ssh.port, Some(2222));
        assert_eq!(ssh.username.as_deref(), Some("jumpboxdev"));
        assert_eq!(ssh.local_host.as_deref(), Some("localhost"));
        assert_eq!(ssh.local_port, None);
        assert_eq!(ssh.forward_host.as_deref(), Some("cluster.mongodb.net"));
        assert_eq!(ssh.forward_port, Some(27017));
        let warning = compass_shared_tunnel_warning(&conn);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Azure Bastion tunnel"));
    }

    #[test]
    fn extract_tcp_host_port_supports_mongodb_srv() {
        let result = extract_tcp_host_port(
            "mongodb+srv://user@cluster.example.mongodb.net/?retryWrites=true",
        );
        assert_eq!(
            result,
            Some(("cluster.example.mongodb.net".to_string(), 27017))
        );
    }

    #[test]
    fn rewrite_mongodb_srv_url_for_local_endpoint_uses_mongodb_scheme() {
        let result = rewrite_mongodb_url_for_local_endpoint(
            "mongodb+srv://user@cluster.example.mongodb.net/?retryWrites=true",
            "localhost",
            2222,
        );
        assert_eq!(
            result.as_deref(),
            Some("mongodb://user@localhost:2222/?retryWrites=true&tls=true&tlsAllowInvalidHostnames=true&directConnection=true")
        );
    }

    #[test]
    fn rewrite_mongodb_srv_url_without_query_adds_tunnel_tls_options() {
        let result = rewrite_mongodb_url_for_local_endpoint(
            "mongodb+srv://user@cluster.example.mongodb.net/",
            "localhost",
            15433,
        );

        assert_eq!(
            result,
            Some(
                "mongodb://user@localhost:15433/?tls=true&tlsAllowInvalidHostnames=true&directConnection=true"
                    .to_string()
            )
        );
    }

    #[test]
    fn rewrite_mongodb_srv_url_adds_read_preference_for_tagged_reads() {
        let result = rewrite_mongodb_url_for_local_endpoint(
            "mongodb+srv://user@cluster.example.mongodb.net/?readPreferenceTags=nodeType%3Areadonly",
            "localhost",
            2222,
        );
        assert_eq!(
            result.as_deref(),
            Some("mongodb://user@localhost:2222/?readPreference=secondaryPreferred&readPreferenceTags=nodeType%3Areadonly&tls=true&tlsAllowInvalidHostnames=true&directConnection=true")
        );
    }

    #[test]
    fn parses_mongodb_srv_record() {
        let result = parse_mongodb_srv_record(
            "0 0 1241 pl-0-westeurope-azure.sezph.mongodb.net.\n0 0 1242 other.mongodb.net.\n",
        );
        assert_eq!(
            result,
            Some(("pl-0-westeurope-azure.sezph.mongodb.net".to_string(), 1241))
        );
    }

    #[test]
    fn inject_mongodb_password_placeholder_adds_placeholder() {
        let result = inject_mongodb_password_placeholder(
            "mongodb://user@localhost:2222/app?retryWrites=true",
            "user",
        );
        assert_eq!(
            result,
            "mongodb://user:__SAFESELECT_PASSWORD__@localhost:2222/app?retryWrites=true"
        );
    }

    #[test]
    fn lists_only_environment_toml_files() {
        let root = std::env::temp_dir().join(format!("safeselect-envs-{}", uuid::Uuid::new_v4()));
        let env_dir = root.join(".safeselect/environments");
        std::fs::create_dir_all(&env_dir).unwrap();
        std::fs::write(env_dir.join("prod.toml"), "").unwrap();
        std::fs::write(env_dir.join("dev.toml"), "").unwrap();
        std::fs::write(env_dir.join("README.md"), "").unwrap();
        assert_eq!(list_environment_names(&root).unwrap(), vec!["dev", "prod"]);
        assert_eq!(
            selected_environment_names(&root, Some("staging")).unwrap(),
            vec!["staging"]
        );
        assert_eq!(
            selected_environment_names(&root, None).unwrap(),
            vec!["dev", "prod"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reports_missing_environment_directory() {
        let root =
            std::env::temp_dir().join(format!("safeselect-missing-{}", uuid::Uuid::new_v4()));
        assert!(list_environment_names(&root).is_err());
    }

    #[test]
    fn reusable_ssh_entry_skips_non_environment_files() {
        let project = config::ProjectConfig::default();
        assert!(reusable_ssh_entry(&project, Path::new("notes.txt"), "dev").is_none());
        assert!(reusable_ssh_entry(&project, Path::new("dev.toml"), "dev").is_none());
        assert!(reusable_ssh_entry(&project, Path::new("missing.toml"), "dev").is_none());
    }
}
