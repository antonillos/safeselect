#![allow(dead_code)]

mod agents;
mod audit;
mod cli;
mod compose;
mod config;
mod dbeaver;
mod error;
mod mcp;
mod security;
mod sidecar;

use clap::Parser;
use cli::{Cli, Command, ConfigAction, DriverAction, AgentAction};
use config::ConfigLoader;
use error::{Result, SafeselectError};
use sidecar::SidecarProcess;
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
        } => {
            match resolve_project_dir(&loader, project.clone()) {
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
            }
        }
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
        Command::Check {
            project,
            environment,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_check(&loader, &dir, &environment)
        }
        Command::Query {
            project,
            environment,
            sql,
        } => {
            let dir = resolve_project_dir(&loader, project)?;
            cmd_query(&loader, &dir, &environment, sql.as_deref())
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
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
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

    tracing::info!(
        "Starting MCP server (sidecar will start lazily on first query)"
    );

    let db_url = resolved.environment.database.url.clone();
    let db_username = resolved.environment.database.username.clone();
    let db_password = resolved.password.clone();
    let driver_path = resolved.driver.path.clone();
    let driver_class = resolved.driver.class.clone();

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
                        println!(
                            "Config valid: {}/{}",
                            project_display_name(&dir),
                            env
                        );
                    } else {
                        let safeselect_dir = dir.join(".safeselect");
                        if safeselect_dir.join("project.toml").exists() || safeselect_dir.join("environments").is_dir() {
                            println!(
                                "Config valid: {}",
                                project_display_name(&dir)
                            );
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
                            println!("Use --environment <name> to validate a specific environment.");
                            let envs_dir = dir.join(".safeselect").join("environments");
                            if envs_dir.is_dir() {
                                let mut entries: Vec<_> = std::fs::read_dir(&envs_dir)
                                    .into_iter()
                                    .flatten()
                                    .flatten()
                                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "toml"))
                                    .filter_map(|e| e.path().file_stem().and_then(|s| s.to_str().map(String::from)))
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
                    loader.find_local_project(&cwd).ok_or_else(|| {
                        SafeselectError::LocalProjectNotFound(cwd)
                    })?
                }
            };
            let resolved = loader.resolve_local(&dir, &environment)?;
            let name = project_display_name(&dir);
            println!("Project: {name}");
            println!("Environment: {environment}");
            println!("Driver: {} ({})", resolved.driver.vendor, resolved.driver.class);
            println!("JDBC URL: {}", resolved.environment.database.url);
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
            println!("Max result bytes: {}", resolved.project.limits.max_result_bytes);
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
        ConfigAction::RenameEnvironment {
            old,
            new,
            project,
        } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader.find_local_project(&cwd).ok_or_else(|| {
                        SafeselectError::LocalProjectNotFound(cwd)
                    })?
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
                    loader.find_local_project(&cwd).ok_or_else(|| {
                        SafeselectError::LocalProjectNotFound(cwd)
                    })?
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
                env_config.database.secret.map(|s| (s.source, s.account, s.variable))
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
                    loader.find_local_project(&cwd).ok_or_else(|| {
                        SafeselectError::LocalProjectNotFound(cwd)
                    })?
                }
            };

            let env_file = dir.join(".safeselect").join("environments").join(format!("{environment}.toml"));
            if !env_file.exists() {
                return Err(SafeselectError::EnvironmentNotFound(
                    environment.clone(),
                    env_file.display().to_string(),
                ));
            }

            let project_name = project_display_name(&dir);
            let account = format!("{project_name}/{environment}");

            let pw = match password {
                Some(p) => p,
                None => {
                    inquire::Password::new(&format!("Password for '{project_name}/{environment}'"))
                        .without_confirmation()
                        .prompt()
                        .map_err(|e| SafeselectError::Other(format!("Failed to read password: {e}")))?
                }
            };

            compose::store_password_in_keychain(&account, &pw)?;
            println!("  ✓ Password stored in Keychain ({account})");

            let mut content = std::fs::read_to_string(&env_file)?;
            if content.trim().ends_with("]") {
                content.push('\n');
            }
            content.push_str(&format!(
                "\n[database.secret]\nsource = \"macos-keychain\"\nservice = \"safeselect\"\naccount = \"{account}\"\n"
            ));
            std::fs::write(&env_file, content)?;
            println!("  ✓ Updated {}", env_file.display());
            println!("\nDone. Run: safeselect check --environment {environment}");
            Ok(())
        }
        ConfigAction::Reset { project } => {
            let dir = match project {
                Some(d) => d,
                None => {
                    let cwd = std::env::current_dir()?;
                    loader.find_local_project(&cwd).ok_or_else(|| {
                        SafeselectError::LocalProjectNotFound(cwd)
                    })?
                }
            };
            reset_project_config(&dir)
        }
    }
}

