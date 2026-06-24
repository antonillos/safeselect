use clap::{Parser, Subcommand};
use std::path::PathBuf;

const VERSION: &str = match option_env!("SAFESELECT_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(name = "safeselect", about = "MCP SQL Fail-Closed for AI Agents", version = VERSION)]
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
        /// Non-interactive mode — import all connections without prompting
        #[arg(long, default_value_t = false)]
        non_interactive: bool,
    },
    /// Discover PostgreSQL connections from docker-compose files
    ImportCompose {
        /// Directory to scan for docker-compose files (default: CWD)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Non-interactive mode — import all without prompting
        #[arg(long, default_value_t = false)]
        non_interactive: bool,
    },
    /// Test connectivity
    Check {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        #[arg(long)]
        environment: Option<String>,
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Diagnose configuration, SSH, sidecar, JDBC, and SELECT 1 connectivity
    Doctor {
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
        /// Enable verbose logging (sidecar debug output)
        #[arg(long, default_value_t = false)]
        verbose: bool,
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
    /// Restart sidecar and verify the database connection with a test query
    Reconnect {
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
    /// Rename an environment
    RenameEnvironment {
        /// Current environment name
        #[arg(long)]
        old: String,
        /// New environment name
        #[arg(long)]
        new: String,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Delete an environment configuration
    DeleteEnvironment {
        /// Environment name to delete
        #[arg(long)]
        name: String,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Store a password in the Keychain and update the environment config
    SetPassword {
        #[arg(long)]
        environment: String,
        /// Password value (prompts securely if omitted)
        #[arg(long)]
        password: Option<String>,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Reset project config (removes all environments and keychain entries)
    Reset {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
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
        /// Entry name (default: safeselect-<project-dir>-<environment>)
        #[arg(long)]
        name: Option<String>,
        /// Install to project-local config instead of global config
        #[arg(long)]
        local: bool,
    },
    /// Upgrade an existing MCP entry in-place
    Upgrade {
        /// Client name (opencode, copilot, cursor, etc.)
        client: String,
        /// Existing entry name to upgrade (auto-detected from the current project if omitted)
        #[arg(long)]
        name: Option<String>,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment name to serve (auto-detected from the existing entry if omitted)
        #[arg(long)]
        environment: Option<String>,
        /// Upgrade the project-local config instead of global config
        #[arg(long)]
        local: bool,
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
