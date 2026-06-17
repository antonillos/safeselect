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
        Command::ImportDbeaver { path } => cmd_import_dbeaver(&path),
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
        "Starting sidecar with driver '{}'",
        resolved.driver.vendor
    );

    let idle_timeout = resolved.environment.limits.idle_timeout_seconds.unwrap_or(0);
    let sidecar = SidecarProcess::start_with_timeout(
        &resolved.driver.path,
        &resolved.driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
        idle_timeout,
    )?;

    tracing::info!("Sidecar ready, starting MCP server");

    let mut server = mcp::McpServer::new(
        sidecar,
        resolved.project,
        resolved.environment,
        &name,
        environment,
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
    }
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
            println!();
            println!("Next step:");
            println!("  safeselect check --environment <name>");

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

fn cmd_import_dbeaver(path: &str) -> Result<()> {
    let zip_path = std::path::Path::new(path);
    if !zip_path.exists() {
        return Err(SafeselectError::Other(format!(
            "File not found: {path}"
        )));
    }

    let connections = dbeaver::import_zip(zip_path)?;

    if connections.is_empty() {
        println!("No database connections found in the DBeaver export.");
        return Ok(());
    }

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

    let cwd = std::env::current_dir()?;
    let safeselect_dir = cwd.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir)?;

    let mut created_any = false;
    let mut imported_envs: Vec<(String, bool)> = vec![]; // (env_name, has_secret)

    let project_config = config::ProjectConfig::default();
    let project_toml = toml::to_string_pretty(&project_config)
        .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
    let project_file = safeselect_dir.join("project.toml");
    if !project_file.exists() {
        std::fs::write(&project_file, project_toml)?;
        println!("  ✔ Created {}", project_file.display());
        created_any = true;
    }

    let project_name = project_display_name(&cwd);

    for label in &selected {
        let conn = &connections[label.0];

        let env = conn
            .name
            .split_once(" (")
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or("default")
            .to_lowercase()
            .replace(' ', "-")
            .replace("--", "-");

        let ssh = conn.ssh_host.as_ref().map(|h| config::SshConfig {
            enabled: true,
            host: Some(h.clone()),
            port: conn.ssh_port,
            username: conn.ssh_user.clone(),
            identity_file: None,
            known_hosts: None,
        });

        let (secret, has_secret) = if let Some(ref pw) = conn.password {
            if !pw.is_empty() {
                let account = format!("{project_name}/{env}");
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
                url: format!("jdbc:postgresql://{}:{}/{}", conn.host, conn.port, conn.database),
                username: conn.username.clone(),
                secret,
            },
            tls: None,
            ssh,
            limits: config::LimitsOverride::default(),
        };
        let env_toml = toml::to_string_pretty(&env_config)
            .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        let env_file = env_dir.join(format!("{env}.toml"));
        if !env_file.exists() {
            std::fs::write(&env_file, env_toml)?;
            println!("  ✔ Created {} → {}", env_file.display(), conn.name);
            created_any = true;
            imported_envs.push((env, has_secret));
        }
    }

    if created_any {
        println!(
            "\nImport complete. {} environment(s) added to .safeselect/.",
            selected.len()
        );

        let mut no_password: Vec<(String, String)> = Vec::new();
        let env_names: Vec<String> = imported_envs.iter().map(|(n, _)| n.clone()).collect();
        for (env, has) in &imported_envs {
            if !has {
                no_password.push((env.clone(), format!("{project_name}/{env}")));
            }
        }
        let result = compose::ImportResult { created: selected.len(), env_names, no_password };
        print_import_next_steps(&project_name, &result);
        check_gitignore(&cwd);
    } else {
        println!("All environments already exist. Nothing to import.");
    }
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

    if result.created > 0 {
        println!(
            "\nImport complete. {} environment(s) added to {}",
            to_import.len(),
            dest_dir.join(".safeselect").display()
        );
        print_import_next_steps(&project_name, &result);
        check_gitignore(dest_dir);
    } else {
        println!("All environments already exist. Nothing to import.");
        print_driver_hint_once();
    }

    Ok(())
}

fn import_selected_connections(connections: &[compose::ComposeConnection]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = project_display_name(&cwd);
    let result = compose::write_config_files(&cwd, connections, &name)?;

    if result.created > 0 {
        println!(
            "\nImport complete. {} environment(s) added to .safeselect/.",
            connections.len()
        );
        print_import_next_steps(&name, &result);
        check_gitignore(&cwd);
    } else {
        println!("All environments already exist. Nothing to import.");
        print_driver_hint_once();
    }

    Ok(())
}

fn no_driver_exists() -> bool {
    let loader = config::ConfigLoader::new();
    loader.list_drivers().map(|d| d.is_empty()).unwrap_or(true)
}

fn print_driver_hint_once() {
    if no_driver_exists() {
        println!();
        println!("  No JDBC driver found. Install one:");
        println!("    safeselect driver download --vendor postgresql");
    }
}

fn print_import_next_steps(_project_name: &str, result: &compose::ImportResult) {
    let needs_password = !result.no_password.is_empty();
    let needs_driver = no_driver_exists();

    println!();
    for env_name in &result.env_names {
        if needs_password && result.no_password.iter().any(|(n, _)| n == env_name) {
            println!(
                "  {env_name}:  safeselect config set-password --environment {env_name}"
            );
        } else if needs_password {
            println!("  {env_name}:  safeselect check --environment {env_name}");
        } else {
            println!("  {env_name}:  safeselect check --environment {env_name}");
        }
    }

    if needs_driver {
        println!();
        println!("  No JDBC driver found. Install one:");
        println!("    safeselect driver download --vendor postgresql");
    }

    println!();
    println!("Then start the MCP server:");
    for env_name in &result.env_names {
        println!("  safeselect serve --environment {env_name}");
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
            println!("  Ensure tunnel is active before connecting");
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