fn reset_project_config(repo_root: &Path) -> Result<()> {
    let safeselect_dir = repo_root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");

    if !env_dir.is_dir() {
        println!("  ◉ No environments to reset.");
        return Ok(());
    }

    let ans = inquire::Confirm::new("This will remove all environments and their keychain entries. Continue?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    if !ans {
        println!("Cancelled.");
        return Ok(());
    }

    let mut removed = 0u32;
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
                    }
                }
                let _ = std::fs::remove_file(&path);
                removed += 1;
            }
        }
    }

    if removed > 0 {
        println!("  ✓ Removed {removed} environment(s)");
    }

    // Reset generated_by in project.toml
    let project_file = safeselect_dir.join("project.toml");
    if project_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&project_file) {
            if let Ok(mut proj) = toml::from_str::<config::ProjectConfig>(&content) {
                proj.generated_by = Some(env!("CARGO_PKG_VERSION").to_string());
                if let Ok(new_content) = toml::to_string_pretty(&proj) {
                    let _ = std::fs::write(&project_file, new_content);
                }
            }
        }
    }

    if removed > 0 {
        println!("\nReset complete. Re-import with:");
        println!("  safeselect import-dbeaver <export.zip>");
        println!("  safeselect import-compose");
    } else {
        println!("  ◉ No environment files found.");
    }

    Ok(())
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
            let content = toml::to_string(&config)
                .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
            std::fs::write(&driver_file, content)?;

            println!("Driver '{vendor}' registered at {}", driver_file.display());
            println!("SHA-256: {checksum}");

            Ok(())
        }
        DriverAction::List => {
            let drivers = loader.list_drivers()?;
            if drivers.is_empty() {
                println!("No drivers registered in {}", loader.drivers_dir().display());
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
            let content = toml::to_string(&config)
                .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
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
                let status = if client.detected {
                    "✓"
                } else {
                    "✗"
                };
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
            let entry_name = match name {
                Some(n) => n,
                None => {
                    let root = project_dir.ok_or_else(|| SafeselectError::Other(
                        "no .safeselect/ found; use --project or --name to specify".into()
                    ))?;
                    format!("{}-{}", project_display_name(&root), environment)
                }
            };
            agents::install_entry(&client, &environment, &entry_name, repo_root.as_deref(), Some(loader.config_dir()))
        }
        AgentAction::Uninstall { client, name } => {
            agents::uninstall_entry(&client, &name)
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
            if !content.lines().any(|l| l.trim() == ".safeselect/" || l.trim() == ".safeselect") {
                println!("  ⚠  .safeselect/ not found in .gitignore — consider adding it");
            }
        }
    } else {
        println!("  ⚠  No .gitignore found at {} — consider adding .safeselect/ to it", gitignore.display());
    }
}

fn write_project_toml(safeselect_dir: &Path) -> Result<()> {
    let project_file = safeselect_dir.join("project.toml");
    let config = config::ProjectConfig::default();
    let toml_str = toml::to_string_pretty(&config)
        .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(&project_file, toml_str)?;
    Ok(())
}

