use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "safeselect", about = "MCP SQL Fail-Closed for AI Agents", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the MCP server for a project/environment
    Serve {
        /// Project name (directory under ~/.config/safeselect/projects/)
        #[arg(long)]
        project: String,
        /// Environment name (file under projects/<name>/environments/)
        #[arg(long)]
        environment: String,
    },
    /// Validate configuration without starting the server
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage JDBC drivers
    Driver {
        #[command(subcommand)]
        action: DriverAction,
    },
    /// Manage AI agent integrations
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Import configuration from DBeaver export ZIP
    ImportDbeaver {
        /// Path to DBeaver .zip export
        path: String,
    },
    /// Test connectivity
    Check {
        #[arg(long)]
        project: String,
        #[arg(long)]
        environment: String,
    },
    /// Uninstall SafeSelect (binary, config, data, audit, keychain)
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Validate all configuration files
    Validate {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        environment: Option<String>,
    },
    /// Show resolved configuration (secrets redacted)
    Show {
        #[arg(long)]
        project: String,
        #[arg(long)]
        environment: String,
    },
}

#[derive(Subcommand)]
pub enum DriverAction {
    /// Register a JDBC driver
    Add {
        #[arg(long)]
        vendor: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        class: String,
        #[arg(long)]
        sha256: Option<String>,
    },
    /// List registered drivers
    List,
    /// Download official PostgreSQL driver
    Download {
        #[arg(long)]
        vendor: String,
    },
}

#[derive(Subcommand)]
pub enum AgentAction {
    /// Detect installed MCP clients
    Detect,
    /// Install MCP entry for a client
    Install {
        /// Client name (opencode, copilot, cursor, etc.)
        client: String,
        #[arg(long)]
        project: String,
        #[arg(long)]
        environment: String,
        #[arg(long)]
        name: String,
    },
    /// Uninstall MCP entry
    Uninstall {
        /// Client name
        client: String,
        #[arg(long)]
        name: String,
    },
    /// Show installation status
    Status,
}
