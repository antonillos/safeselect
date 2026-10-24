use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;

pub(crate) const VERSION: &str = match option_env!("SAFESELECT_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(
    name = "safeselect",
    about = "Fail-closed read-only database access for AI agents over MCP",
    version = VERSION
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse_with_help() -> Self {
        let matches = Self::help_command().get_matches();
        Self::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
    }

    fn help_command() -> clap::Command {
        let mut command = Self::command();
        // Build first so generated help/version flags receive the same layout.
        command.build();
        style_help(command)
    }
}

fn style_help(command: clap::Command) -> clap::Command {
    use clap::builder::styling::{Ansi256Color, Style, Styles};

    let heading = Style::new().bold();
    let usage_color = Ansi256Color(147).on_default();
    let styles = Styles::styled()
        .header(heading)
        .usage(heading)
        .literal(Ansi256Color(155).on_default())
        .placeholder(Ansi256Color(245).on_default());
    let mut command = command
        .styles(styles.clone())
        .mut_args(|arg| {
            let heading = if arg.is_positional() {
                "ARGUMENTS"
            } else if matches!(arg.get_id().as_str(), "help" | "version") {
                "GLOBAL FLAGS"
            } else {
                "FLAGS"
            };
            arg.help_heading(heading)
        })
        .mut_subcommands(style_help);
    let usage = command.render_usage().to_string();
    let usage = usage.strip_prefix("Usage: ").unwrap_or(&usage);
    let sections = if command.get_subcommands().any(|sub| !sub.is_hide_set()) {
        let flags = command
            .clone()
            .mut_subcommands(|sub| sub.hide(true))
            .help_template("{all-args}")
            .render_help();
        let subcommands = command
            .clone()
            .styles(styles.literal(usage_color))
            .help_template("{subcommands}")
            .render_help();
        let flags = flags.ansi().to_string();
        let flags = flags.trim_end();
        let separator = if flags.is_empty() { "" } else { "\n\n" };
        format!(
            "{flags}{separator}{heading}SUBCOMMANDS{heading:#}\n{}",
            subcommands.ansi().to_string().trim_end()
        )
    } else {
        "{all-args}".to_owned()
    };
    let template = format!(
        "{{before-help}}{heading}DESCRIPTION{heading:#}\n  {{about}}\n\n\
         {heading}USAGE{heading:#}\n  {usage_color}{usage}{usage_color:#}\n\n\
         {sections}{{after-help}}\n"
    );
    command.help_template(template)
}