fn update_generated_by(safeselect_dir: &Path) -> Result<()> {
    let project_file = safeselect_dir.join("project.toml");
    if !project_file.exists() {
        return write_project_toml(safeselect_dir);
    }
    let content = std::fs::read_to_string(&project_file)?;
    let mut proj: config::ProjectConfig = toml::from_str(&content)
        .map_err(|e| SafeselectError::Config(format!("invalid project.toml: {e}")))?;
    proj.generated_by = Some(env!("CARGO_PKG_VERSION").to_string());
    let new_content = toml::to_string_pretty(&proj)
        .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    std::fs::write(&project_file, new_content)?;
    Ok(())
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
fn prompt_ssh_config(
    conn: &dbeaver::DBeaverConnection,
    project_name: &str,
    env_name: &str,
) -> Result<config::SshConfig> {
    let default_host = conn.ssh_host.as_deref().unwrap_or("");
    let default_user = conn.ssh_user.as_deref().unwrap_or("");
    let default_key = conn.ssh_key_file.as_deref().unwrap_or("");
    let default_auth = conn.ssh_auth_type.as_deref().unwrap_or("KEY");

    println!();
    println!("── SSH Configuration ({env_name}) ───────────────────");
    println!();

    let ans = inquire::Confirm::new("Configure SSH tunnel now?")
        .with_default(true)
        .prompt()
        .map_err(|e| SafeselectError::Other(format!("Cancelled: {e}")))?;

    if !ans {
        // Store minimal SSH config with whatever DBeaver extracted
        return Ok(config::SshConfig {
            enabled: true,
            host: conn.ssh_host.clone(),
            port: conn.ssh_port,
            username: conn.ssh_user.clone(),
            identity_file: conn.ssh_key_file.clone(),
            known_hosts: None,
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

    let auth_method = inquire::Select::new(
        "  Authentication method:",
        vec!["Key file", "Password"],
    )
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
            (if kf.is_empty() { None } else { Some(kf) }, Some("KEY".into()))
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
        host: Some(host),
        port: Some(port),
        username: Some(user),
        identity_file: key_file,
        known_hosts: None,
        forward_host: Some(conn.host.clone()),
        forward_port: Some(conn.port),
        auth_type,
    })
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

    let project_name = project_display_name(&cwd);
    let mut imported_envs: Vec<(String, bool)> = vec![];

    for (idx, env_name) in &to_import {
        let conn = &connections[*idx];

        let has_ssh = conn.ssh_host.is_some();

        let ssh = if has_ssh {
            Some(prompt_ssh_config(conn, &project_name, env_name)?)
        } else {
            None
        };

        let requires_ssl = conn.host.contains(".azure.com") || conn.host.contains(".database.azure");
        let ssl_param = if requires_ssl { "?sslmode=require" } else { "" };

        let url = if has_ssh {
            if let Some(lp) = conn.ssh_local_port {
                format!("jdbc:postgresql://{lh}:{lp}/{db}{ssl_param}",
                    lh = conn.ssh_local_host.as_deref().unwrap_or("localhost"),
                    db = conn.database)
            } else {
                format!("jdbc:postgresql://{h}:{p}/{db}{ssl_param}",
                    h = conn.ssh_host.as_deref().unwrap_or("localhost"),
                    p = conn.ssh_port.unwrap_or(5432),
                    db = conn.database)
            }
        } else {
            format!("jdbc:postgresql://{}:{}/{}{ssl_param}",
                conn.host, conn.port, conn.database)
        };

        let (secret, has_secret) = if let Some(ref pw) = conn.password {
            if !pw.is_empty() {
                let account = format!("{project_name}/{env_name}");
                compose::store_password_in_keychain(&account, pw)?;
                (Some(config::SecretConfig {
                    source: "macos-keychain".to_string(),
                    service: Some("safeselect".to_string()),
                    account: Some(account),
                    variable: None,
                }), true)
            } else {
                (None, false)
            }
        } else {
            (None, false)
        };

        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                driver: conn.driver.clone(),
                url,
                username: conn.username.clone(),
                secret,
            },
            tls: None,
            ssh,
            limits: config::LimitsOverride::default(),
        };
        let env_toml = toml::to_string_pretty(&env_config)
            .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        let env_file = env_dir.join(format!("{env_name}.toml"));
        if !env_file.exists() {
            std::fs::write(&env_file, env_toml)?;
            imported_envs.push((env_name.clone(), has_secret));
        }
    }

    // Step 4: print summary
    if imported_envs.is_empty() {
        println!("  ◉ All environments already exist.");
    } else {
        println!();
        println!("── Import Complete ──────────────────────────────");
        println!();
        println!("  ✓ {} environment(s) added", imported_envs.len());
        check_gitignore(&cwd);
    }

    // Step 5: shared helpers (driver, passwords, verify)
    let mut env_names: Vec<String> = imported_envs.iter().map(|(n, _)| n.clone()).collect();
    // Also include already-existing selected environments
    for (_, name) in &to_import {
        if !env_names.contains(name) {
            env_names.push(name.clone());
        }
    }
    setup_driver_if_missing()?;
    setup_passwords_for_missing(&cwd, &env_names)?;
    run_checks(&cwd, &env_names)?;
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
        .flat_map(|(label, conns)| {
            conns.into_iter().map(move |c| (label.clone(), c))
        })
        .collect();

    if all_connections.is_empty() {
        println!("No PostgreSQL services found in docker-compose files.");
        return Ok(());
    }

    let mut to_import: Vec<compose::ComposeConnection> = if non_interactive {
        all_connections
            .iter()
            .map(|(_, c)| c)
            .cloned()
            .collect()
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

    if result.created > 0 {
        println!();
        println!("── Import Complete ──────────────────────────────");
        println!();
        println!("  ✓ {} environment(s) added", to_import.len());
        check_gitignore(dest_dir);
    } else {
        println!("  ◉ All environments already exist.");
    }

    let env_names: Vec<String> = to_import.iter().map(|c| c.env_name.clone()).collect();
    setup_driver_if_missing()?;
    setup_passwords_for_missing(dest_dir, &env_names)?;
    run_checks(dest_dir, &env_names)?;

    Ok(())
}

fn import_selected_connections(connections: &[compose::ComposeConnection]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = project_display_name(&cwd);
    let result = compose::write_config_files(&cwd, connections, &name)?;
    update_generated_by(&cwd.join(".safeselect"))?;

    if result.created > 0 {
        println!(
            "\nImport complete. {} environment(s) added to .safeselect/.",
            connections.len()
        );
        check_gitignore(&cwd);
    } else {
        println!("All environments already exist. Nothing to import.");
    }

    let env_names: Vec<String> = connections.iter().map(|c| c.env_name.clone()).collect();
    setup_driver_if_missing()?;
    setup_passwords_for_missing(&cwd, &env_names)?;
    run_checks(&cwd, &env_names)?;

    Ok(())
}

fn setup_driver_if_missing() -> Result<()> {
    let loader = config::ConfigLoader::new();
    if !loader.list_drivers().map(|d| d.is_empty()).unwrap_or(true) {
        return Ok(());
    }
    println!();
    println!("── JDBC Driver ──────────────────────────────────");
    println!();
    cmd_driver(&loader, DriverAction::Download { vendor: "postgresql".into() })?;
    Ok(())
}

fn setup_passwords_for_missing(repo_root: &std::path::Path, env_names: &[String]) -> Result<()> {
    for env_name in env_names {
        let env_file = repo_root
            .join(".safeselect")
            .join("environments")
            .join(format!("{env_name}.toml"));
        if !env_file.exists() { continue; }
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

        let repo_name = project_display_name(repo_root);
        let account = format!("{repo_name}/{env_name}");
        let pw = rpassword::prompt_password(format!(
            "  Password for '{repo_name}/{env_name}': "
        ))?;
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
                if in_secret { continue; }
                buf.push_str(line);
                buf.push('\n');
            }
            while buf.ends_with('\n') { buf.pop(); }
            buf.push('\n');
            buf.push('\n');
            buf.push_str(&secret_section);
            buf
        } else {
            let mut c = content;
            if !c.ends_with('\n') { c.push('\n'); }
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
fn setup_ssh_tunnels(repo_root: &Path, env_names: &[String]) -> Result<Vec<u32>> {
    use std::io::Write;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    let mut started_pids: Vec<u32> = vec![];

    for env_name in env_names {
        let env_file = repo_root
            .join(".safeselect")
            .join("environments")
            .join(format!("{env_name}.toml"));
        if !env_file.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&env_file) {
            Ok(c) => c,
            _ => continue,
        };
        let cfg: config::EnvironmentConfig = match toml::from_str(&content) {
            Ok(c) => c,
            _ => continue,
        };
        let ssh = match &cfg.ssh {
            Some(s) if s.enabled => s,
            _ => continue,
        };

        let local = match extract_host_port(&cfg.database.url) {
            Some(h) => h,
            None => continue,
        };

        // Kill stale tunnel on the local port (macOS)
        if kill_process_on_port(local.1) {
            print!("  ◇ Killed stale tunnel on port {} ({env_name})", local.1);
            std::io::stdout().flush()?;
            std::thread::sleep(Duration::from_millis(500));
        }

        // Check if already active (might be a fresh user-established tunnel)
        let addr = format!("{}:{}", local.0, local.1)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next());

        let active = addr.as_ref().is_some_and(|a| {
            std::net::TcpStream::connect_timeout(a, Duration::from_secs(1)).is_ok()
        });

        if active {
            print!("  ◉ SSH tunnel active ({env_name})");
            std::io::stdout().flush()?;
            continue;
        }

        // Try to establish it
        let bastion = ssh.host.as_deref().unwrap_or("");
        let user = ssh.username.as_deref().unwrap_or("");
        let fwd_host = ssh.forward_host.as_deref().unwrap_or("");
        let fwd_port = ssh.forward_port.unwrap_or(0);

        if bastion.is_empty() || user.is_empty() || fwd_host.is_empty() || fwd_port == 0 {
            print!("  ⚠  Incomplete SSH config for '{env_name}'");
            std::io::stdout().flush()?;
            continue;
        }

        print!("  ● Establishing SSH tunnel ({env_name}) ... ");
        std::io::stdout().flush()?;

        let use_password = match &ssh.auth_type {
            Some(at) if at == "PASSWORD" => true,
            _ => false,
        };

        let result = if use_password {
            // Password auth: try sshpass with password from Keychain
            let ssh_acct = format!("{}/{env_name}/ssh", project_display_name(repo_root));
            let pw = match compose::read_password_from_keychain(&ssh_acct) {
                Ok(p) => p,
                Err(_) => {
                    println!("NO PASSWORD");
                    let cmd = build_ssh_command(ssh, &cfg.database.url)
                        .unwrap_or_else(|| "ssh command unavailable".to_string());
                    println!("  Establish it manually:");
                    println!("    {cmd}");
                    continue;
                }
            };

            let mut pass_args = vec![
                "-p".to_string(),
                pw,
                "ssh".to_string(),
                "-o".to_string(),
                "ConnectTimeout=5".to_string(),
                "-N".to_string(),
                "-f".to_string(),
                "-L".to_string(),
                format!("{}:{}:{}", local.1, fwd_host, fwd_port),
                format!("{user}@{bastion}"),
            ];
            if let Some(p) = ssh.port {
                if p != 22 {
                    pass_args.push("-p".into());
                    pass_args.push(p.to_string());
                }
            }

            match std::process::Command::new("sshpass").args(&pass_args).output() {
                Ok(out) if out.status.success() => Ok(out),
                Ok(_) => {
                    println!("FAILED (sshpass)");
                    let cmd = build_ssh_command(ssh, &cfg.database.url)
                        .unwrap_or_else(|| "ssh command unavailable".to_string());
                    println!("  Establish it manually:");
                    println!("    {cmd}");
                    continue;
                }
                Err(_) => {
                    println!("sshpass not installed");
                    let cmd = build_ssh_command(ssh, &cfg.database.url)
                        .unwrap_or_else(|| "ssh command unavailable".to_string());
                    println!("  To establish the tunnel manually:");
                    println!("    {cmd}");
                    continue;
                }
            }
        } else {
            // Key file auth with BatchMode
            let mut ssh_args: Vec<String> = vec![
                "-o".into(),
                "BatchMode=yes".into(),
                "-o".into(),
                "ConnectTimeout=5".into(),
                "-N".into(),
                "-f".into(),
                "-L".into(),
                format!("{}:{}:{}", local.1, fwd_host, fwd_port),
                format!("{user}@{bastion}"),
            ];
            if let Some(p) = ssh.port {
                if p != 22 {
                    ssh_args.push("-p".into());
                    ssh_args.push(p.to_string());
                }
            }
            if let Some(ref k) = ssh.identity_file {
                if Path::new(k).exists() {
                    ssh_args.push("-i".into());
                    ssh_args.push(k.clone());
                }
            }
            std::process::Command::new("ssh").args(&ssh_args).output()
        };

        match result {
            Ok(out) if out.status.success() => {
                std::thread::sleep(Duration::from_secs(2));
                let ok = addr.as_ref().is_some_and(|a| {
                    std::net::TcpStream::connect_timeout(a, Duration::from_secs(2)).is_ok()
                });
                if ok {
                    println!("OK");
                    // Try to get PID from ssh -f (it might still be starting)
                    if let Ok(pid_str) = std::str::from_utf8(&out.stdout) {
                        if let Some(pid_str) = pid_str.lines().next() {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                started_pids.push(pid);
                            }
                        }
                    }
                } else {
                    println!("FAILED");
                    let cmd = build_ssh_command(ssh, &cfg.database.url)
                        .unwrap_or_else(|| "ssh command unavailable".to_string());
                    println!("  Establish it manually:");
                    println!("    {cmd}");
                }
            }
            Ok(out) => {
                println!("FAILED");
                let stderr = String::from_utf8_lossy(&out.stderr);
                let err_line = stderr.lines().next().unwrap_or("unknown error");
                let cmd = build_ssh_command(ssh, &cfg.database.url)
                    .unwrap_or_else(|| "ssh command unavailable".to_string());
                println!("  {err_line}");
                println!("  Establish it manually:");
                println!("    {cmd}");
            }
            Err(e) => {
                println!("FAILED");
                println!("  {e}");
                let cmd = build_ssh_command(ssh, &cfg.database.url)
                    .unwrap_or_else(|| "ssh command unavailable".to_string());
                println!("  Establish it manually:");
                println!("    {cmd}");
            }
        }
    }
    Ok(started_pids)
}

