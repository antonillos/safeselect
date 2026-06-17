use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
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
    /// Import configuration from DBeaver export ZIP into .safeselect/
    ImportDbeaver {
        /// Path to DBeaver .zip export
        path: String,
    },
    /// Test connectivity
    Check {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: String,
    },
    /// Execute a SQL query and display results
    Query {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: String,
        /// SQL query to execute (reads from stdin if omitted)
        #[arg(long)]
        sql: Option<String>,
    },
    /// Disconnect from the database (MCP tool — callable by AI agents)
    Disconnect {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: String,
    },
    /// Reconnect to the database (MCP tool — callable by AI agents)
    Connect {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: String,
    },
    /// Uninstall SafeSelect (binary, global config, data, audit, keychain)
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Validate .safeselect/ configuration
    Validate {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: Option<String>,
    },
    /// Show resolved configuration (secrets redacted)
    Show {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
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
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
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
