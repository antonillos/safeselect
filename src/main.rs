#![allow(dead_code)]

mod agents;
mod audit;
mod cli;
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
        } => cmd_serve(&loader, &project, &environment),
        Command::Config { action } => cmd_config(&loader, action),
        Command::Driver { action } => cmd_driver(&loader, action),
        Command::Agent { action } => cmd_agent(action),
        Command::ImportDbeaver { path } => cmd_import_dbeaver(&path),
        Command::Check {
            project,
            environment,
        } => cmd_check(&loader, &project, &environment),
        Command::Query {
            project,
            environment,
            sql,
        } => cmd_query(&loader, &project, &environment, sql.as_deref()),
        Command::Uninstall { force } => cmd_uninstall(force),
    }
}

fn cmd_serve(loader: &ConfigLoader, project: &str, environment: &str) -> Result<()> {
    tracing::info!("Loading config for {project}/{environment}");

    let resolved = loader.resolve(project, environment)?;

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

    let sidecar = SidecarProcess::start(
        &resolved.driver.path,
        &resolved.driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
    )?;

    tracing::info!("Sidecar ready, starting MCP server");

    let mut server = mcp::McpServer::new(
        sidecar,
        resolved.project,
        resolved.environment,
        project,
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
            if let Some(ref proj) = project {
                let projects = loader.list_projects()?;
                if !projects.contains(proj) {
                    return Err(SafeselectError::Config(format!(
                        "Project '{proj}' not found. Available: {}",
                        projects.join(", ")
                    )));
                }
                let _ = loader.load_project(proj)?;
                if let Some(ref env) = environment {
                    let _ = loader.load_environment(proj, env)?;
                    println!("Config valid: {proj}/{env}");
                } else {
                    println!("Config valid: {proj}");
                }
            } else {
                let projects = loader.list_projects()?;
                if projects.is_empty() {
                    println!("No projects found in {}", loader.projects_dir().display());
                    println!(
                        "Create a project: {}<name>/project.toml",
                        loader.projects_dir().display()
                    );
                } else {
                    for p in &projects {
                        println!("  {p}");
                    }
                    println!("\nUse --project <name> [--environment <env>] to validate");
                }
            }
            Ok(())
        }
        ConfigAction::Show {
            project,
            environment,
        } => {
            let resolved = loader.resolve(&project, &environment)?;
            println!("Project: {project}");
            println!("Environment: {environment}");
            println!("Driver: {} ({})", resolved.driver.vendor, resolved.driver.class);
            println!("JDBC URL: {}", resolved.environment.database.url);
            println!("Username: {}", resolved.environment.database.username);
            println!("Password: [redacted]");
            println!();
            println!("--- Security Policy ---");
            println!("Read only: {}", resolved.project.security.read_only);
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
            agents::install_entry(&client, &project, &environment, &name, Some(loader.config_dir()))
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

    println!("Found {} connection(s):\n", connections.len());
    for conn in &connections {
        let ssh = conn.ssh_host.as_deref().unwrap_or("-");
        println!("  {:<30}  {}:{}  ssh={}", conn.name, conn.host, conn.port, ssh);
    }

    println!();
    let loader = ConfigLoader::new();
    let projects_dir = loader.projects_dir();
    let mut created_any = false;

    for conn in &connections {
        // Derive project name: base name before " (" if present, else lowercased database name
        let project = conn
            .name
            .split_once(" (")
            .map(|(base, _)| base.trim().to_lowercase())
            .unwrap_or_else(|| conn.database.to_lowercase());

        // Derive environment name: parenthetical part, or "default"
        let env = conn
            .name
            .split_once(" (")
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .unwrap_or("default")
            .to_lowercase()
            .replace(' ', "-")
            .replace("--", "-");

        let project_dir = projects_dir.join(&project);
        let env_dir = project_dir.join("environments");
        std::fs::create_dir_all(&env_dir)?;

        let project_config = config::ProjectConfig::default();
        let project_toml = toml::to_string_pretty(&project_config)
            .map_err(|e| SafeselectError::TomlSer(e.to_string()))?;
        let project_file = project_dir.join("project.toml");
        if !project_file.exists() {
            std::fs::write(&project_file, project_toml)?;
            println!("  ✔ Created {}", project_file.display());
            created_any = true;
        }

        let ssh = conn.ssh_host.as_ref().map(|h| config::SshConfig {
            enabled: true,
            host: Some(h.clone()),
            port: conn.ssh_port,
            username: conn.ssh_user.clone(),
            identity_file: None,
            known_hosts: None,
        });

        let env_config = config::EnvironmentConfig {
            version: 1,
            database: config::DatabaseConfig {
                driver: conn.driver.clone(),
                url: format!("jdbc:postgresql://{}:{}/{}", conn.host, conn.port, conn.database),
                username: conn.username.clone(),
                secret: None,
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
            println!("  ✔ Created {}", env_file.display());
            created_any = true;
        }
    }

    if created_any {
        println!("\nImport complete. Edit the new files and add secrets.");
        println!("  Example: security add-generic-password -a \"<project>/<env>\" -s \"safeselect\" -w \"<password>\"");
    } else {
        println!("All projects/environments already exist. Nothing to import.");
    }
    Ok(())
}

fn cmd_check(loader: &ConfigLoader, project: &str, environment: &str) -> Result<()> {
    println!("Checking configuration for {project}/{environment}...");

    let resolved = loader.resolve(project, environment)?;

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
    println!("  ✓ All checks passed for {project}/{environment}");

    sidecar.shutdown()?;

    Ok(())
}

fn cmd_query(loader: &ConfigLoader, project: &str, environment: &str, sql: Option<&str>) -> Result<()> {
    let resolved = loader.resolve(project, environment)?;
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

fn cmd_uninstall(force: bool) -> Result<()> {
    if !force {
        println!("This will remove: safeselect binary, config, data, audit logs, and keychain entries.");
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

    for config_dir in [
        dirs::config_dir().map(|d| d.join("safeselect")),
        Some(std::path::PathBuf::from("~/.config/safeselect")),
    ]
    .into_iter()
    .flatten()
    {
        if config_dir.exists() {
            std::fs::remove_dir_all(&config_dir)?;
            println!("  ✓ Removed {}", config_dir.display());
            removed_anything = true;
        }
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

    let backup_patterns = [
        format!(
            "{}/Library/Application Support/opencode/opencode.json.safeselect.bak",
            dirs::home_dir().unwrap().display()
        ),
        format!(
            "{}/.config/opencode/opencode.json.safeselect.bak",
            dirs::home_dir().unwrap().display()
        ),
    ];
    for pattern in &backup_patterns {
        let path = std::path::Path::new(pattern);
        if path.exists() {
            std::fs::remove_file(path)?;
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
