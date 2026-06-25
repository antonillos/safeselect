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
        tracing::error!("{e}");
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
                    return Err(SafeselectError::Other(format!(
                        "path does not exist: {}",
                        cwd.display()
                    )));
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
            if let Some(environment) = environment {
                cmd_check(&loader, &dir, &environment, verbose)
            } else {
                let env_names = list_environment_names(&dir)?;
                if env_names.is_empty() {
                    println!(
                        "No environments found in {}",
                        dir.join(".safeselect").join("environments").display()
                    );
                    return Ok(());
                }
                run_checks(&dir, &env_names, verbose)
            }
        }
        Command::Doctor {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_check(&loader, &dir, &environment, false)
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
            cmd_reconnect(&loader, &dir, &environment)
        }
        Command::Uninstall { force } => cmd_uninstall(force),
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

fn default_agent_entry_name(project_name: &str, environment: &str) -> String {
    format!("safeselect-{project_name}-{environment}")
}

fn list_environment_names(repo_root: &Path) -> Result<Vec<String>> {
    let env_dir = repo_root.join(".safeselect").join("environments");
    let mut env_names = Vec::new();
    let entries = std::fs::read_dir(&env_dir).map_err(|e| {
        SafeselectError::Config(format!(
            "cannot read environments in {}: {e}",
            env_dir.display()
        ))
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

fn cmd_serve(loader: &ConfigLoader, repo_root: &std::path::Path, environment: &str) -> Result<()> {
    let name = project_display_name(repo_root);
    tracing::info!("Loading config for {name}/{environment}");

    let resolved = loader.resolve_local(repo_root, environment)?;

    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            tracing::warn!("SSH bastion configured — ensure tunnel is active before connecting");
            tracing::warn!("Example: ssh -L 5432:db.internal:5432 bastion.example.com");
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
    )?;

    server.run()?;

    Ok(())
}

fn cmd_config(loader: &ConfigLoader, action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Validate {
            project,
            environment,
        } => {
            match project {
                Some(dir) => {
                    if !dir.join(".safeselect").is_dir() {
                        return Err(SafeselectError::LocalProjectNotFound(dir));
                    }
                    if let Some(ref env) = environment {
                        let _ = loader.resolve_local(&dir, env)?;
                        println!("Config valid: {}/{}", project_display_name(&dir), env);
                    } else {
                        let safeselect_dir = dir.join(".safeselect");
                        if safeselect_dir.join("project.toml").exists()
                            || safeselect_dir.join("environments").is_dir()
                        {
                            println!("Config valid: {}", project_display_name(&dir));
                        } else {
                            return Err(SafeselectError::Config(format!(
                                "incomplete .safeselect/ in {}",
                                dir.display()
                            )));
                        }
                    }
                }
                None => {
                    let cwd = std::env::current_dir()?;
                    match loader.find_local_project(&cwd) {
                        Some(dir) => {
                            println!(
                                ".safeselect/ found at {} ({})",
                                dir.display(),
                                project_display_name(&dir)
                            );
                            println!(
                                "Use --environment <name> to validate a specific environment."
                            );
                            let envs_dir = dir.join(".safeselect").join("environments");
                            if envs_dir.is_dir() {
                                let mut entries: Vec<_> = std::fs::read_dir(&envs_dir)
                                    .into_iter()
                                    .flatten()
                                    .flatten()
                                    .filter(|e| {
                                        e.path().extension().map_or(false, |ext| ext == "toml")
                                    })
                                    .filter_map(|e| {
                                        e.path()
                                            .file_stem()
                                            .and_then(|s| s.to_str().map(String::from))
                                    })
                                    .collect();
                                entries.sort();
                                if !entries.is_empty() {
                                    println!("  Environments: {}", entries.join(", "));
                                }
                            }
                        }
                        None => {
                            println!("No .safeselect/ directory found. Create one with:");
                            println!("  safeselect import-dbeaver <export.zip>");
                            println!("  mkdir -p .safeselect/environments && touch .safeselect/project.toml");
                        }
                    }
                }
            }
            Ok(())
        }
        ConfigAction::Show {
            project,
            environment,
        } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };
            let resolved = loader.resolve_local(&dir, &environment)?;
            let name = project_display_name(&dir);
            println!("Project: {name}");
            println!("Environment: {environment}");
            println!("Backend: {:?}", resolved.environment.database.kind);
            println!("Vendor: {}", resolved.environment.database.vendor());
            if let Some(driver) = resolved.driver.as_ref() {
                println!("Driver: {} ({})", driver.vendor, driver.class);
                println!("JDBC URL: {}", resolved.environment.database.url);
            } else {
                println!("URL: {}", resolved.environment.database.url);
            }
            println!("Username: {}", resolved.environment.database.username);
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
            match resolved.environment.tls {
                Some(ref tls) => println!("Mode: {}", tls.mode),
                None => println!("TLS: disabled"),
            }
            println!();
            println!("--- SSH ---");
            match resolved.environment.ssh {
                Some(ref ssh) => println!("Enabled: {}", ssh.enabled),
                None => println!("SSH: not configured"),
            }

            Ok(())
        }
        ConfigAction::RenameEnvironment { old, new, project } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };

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
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };

            let env_dir = dir.join(".safeselect").join("environments");
            let env_file = env_dir.join(format!("{name}.toml"));

            if !env_file.exists() {
                return Err(SafeselectError::EnvironmentNotFound(
                    name.clone(),
                    env_dir.display().to_string(),
                ));
            }

            // Try to read the secret before deleting the file
            let old_content = std::fs::read_to_string(&env_file).ok();
            let secret_source = old_content.as_ref().and_then(|c| {
                let env_config: config::EnvironmentConfig = toml::from_str(c).ok()?;
                env_config
                    .database
                    .secret
                    .map(|s| (s.source, s.account, s.variable))
            });

            std::fs::remove_file(&env_file)?;

            let mut removed = format!("Deleted environment '{name}'");
            removed.push_str(&format!("\n  File: {}", env_file.display()));

            if let Some((source, account, _variable)) = secret_source {
                match source.as_str() {
                    "macos-keychain" if cfg!(target_os = "macos") => {
                        if let Some(acct) = account {
                            compose::delete_password_from_keychain(&acct)?;
                            removed.push_str("\n  Keychain entry deleted.");
                        }
                    }
                    "env" => {
                        removed.push_str("\n  Environment variable was not removed — delete it manually if no longer needed.");
                    }
                    _ => {}
                }
            }

            println!("{removed}");
            Ok(())
        }
        ConfigAction::SetPassword {
            environment,
            password,
            project,
        } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };

            let env_file = dir
                .join(".safeselect")
                .join("environments")
                .join(format!("{environment}.toml"));
            if !env_file.exists() {
                return Err(SafeselectError::EnvironmentNotFound(
                    environment.clone(),
                    env_file.display().to_string(),
                ));
            }

            let content = std::fs::read_to_string(&env_file)?;
            let env_config: config::EnvironmentConfig = toml::from_str(&content).map_err(|e| {
                SafeselectError::Config(format!("invalid {}: {e}", env_file.display()))
            })?;
            let account = config::preferred_keychain_account(&dir, &environment, &env_config);

            let pw = match password {
                Some(p) => p,
                None => inquire::Password::new(&format!("Password for '{account}'"))
                    .without_confirmation()
                    .prompt()
                    .map_err(|e| SafeselectError::Other(format!("Failed to read password: {e}")))?,
            };

            compose::store_password_in_keychain(&account, &pw)?;
            println!("  ✓ Password stored in Keychain ({account})");

            config::write_keychain_secret_to_env_file(&env_file, &account)?;
            println!("  ✓ Updated {}", env_file.display());
            println!("\nDone. Run: safeselect check --environment {environment}");
            Ok(())
        }
        ConfigAction::Reset { project } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };
            reset_project_config(&dir)
        }
        ConfigAction::Uninstall { project } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader
                        .find_local_project(&cwd)
                        .ok_or_else(|| SafeselectError::LocalProjectNotFound(cwd))?
                }
            };
            uninstall_project_config(&dir)
        }
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
                if path.extension().map_or(false, |ext| ext == "toml") {
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
        println!("  ✓ Removed {removed} environment(s)");
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
            println!("  ✓ Removed {}", safeselect_dir.display());
        }
    } else if removed > 0 {
        println!("\nReset complete. Re-import with:");
        println!("  safeselect import-dbeaver <export.zip>");
        println!("  safeselect import-compose");
    } else if has_project_file {
        println!("  ✓ Cleared shared SSH bastions from project config");
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

fn cmd_agent(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::Detect => {
            let clients = agents::detect_clients()?;
            println!("Detected MCP clients:");
            for client in &clients {
                let status = if client.detected { "✓" } else { "✗" };
                println!("  {status} {}", client.name);
                if client.detected {
                    println!("    Config: {}", client.config_path.display());
                }
            }
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
                            (project.limits.statement_timeout_ms + 30_000) as u64
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
                    None => default_agent_entry_name(&project_display_name(&root), &environment),
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
                            (project.limits.statement_timeout_ms + 30_000) as u64
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
            let clients = agents::detect_clients()?;
            println!("Agent integration status:");

            for client in &clients {
                if client.detected {
                    let content = std::fs::read_to_string(&client.config_path).unwrap_or_default();
                    let has_entries = content.contains("safeselect");
                    println!(
                        "  {} {} {}",
                        if has_entries { "✓" } else { " " },
                        client.name,
                        if has_entries { "(installed)" } else { "" }
                    );
                } else {
                    println!("  ✗ {}", client.name);
                }
            }
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
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if name == current_env_name {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut environment) = toml::from_str::<config::EnvironmentConfig>(&content) else {
            continue;
        };
        if config::merge_project_ssh(&project, &mut environment).is_err() {
            continue;
        }
        let Some(ssh) = environment.ssh else {
            continue;
        };
        if !ssh.enabled {
            continue;
        }
        if ssh.host.as_deref().is_none_or(str::is_empty) {
            continue;
        }

        let bastion_name = ssh.bastion.clone().unwrap_or_else(|| name.to_string());
        reusable.push((bastion_name, ssh));
    }

    reusable.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(reusable)
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
        println!("  ✓ Reusing bastion configuration");
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
                println!("  ✓ SSH password stored in Keychain");
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
        forward_host: Some(conn.host.clone()),
        forward_port: Some(conn.port),
        auth_type,
    })
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
        // Azure PostgreSQL requires SSL at protocol level.
        let url = if let Some(ref ssh) = ssh {
            let local_forward_port = ssh.local_port.unwrap_or(DEFAULT_SSH_LOCAL_PORT);
            format!(
                "jdbc:postgresql://localhost:{}/{}?sslmode=require",
                local_forward_port, conn.database
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
        println!("  ✓ {created} environment(s) added");
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
    run_checks(&cwd, &env_names, false)?;
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
        println!("  ✓ {} environment(s) added", to_import.len());
        check_gitignore(dest_dir);
    } else {
        println!("  ◉ All environments already exist.");
    }

    println!();
    println!("{}", guidance.text);

    let env_names = guidance.imported_env_names;
    setup_driver_if_missing()?;
    setup_passwords_for_missing(dest_dir, &env_names)?;
    run_checks(dest_dir, &env_names, false)?;

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
    run_checks(&cwd, &env_names, false)?;

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
            rewrite_mongodb_url_for_ssh(&conn.url, ssh.local_port.unwrap_or(DEFAULT_SSH_LOCAL_PORT))
                .unwrap_or_else(|| conn.url.clone())
        } else {
            conn.url.clone()
        };
        let (url, username, secret) = prepare_mongodb_url(&project_name, &env_name, &raw_url)?;
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
    println!("Next: safeselect serve --environment <name>");
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
        println!("  ✓ Reusing bastion configuration");
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
                println!("  ✓ SSH password stored in Keychain");
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
    let (forward_host, forward_port) = crate::extract_tcp_host_port(&conn.url)
        .map(|(host, port)| (Some(host), Some(port)))
        .unwrap_or((None, None));

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
Keep it if you open the tunnel yourself first; otherwise replace it with the real bastion."
                .to_string(),
        );
    }

    None
}