/// Run `safeselect check` for each environment and report results.
fn run_checks(repo_root: &std::path::Path, env_names: &[String]) -> Result<()> {
    use std::io::Write;

    // Try to auto-establish SSH tunnels
    let _ssh_pids = setup_ssh_tunnels(repo_root, env_names)?;

    // Pre-scan for SSH bastions (only warn if still not active)
    let ssh_envs: Vec<&String> = env_names
        .iter()
        .filter(|env| {
            let env_file = repo_root
                .join(".safeselect")
                .join("environments")
                .join(format!("{env}.toml"));
            std::fs::read_to_string(&env_file)
                .ok()
                .and_then(|c| toml::from_str::<config::EnvironmentConfig>(&c).ok())
                .and_then(|cfg| cfg.ssh)
                .map(|s| s.enabled)
                .unwrap_or(false)
        })
        .collect();

    if !ssh_envs.is_empty() {
        use std::net::ToSocketAddrs;
        let mut still_down = vec![];
        for env in &ssh_envs {
            let env_file = repo_root
                .join(".safeselect")
                .join("environments")
                .join(format!("{env}.toml"));
            let active = std::fs::read_to_string(&env_file).ok().and_then(|c| {
                toml::from_str::<config::EnvironmentConfig>(&c).ok()
            }).and_then(|cfg| {
                extract_host_port(&cfg.database.url)
            }).map(|(h, p)| {
                format!("{h}:{p}").to_socket_addrs().ok()
                    .and_then(|mut a| a.next())
                    .map(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(2)).is_ok())
                    .unwrap_or(false)
            }).unwrap_or(false);
            if !active {
                still_down.push(env.as_str());
            }
        }

        if !still_down.is_empty() {
            println!();
            println!("── SSH Bastions ─────────────────────────────────");
            println!();
            for env_name in &still_down {
                let env_file = repo_root
                    .join(".safeselect")
                    .join("environments")
                    .join(format!("{env_name}.toml"));
                if let Ok(content) = std::fs::read_to_string(&env_file) {
                    if let Ok(cfg) = toml::from_str::<config::EnvironmentConfig>(&content) {
                        if let Some(ref ssh) = cfg.ssh {
                            if ssh.enabled {
                                let host = ssh.host.as_deref().unwrap_or("unknown");
                                let port = ssh.port.unwrap_or(22);
                                println!("  ⚠  '{env_name}' requires SSH tunnel ({host}:{port})");
                                let cmd = build_ssh_command(ssh, &cfg.database.url)
                                    .unwrap_or_else(|| "ssh command unavailable".to_string());
                                println!("     {cmd}");
                            }
                        }
                    }
                }
            }
            println!();
        }
    }

    println!("── Verification ──────────────────────────────────");
    println!();
    let mut all_ok = true;
    for env_name in env_names {
        print!("  • {env_name} ... ");
        std::io::stdout().flush()?;
        let loader = config::ConfigLoader::new();
        match cmd_check(&loader, repo_root, env_name) {
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

    let auto_import: Vec<compose::ComposeConnection> = dirs
        .into_iter()
        .flat_map(|(_, conns)| conns)
        .collect();

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

fn extract_host_port(url: &str) -> Option<(String, u16)> {
    let without_prefix = url.strip_prefix("jdbc:postgresql://")?;
    let host_port = without_prefix.split('/').next()?;
    let (host, port_str) = host_port.split_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    Some((host.to_string(), port))
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
fn build_ssh_command(ssh: &config::SshConfig, db_url: &str) -> Option<String> {
    let bastion = ssh.host.as_deref()?;
    let user = ssh.username.as_deref()?;
    let forward_host = ssh.forward_host.as_deref()?;
    let forward_port = ssh.forward_port?;

    let local_port = extract_host_port(db_url)
        .map(|(_, p)| p)
        .or(ssh.port)?;

    let mut cmd = format!(
        "ssh -L {local_port}:{forward_host}:{forward_port} {user}@{bastion}"
    );

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

fn cmd_check(loader: &ConfigLoader, repo_root: &std::path::Path, environment: &str) -> Result<()> {
    let name = project_display_name(repo_root);
    println!("Checking configuration for {name}/{environment}...");

    let resolved = loader.resolve_local(repo_root, environment)?;

    println!("  ✓ Config valid");
    println!("  ✓ Driver '{}' found and checksum OK", resolved.driver.vendor);
    println!("  ✓ Secret resolved");

    if let Some(ref ssh) = resolved.environment.ssh {
        if ssh.enabled {
            println!("  SSH bastion: {}:{}", ssh.host.as_deref().unwrap_or("unknown"), ssh.port.unwrap_or(22));
            if let Some((host, port)) = extract_host_port(&resolved.environment.database.url) {
                use std::net::ToSocketAddrs;
                let addr = format!("{host}:{port}")
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .ok_or_else(|| SafeselectError::Other(format!(
                        "Cannot resolve {host}:{port}"
                    )))?;
                match std::net::TcpStream::connect_timeout(
                    &addr,
                    std::time::Duration::from_secs(5),
                ) {
                    Ok(_) => println!("  ✓ Tunnel reachable at {host}:{port}"),
                    Err(_) => {
                        println!("  ✗ Cannot reach {host}:{port}");
                        let ssh_cmd = build_ssh_command(ssh, &resolved.environment.database.url);
                        if let Some(cmd) = ssh_cmd {
                            println!("  To establish the tunnel:");
                            println!("    {cmd}");
                        }
                        return Err(SafeselectError::Other(format!(
                            "SSH tunnel not detected at {host}:{port}. Establish it with the command above."
                        )));
                    }
                }
            }
        }
    }

    println!("  Attempting sidecar connection...");

    let mut sidecar = SidecarProcess::start(
        &resolved.driver.path,
        &resolved.driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
    )?;

    sidecar.ping()?;
    println!("  ✓ Sidecar JDBC connection OK");
    println!("  ✓ All checks passed for {name}/{environment}");

    sidecar.shutdown()?;

    Ok(())
}

fn cmd_query(loader: &ConfigLoader, repo_root: &std::path::Path, environment: &str, sql: Option<&str>) -> Result<()> {
    let resolved = loader.resolve_local(repo_root, environment)?;

    let sql = match sql {
        Some(s) => s.to_string(),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            let trimmed = buf.trim().to_string();
            if trimmed.is_empty() {
                return Err(SafeselectError::Other("No SQL provided. Use --sql or pipe a query.".into()));
            }
            trimmed
        }
    };

    let mut sidecar = SidecarProcess::start(
        &resolved.driver.path,
        &resolved.driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
    )?;

    let security = security::SecurityEngine::new(resolved.project.security.clone(), resolved.project.limits.clone());
    security.validate(&sql)?;

    let result = sidecar.execute(&sql)?;
    security.check_result_size(result.row_count, result.byte_count)?;

    sidecar.shutdown()?;

    if result.columns.is_empty() {
        println!("Query executed. {} rows affected.", result.row_count);
        return Ok(());
    }

    let col_widths: Vec<usize> = result.columns.iter()
        .enumerate()
        .map(|(i, col)| {
            let max_data = result.rows.iter()
                .filter_map(|row| row.get(i))
                .filter_map(|v| v.as_str())
                .map(|s| s.len())
                .max()
                .unwrap_or(0);
            col.len().max(max_data).min(80)
        })
        .collect();

    let print_row = |cells: &[String]| {
        let parts: Vec<String> = cells.iter().enumerate()
            .map(|(i, cell)| {
                let width = col_widths.get(i).copied().unwrap_or(20);
                format!(" {:width$} ", cell, width = width)
            })
            .collect();
        println!("|{}|", parts.join("|"));
    };

    let separator = || {
        let parts: Vec<String> = col_widths.iter()
            .map(|w| format!("-{:-<width$}-", "", width = w))
            .collect();
        println!("|{}|", parts.join("+"));
    };

    separator();
    print_row(&result.columns);
    separator();
    for row in &result.rows {
        let cells: Vec<String> = row.iter()
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
    println!("({} rows, {} bytes)", result.row_count, result.byte_count);

    Ok(())
}

fn cmd_connectivity_action(loader: &ConfigLoader, repo_root: &std::path::Path, environment: &str, action: &str) -> Result<()> {
    let name = project_display_name(repo_root);
    let resolved = loader.resolve_local(repo_root, environment)?;

    let mut sidecar = SidecarProcess::start(
        &resolved.driver.path,
        &resolved.driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
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

    let bin = dirs::home_dir()
        .map(|h| h.join(".local").join("bin").join("safeselect"));
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

    let audit_dir = dirs::home_dir()
        .map(|h| h.join(".local").join("state").join("safeselect"));
    if let Some(ref path) = audit_dir {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
            println!("  ✓ Removed {}", path.display());
            removed_anything = true;
        }
    }

    let backup_paths = [
        dirs::home_dir().map(|h| h.join("Library/Application Support/opencode/opencode.json.safeselect.bak")),
        dirs::config_dir().map(|d| d.join("opencode/opencode.json.safeselect.bak")),
        Some(std::path::PathBuf::from("~/.config/opencode/opencode.json.safeselect.bak")),
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