#[derive(Subcommand)]
pub enum Command {
    /// Check for a newer stable release and show how to update
    UpdateCheck,
    /// Start the MCP server for a project/environment
    Serve {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to serve (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
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
    /// Import MongoDB Compass connections into .safeselect/
    ImportCompass {
        /// File or directory containing MongoDB Compass connection JSON
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
        /// Environment to validate (all environments by default)
        #[arg(long)]
        environment: Option<String>,
        /// Show detailed connectivity diagnostics
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Diagnose configuration, SSH, sidecar, JDBC, and SELECT 1 connectivity (show findings only)
    Doctor {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to inspect (all environments by default)
        #[arg(long)]
        environment: Option<String>,
    },
    /// Inspect the effective PostgreSQL security posture
    Posture {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to inspect (all environments by default)
        #[arg(long)]
        environment: Option<String>,
        /// Output format (text or json)
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit with an error when the security posture is unsafe
        #[arg(long, default_value_t = false)]
        strict: bool,
        /// Record acknowledgement of the current security posture
        #[arg(long, default_value_t = false)]
        acknowledge: bool,
    },
    /// Execute a SQL query and display results
    Query {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to query (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
        /// SQL query to execute (reads from stdin if omitted)
        #[arg(long)]
        sql: Option<String>,
        /// Enable verbose logging (sidecar debug output)
        #[arg(long, default_value_t = false)]
        verbose: bool,
    },
    /// Test disconnect on a temporary JDBC sidecar (does not affect an active MCP session)
    Disconnect {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to disconnect (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
    },
    /// Test connect on a temporary JDBC sidecar (does not affect an active MCP session)
    Connect {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to connect (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
    },
    /// Verify connectivity with a temporary sidecar (all environments by default)
    Reconnect {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to reconnect (all environments by default)
        #[arg(long)]
        environment: Option<String>,
    },
    /// Uninstall SafeSelect (binary, global config, data, audit, keychain)
    Uninstall {
        /// Skip confirmation prompt
        #[arg(long, default_value_t = false)]
        force: bool,
        /// Remove only binaries installed under user-local paths
        #[arg(long, default_value_t = false)]
        binary_only: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Validate .safeselect/ configuration
    Validate {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to validate (all environments by default)
        #[arg(long)]
        environment: Option<String>,
    },
    /// Show resolved configuration (secrets redacted)
    Show {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to show (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
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
    /// Configure a database password: {env:NAME} on any platform or Keychain on macOS
    SetPassword {
        /// Environment whose password is stored (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
        /// Password or {env:NAME} reference (prompts securely if omitted)
        #[arg(long)]
        password: Option<String>,
        /// Treat password input as literal, including the secure prompt (macOS only)
        #[arg(long)]
        literal_password: bool,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Configure an SSH password: {env:NAME} on any platform or Keychain on macOS
    SetSshPassword {
        /// Environment whose SSH password is stored (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
        /// SSH password or {env:NAME} reference (literal storage is macOS-only)
        #[arg(long)]
        password: Option<String>,
        /// Treat password input as literal, including the secure prompt (macOS only)
        #[arg(long)]
        literal_password: bool,
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
    /// Uninstall project config without uninstalling the SafeSelect CLI
    Uninstall {
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum DriverAction {
    /// Register a JDBC driver
    Add {
        /// Vendor name used to identify the driver
        #[arg(long)]
        vendor: String,
        /// Path to the JDBC driver JAR
        #[arg(long)]
        path: String,
        /// Fully qualified JDBC driver class name
        #[arg(long)]
        class: String,
        /// SHA-256 checksum to register (computed from the JAR if omitted)
        #[arg(long)]
        sha256: Option<String>,
    },
    /// List registered drivers
    List,
    /// Download official PostgreSQL driver
    Download {
        /// Driver vendor (postgresql)
        #[arg(long)]
        vendor: String,
    },
}

#[derive(Subcommand)]
pub enum AgentAction {
    /// Detect installed MCP clients
    Detect,
    /// Install MCP entry (and shared skills for Codex/OpenCode)
    Install {
        /// Client name (opencode, copilot, cursor, etc.)
        client: String,
        /// Path to repo root containing .safeselect/ (auto-detected from CWD if omitted)
        #[arg(long)]
        project: Option<PathBuf>,
        /// Environment to serve (inferred when the project has exactly one)
        #[arg(long)]
        environment: Option<String>,
        /// Entry name (default: safeselect-<project-dir>-<environment>)
        #[arg(long)]
        name: Option<String>,
        /// Install to project-local config (auto-selected when local config exists)
        #[arg(long, conflicts_with = "global")]
        local: bool,
        /// Install to global user config instead of detecting or asking for scope
        #[arg(long)]
        global: bool,
    },
    /// Upgrade an existing MCP entry and refresh Codex/OpenCode skills
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
        /// Upgrade project-local config (auto-selected when local config exists)
        #[arg(long, conflicts_with = "global")]
        local: bool,
        /// Upgrade global user config instead of detecting or asking for scope
        #[arg(long)]
        global: bool,
    },
    /// Uninstall MCP entry
    Uninstall {
        /// Client name
        client: String,
        /// Entry to uninstall (auto-detected with confirmation if omitted)
        #[arg(long)]
        name: Option<String>,
    },
    /// Show installation status
    Status,
}

#[cfg(test)]
mod tests {
    #[path = "password_references.rs"]
    mod password_reference_tests;

    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn help_layout_reaches_every_command() {
        fn check(command: &mut clap::Command) {
            let help = command.render_help().to_string();
            assert!(help.starts_with("DESCRIPTION\n  "), "{help}");
            assert!(help.contains("\n\nUSAGE\n  safeselect"), "{help}");
            assert!(!help.contains('\u{1b}'), "{help}");
            if command.get_arguments().any(|arg| arg.get_id() == "help") {
                assert!(help.contains("GLOBAL FLAGS:"), "{help}");
            }
            if command.get_subcommands().any(|sub| !sub.is_hide_set())
                && help.contains("GLOBAL FLAGS:")
            {
                assert!(
                    matches!(
                        (help.find("GLOBAL FLAGS:"), help.find("SUBCOMMANDS\n")),
                        (Some(flags), Some(subcommands)) if flags < subcommands
                    ),
                    "{help}"
                );
            }
            for subcommand in command.get_subcommands_mut() {
                check(subcommand);
            }
        }
        check(&mut Cli::help_command());
    }

    #[test]
    fn help_entry_points_share_layout_and_preserve_arguments() {
        for args in [
            vec!["safeselect", "agent", "install", "--help"],
            vec!["safeselect", "agent", "install", "-h"],
            vec!["safeselect", "help", "agent", "install"],
        ] {
            let error = Cli::help_command().try_get_matches_from(args).unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
            let help = error.to_string();
            for text in [
                "DESCRIPTION",
                "USAGE",
                "ARGUMENTS:",
                "FLAGS:",
                "GLOBAL FLAGS:",
                "<CLIENT>",
                "--project",
                "--local",
                "--help",
            ] {
                assert!(help.contains(text), "Missing {text}: {help}");
            }
        }
        let matches = Cli::help_command()
            .try_get_matches_from(["safeselect", "query", "--sql", "SELECT 1"])
            .unwrap();
        let cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches).unwrap();
        assert!(matches!(cli.command, Command::Query { sql: Some(sql), .. } if sql == "SELECT 1"));
    }

    #[test]
    fn doctor_and_posture_default_to_all_environments() {
        let Cli { command } = Cli::try_parse_from(["safeselect", "doctor"]).unwrap();
        assert!(matches!(
            command,
            Command::Doctor {
                environment: None,
                ..
            }
        ));

        let Cli { command } = Cli::try_parse_from(["safeselect", "posture"]).unwrap();
        assert!(matches!(
            command,
            Command::Posture {
                environment: None,
                ..
            }
        ));
    }

    #[test]
    fn agent_install_scope_flags_are_optional_and_mutually_exclusive() {
        for flag in [None, Some("--local"), Some("--global")] {
            let mut args = vec!["safeselect", "agent", "install", "opencode"];
            args.extend(flag);
            assert!(Cli::try_parse_from(args).is_ok());
        }
        assert!(Cli::try_parse_from([
            "safeselect",
            "agent",
            "install",
            "opencode",
            "--local",
            "--global",
        ])
        .is_err());
    }

    #[test]
    fn agent_upgrade_scope_flags_are_optional_and_mutually_exclusive() {
        for flag in [None, Some("--local"), Some("--global")] {
            let mut args = vec!["safeselect", "agent", "upgrade", "opencode"];
            args.extend(flag);
            assert!(Cli::try_parse_from(args).is_ok());
        }
        assert!(Cli::try_parse_from([
            "safeselect",
            "agent",
            "upgrade",
            "opencode",
            "--local",
            "--global",
        ])
        .is_err());
    }

    #[test]
    fn single_environment_commands_allow_convention_based_selection() {
        for args in [
            ["safeselect", "serve"].as_slice(),
            ["safeselect", "query"].as_slice(),
            ["safeselect", "connect"].as_slice(),
            ["safeselect", "disconnect"].as_slice(),
            ["safeselect", "config", "show"].as_slice(),
            ["safeselect", "config", "set-password"].as_slice(),
            ["safeselect", "config", "set-ssh-password"].as_slice(),
        ] {
            assert!(Cli::try_parse_from(args).is_ok(), "{args:?}");
        }
    }
}