fn compass_forward_target(conn: &crate::compass::CompassConnection) -> Option<(String, u16)> {
    crate::extract_tcp_host_port(&conn.url)
}

fn rewrite_mongodb_url_for_ssh(url: &str, local_port: u16) -> Option<String> {
    if url.starts_with("mongodb+srv://") {
        return None;
    }
    let scheme_end = url.find("://")?;
    let authority_start = scheme_end + 3;
    let path_start = url[authority_start..]
        .find('/')
        .map(|idx| authority_start + idx)
        .unwrap_or(url.len());
    let authority = &url[authority_start..path_start];
    let credentials_end = authority.rfind('@').map(|idx| idx + 1).unwrap_or(0);
    let credentials = &authority[..credentials_end];
    Some(format!(
        "{}{}localhost:{}{}",
        &url[..authority_start],
        credentials,
        local_port,
        &url[path_start..]
    ))
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

fn setup_driver_if_missing() -> Result<()> {
    let loader = config::ConfigLoader::new();
    if !loader.list_drivers().map(|d| d.is_empty()).unwrap_or(true) {
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
        println!("  ✓ Updated {env_name}.toml");
    }
    Ok(())
}

/// Try to establish SSH tunnels for environments that need one.
/// Returns PIDs of tunnels started by this call.
pub(crate) fn setup_ssh_tunnels(repo_root: &Path, env_names: &[String]) -> Result<()> {
    use std::io::Write;
    use std::time::Duration;

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

        // Step 2: Check PostgreSQL via tunnel endpoint.
        let pg_via_tunnel = check_postgres_endpoint(tunnel_local_host, tunnel_local_port);

        // Step 3: Check PostgreSQL via original target (if resolvable)
        let pg_via_direct = extract_host_port(&cfg.database.url)
            .map(|(host, port)| check_postgres_endpoint(&host, port))
            .unwrap_or(false);

        if pg_via_direct || pg_via_tunnel {
            print!("  ◉ PostgreSQL reachable ({env_name})");
            std::io::stdout().flush()?;
            continue;
        }

        if bastion_up {
            print!("  ◇ Bastion reachable but PostgreSQL not responding ({env_name})");
            std::io::stdout().flush()?;
        }

        let use_password = match &ssh.auth_type {
            Some(at) if at == "PASSWORD" => true,
            _ => false,
        };

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
            println!("  ⚠  SSH bastion {bastion_host}:{bastion_port} unreachable (connect timed out after 3s)");
            if !use_password && ssh.identity_file.is_none() {
                println!("  ⚠  No SSH key or password configured");
            }
            if let Some(ref identity_file) = ssh.identity_file {
                if !std::path::Path::new(identity_file).exists() {
                    println!("  ⚠  SSH identity file not found: {identity_file}");
                }
            }
            let cmd = build_ssh_command(ssh, &cfg.database.url).unwrap_or_default();
            println!("  Establish tunnel manually:\n    {cmd}");
            failures.push(format!(
                "{env_name}: SSH bastion {bastion_host}:{bastion_port} unreachable and no active PostgreSQL tunnel"
            ));
            continue;
        }

        // Try to establish it
        let bastion = ssh.host.as_deref().unwrap_or("");
        let user = ssh.username.as_deref().unwrap_or("");
        let fwd_host = ssh.forward_host.as_deref().unwrap_or("");
        let fwd_port = ssh.forward_port.unwrap_or(0);

        if bastion.is_empty() || user.is_empty() || fwd_host.is_empty() || fwd_port == 0 {
            let mut missing = vec![];
            if bastion.is_empty() {
                missing.push("host");
            }
            if user.is_empty() {
                missing.push("username");
            }
            if fwd_host.is_empty() {
                missing.push("forward_host");
            }
            if fwd_port == 0 {
                missing.push("forward_port");
            }
            println!(
                "  ⚠  Incomplete SSH config for '{env_name}': missing {}",
                missing.join(", ")
            );
            std::io::stdout().flush()?;
            failures.push(format!(
                "{env_name}: incomplete SSH config, missing {}",
                missing.join(", ")
            ));
            continue;
        }

        print!("  ● Establishing SSH tunnel ({env_name}) ... ");
        std::io::stdout().flush()?;

        // Use the DBeaver-exported local endpoint when available; otherwise keep the
        // historical SafeSelect default to avoid changing existing behavior.
        let tunnel_local_host = ssh.local_host.as_deref().unwrap_or("localhost");
        let tunnel_local_port = ssh.local_port.unwrap_or(15432);

        let use_password = match &ssh.auth_type {
            Some(at) if at == "PASSWORD" => true,
            _ => false,
        };

        // Use a different local port (15432) for forwarding, not the SSH server port
        // Build SSH args
        let mut ssh_args: Vec<String> = vec![
            "-o".into(),
            "ConnectTimeout=5".into(),
            "-o".into(),
            "ExitOnForwardFailure=yes".into(),
            "-o".into(),
            "ServerAliveInterval=15".into(),
            "-o".into(),
            "ServerAliveCountMax=3".into(),
            "-N".into(),
            "-L".into(),
            format!("{tunnel_local_host}:{tunnel_local_port}:{fwd_host}:{fwd_port}"),
            format!("{user}@{bastion}"),
        ];
        if let Some(p) = ssh.port {
            if p != 22 {
                ssh_args.push("-p".into());
                ssh_args.push(p.to_string());
            }
        }

        // Helper: spawn sshpass -p <pw> ssh <ssh_args>
        let spawn_sshpass = |password: &str| -> std::io::Result<std::process::Child> {
            let mut full = vec!["-p".into(), password.to_string(), "ssh".into()];
            full.extend(ssh_args.clone());
            std::process::Command::new("sshpass")
                .args(&full)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
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
                .stderr(std::process::Stdio::null())
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
                    let cmd = build_ssh_command(ssh, &cfg.database.url)
                        .unwrap_or_else(|| "ssh command unavailable".to_string());
                    println!("  Establish it manually:\n    {cmd}");
                    failures.push(format!("{env_name}: SSH password not found in Keychain"));
                    continue;
                }
            };
            match spawn_sshpass(&pw) {
                Ok(c) => c,
                Err(_) => {
                    println!("sshpass not installed.");
                    println!("  Install it: brew install <tap>/sshpass");
                    println!("  Then run:  safeselect check --environment {env_name}");
                    let cmd = build_ssh_command(ssh, &cfg.database.url).unwrap_or_default();
                    println!("  Or establish the tunnel manually:\n    {cmd}");
                    failures.push(format!("{env_name}: sshpass not installed"));
                    continue;
                }
            }
        } else {
            let extra = vec!["-o".into(), "BatchMode=yes".into()];
            let full_cmd = {
                let mut parts = vec!["ssh".to_string()];
                parts.extend(extra.clone());
                parts.extend(ssh_args.clone());
                parts.join(" ")
            };
            match spawn_ssh(extra) {
                Ok(c) => c,
                Err(e) => {
                    println!("FAILED: {e}");
                    println!("  Command: {full_cmd}");
                    println!("  Check that ssh is installed and the identity file is accessible.");
                    let cmd = build_ssh_command(ssh, &cfg.database.url).unwrap_or_default();
                    println!("  Establish it manually:\n    {cmd}");
                    failures.push(format!("{env_name}: failed to spawn ssh: {e}"));
                    continue;
                }
            }
        };

        // Wait briefly: if the bastion/tunnel is down, fail fast like JDBC clients do.
        let tunnel_wait = Duration::from_secs(5);
        let deadline = std::time::Instant::now() + tunnel_wait;
        let mut pg_ok = false;
        while std::time::Instant::now() < deadline {
            pg_ok = check_postgres_endpoint(tunnel_local_host, tunnel_local_port)
                || extract_host_port(&cfg.database.url)
                    .map(|(host, port)| check_postgres_endpoint(&host, port))
                    .unwrap_or(false);
            if pg_ok {
                break;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        if pg_ok {
            println!("OK");
            // Detach child so it survives after we exit
            let _ = std::thread::spawn(move || {
                let _ = child.wait();
            });
        } else {
            let _ = child.kill();
            let _ = child.wait();
            println!("FAILED");
            println!(
                "  PostgreSQL not reachable through SSH tunnel (polled for up to {}s)",
                tunnel_wait.as_secs()
            );
            println!("  Possible causes:");
            println!("    - Database host:port is wrong: {fwd_host}:{fwd_port}");
            println!("    - Database is not running or not accepting connections");
            println!("    - SSH tunnel failed to forward (check bastion logs)");
            let cmd = build_ssh_command(ssh, &cfg.database.url)
                .unwrap_or_else(|| "ssh command unavailable".to_string());
            println!("  Establish tunnel manually for debug:\n    {cmd}");
            failures.push(format!(
                "{env_name}: PostgreSQL not reachable through SSH tunnel after {}s",
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
fn run_checks(repo_root: &std::path::Path, env_names: &[String], verbose: bool) -> Result<()> {
    use std::io::Write;

    println!("── Verification ──────────────────────────────────");
    println!();
    let mut all_ok = true;
    for env_name in env_names {
        print!("  • {env_name} ... ");
        std::io::stdout().flush()?;
        let loader = config::ConfigLoader::new();
        match cmd_check(&loader, repo_root, env_name, verbose) {
            Ok(()) => println!("OK"),
            Err(e) => {
                println!("FAILED");
                println!("    {e}");
                all_ok = false;
            }
        }
    }
    if all_ok {
        println!();
        println!("  ✓ All environments ready.");
    }
    Ok(())
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
    if let Some((host, port)) = extract_host_port(url) {
        return Some((host, port));
    }

    let is_srv = url.starts_with("mongodb+srv://");
    let without_prefix = url
        .strip_prefix("mongodb://")
        .or_else(|| url.strip_prefix("mongodb+srv://"))?;
    let authority = without_prefix.split('/').next()?.rsplit('@').next()?;
    let first_host = authority.split(',').next()?;
    match first_host.split_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None if !is_srv => Some((first_host.to_string(), 27017)),
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
    let pids = String::from_utf8_lossy(&output.stdout);
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

/// Build an SSH command string to establish a tunnel for the given SSH config + DB URL.
/// Returns None if there isn't enough information to build the command.
fn build_ssh_command(ssh: &config::SshConfig, _db_url: &str) -> Option<String> {
    let bastion = ssh.host.as_deref()?;
    let user = ssh.username.as_deref()?;
    let forward_host = ssh.forward_host.as_deref()?;
    let forward_port = ssh.forward_port?;

    let local_host = ssh.local_host.as_deref().unwrap_or("localhost");
    let local_port = ssh.local_port.unwrap_or(15432);

    let mut cmd =
        format!("ssh -L {local_host}:{local_port}:{forward_host}:{forward_port} {user}@{bastion}");

    if let Some(p) = ssh.port {
        if p != 22 {
            cmd.push_str(&format!(" -p {p}"));
        }
    }

    if let Some(ref key) = ssh.identity_file {
        cmd.push_str(&format!(" -i {key}"));
    }

    Some(cmd)
}

fn print_check_verbose(resolved: &config::ResolvedConfig, environment: &str) {
    println!("  · environment={environment}");
    println!("  · jdbc_url={}", resolved.environment.database.url);
    println!("  · db_user={}", resolved.environment.database.username);
    if let Some(secret) = resolved.environment.database.secret.as_ref() {
        match secret.source.as_str() {
            "macos-keychain" => {
                println!(
                    "  · db_secret=macos-keychain:{}",
                    secret.account.as_deref().unwrap_or("unknown")
                );
            }
            "env" => {
                println!(
                    "  · db_secret=env:{}",
                    secret.variable.as_deref().unwrap_or("unknown")
                );
            }
            other => println!("  · db_secret={other}"),
        }
    }
    if let Some(ssh) = resolved.environment.ssh.as_ref() {
        println!(
            "  · ssh_bastion={} ({})",
            ssh.bastion.as_deref().unwrap_or("-"),
            ssh.host.as_deref().unwrap_or("unknown")
        );
        println!(
            "  · ssh_target={}:{}",
            ssh.username.as_deref().unwrap_or("unknown"),
            ssh.port.unwrap_or(22)
        );
        println!(
            "  · ssh_forward={}:{} -> {}:{}",
            ssh.local_host.as_deref().unwrap_or("localhost"),
            ssh.local_port.unwrap_or(DEFAULT_SSH_LOCAL_PORT),
            ssh.forward_host.as_deref().unwrap_or("unknown"),
            ssh.forward_port.unwrap_or(0)
        );
        if let Some(secret_account) = ssh.secret_account.as_deref() {
            println!("  · ssh_secret=macos-keychain:{secret_account}");
        }
    }
}

fn cmd_check(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    verbose: bool,
) -> Result<()> {
    let name = project_display_name(repo_root);
    println!("Checking configuration for {name}/{environment}...");

    let resolved = loader.resolve_local(repo_root, environment)?;

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
            println!("  SSH bastion: {bastion_host}:{bastion_port}");

            let mut postgres_reachable = false;
            if let Some((host, port)) = extract_host_port(&resolved.environment.database.url) {
                postgres_reachable = check_postgres_endpoint(&host, port);
                if postgres_reachable {
                    diagnostics::print(
                        DiagnosticStatus::Ok,
                        DiagnosticCode::PostgresReachable,
                        format!("PostgreSQL reachable at {host}:{port}"),
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
                        format!("Bastion reachable at {bastion_host}:{bastion_port}"),
                    );
                } else {
                    diagnostics::print(
                        DiagnosticStatus::Fail,
                        DiagnosticCode::SshBastionUnreachable,
                        format!("Bastion unreachable at {bastion_host}:{bastion_port} (connect timed out after 3s)"),
                    );
                    if let Some(ref identity_file) = ssh.identity_file {
                        if !std::path::Path::new(identity_file).exists() {
                            diagnostics::print(
                                DiagnosticStatus::Fail,
                                DiagnosticCode::SshIdentityMissing,
                                format!("SSH identity file not found: {identity_file}"),
                            );
                        }
                    }
                    let cmd = build_ssh_command(ssh, &resolved.environment.database.url);
                    if let Some(c) = cmd {
                        println!("  To establish: {c}");
                    }
                    return Err(SafeselectError::Other(
                        format!("SSH bastion {bastion_host}:{bastion_port} not reachable (connect timed out after 3s).")
                    ));
                }
            }

            // 2) Establish SSH tunnel if needed, then check PostgreSQL reachability
            if let Some((host, port)) = extract_host_port(&resolved.environment.database.url) {
                // If SSH is enabled and PostgreSQL is not already reachable, try establishing the tunnel
                let pg_reachable = if postgres_reachable {
                    true
                } else {
                    diagnostics::print(
                        DiagnosticStatus::Info,
                        DiagnosticCode::SshTunnelAttempt,
                        "Establishing SSH tunnel...",
                    );
                    let _ = setup_ssh_tunnels(repo_root, &[environment.to_string()]);
                    check_postgres_endpoint(&host, port)
                };

                match pg_reachable {
                    true => diagnostics::print(
                        DiagnosticStatus::Ok,
                        DiagnosticCode::PostgresReachable,
                        format!("PostgreSQL reachable at {host}:{port}"),
                    ),
                    _ => {
                        diagnostics::print(
                            DiagnosticStatus::Fail,
                            DiagnosticCode::PostgresUnreachable,
                            format!("PostgreSQL unreachable at {host}:{port}"),
                        );
                        println!("  Possible causes:");
                        println!("    - Database host:port is wrong ({host}:{port})");
                        println!("    - Database is not running or not accepting connections");
                        println!("    - SSH tunnel is not established or not forwarding correctly");
                        let cmd = build_ssh_command(ssh, &resolved.environment.database.url);
                        if let Some(c) = cmd {
                            println!("  To establish tunnel: {c}");
                        }
                        return Err(SafeselectError::Other(
                            format!("Cannot reach PostgreSQL at {host}:{port} through SSH tunnel (read timed out after 2s).")
                        ));
                    }
                }
            }
        }
    }

    diagnostics::print(
        DiagnosticStatus::Info,
        DiagnosticCode::SidecarStartAttempt,
        "Attempting sidecar connection...",
    );
    println!(
        "    url={} user={} db={}",
        resolved.environment.database.url,
        resolved.environment.database.username,
        resolved
            .environment
            .database
            .url
            .split('/')
            .last()
            .unwrap_or("?")
    );

    let driver = resolved.driver.as_ref().ok_or_else(|| {
        SafeselectError::Config("check currently supports only JDBC environments".into())
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
    )?;

    sidecar.ping()?;
    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::SidecarJdbcOk,
        "Sidecar JDBC connection OK",
    );

    let result = sidecar.execute("SELECT 1 AS connection_test")?;
    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::QuerySelectOneOk,
        format!(
            "Connection verified: SELECT 1 returned {} row(s)",
            result.row_count
        ),
    );
    diagnostics::print(
        DiagnosticStatus::Ok,
        DiagnosticCode::AllChecksPassed,
        format!("All checks passed for {name}/{environment}"),
    );

    sidecar.shutdown()?;

    Ok(())
}

fn cmd_query(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    sql: Option<&str>,
    verbose: bool,
) -> Result<()> {
    let resolved = loader.resolve_local(repo_root, environment)?;

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
    )?;

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

fn cmd_connectivity_action(
    loader: &ConfigLoader,
    repo_root: &std::path::Path,
    environment: &str,
    action: &str,
) -> Result<()> {
    let name = project_display_name(repo_root);
    let resolved = loader.resolve_local(repo_root, environment)?;

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
    )?;

    match action {
        "disconnect" => {
            sidecar.disconnect()?;
            println!("Disconnected from {name}/{environment}.");
            println!("  The AI agent can reconnect via the 'connect' MCP tool.");
        }
        "connect" => {
            sidecar.connect()?;
            println!("Connected to {name}/{environment}.");
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
    let name = project_display_name(repo_root);
    println!("Reconnecting to {name}/{environment}...");

    let resolved = loader.resolve_local(repo_root, environment)?;

    // Establish SSH tunnel if configured
    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            println!("  ◇ Establishing SSH tunnel...");
            setup_ssh_tunnels(repo_root, &[environment.to_string()])?;
        }
    }

    let driver = resolved.driver.as_ref().ok_or_else(|| {
        SafeselectError::Config("reconnect currently supports only JDBC environments".into())
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
    )?;

    sidecar.ping()?;
    println!("  ✓ Sidecar started and pinged");

    let result = sidecar.execute("SELECT 1 AS connection_test")?;
    println!(
        "  ✓ Connection verified: SELECT 1 returned {} row(s)",
        result.row_count
    );

    sidecar.shutdown()?;
    println!("  ✓ Reconnection successful to {name}/{environment}");

    Ok(())
}

fn cmd_uninstall(force: bool) -> Result<()> {
    if !force {
        println!("This will remove: safeselect binary, global config, data, audit logs, and keychain entries.");
        println!("Local .safeselect/ directories in repos will NOT be removed.");
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

    let bin = dirs::home_dir().map(|h| h.join(".local").join("bin").join("safeselect"));
    if let Some(ref path) = bin {
        if path.exists() {
            std::fs::remove_file(path)?;
            println!("  ✓ Removed {}", path.display());
            removed_anything = true;
        }
    }

    let config_dir = {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        std::path::PathBuf::from(home).join(".config/safeselect")
    };
    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)?;
        println!("  ✓ Removed {}", config_dir.display());
        removed_anything = true;
    }

    if let Some(data_dir) = dirs::data_dir().map(|d| d.join("safeselect")) {
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir)?;
            println!("  ✓ Removed {}", data_dir.display());
            removed_anything = true;
        }
    }

    let audit_dir = dirs::home_dir().map(|h| h.join(".local").join("state").join("safeselect"));
    if let Some(ref path) = audit_dir {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
            println!("  ✓ Removed {}", path.display());
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
            println!("  ✓ Removed backup {}", path.display());
        }
    }

    let keychain_result = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", "safeselect"])
        .output();
    if let Ok(output) = keychain_result {
        if output.status.success() {
            println!("  ✓ Removed macOS Keychain entries for 'safeselect'");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dbeaver_connection() -> dbeaver::DBeaverConnection {
        dbeaver::DBeaverConnection {
            name: "sample".to_string(),
            host: "db.example.com".to_string(),
            port: 5432,
            database: "app".to_string(),
            driver: "postgresql".to_string(),
            username: "postgres".to_string(),
            password: None,
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
        let warning = compass_shared_tunnel_warning(&conn);
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("open the tunnel yourself first"));
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
}
