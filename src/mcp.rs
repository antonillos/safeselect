use crate::agents;
use crate::audit::AuditLog;
use crate::compose;
use crate::config::{ConfigLoader, EnvironmentConfig, ProjectConfig};
use crate::diagnostics::{self, DiagnosticCode, DiagnosticStatus};
use crate::error::Result;
use crate::security::SecurityEngine;
use crate::sidecar::SidecarProcess;
use crate::{setup_ssh_tunnels, update_generated_by};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct JsonRpcMessage {
    #[serde(default)]
    id: Option<serde_json::Value>,
    method: Option<String>,
    #[serde(default)]
    params: Option<serde_json::Value>,
    #[serde(default)]
    jsonrpc: Option<String>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct ToolDefinition {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

pub struct McpServer {
    sidecar: Option<SidecarProcess>,
    security: SecurityEngine,
    audit: AuditLog,
    project_name: String,
    env_name: String,
    client_name: String,
    idle_timeout_seconds: u64,
    driver_path: String,
    driver_class: String,
    db_url: String,
    db_username: String,
    db_password: String,
    repo_root: PathBuf,
    config_dir: PathBuf,
}

impl McpServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_config: ProjectConfig,
        env_config: EnvironmentConfig,
        project_name: &str,
        env_name: &str,
        driver_path: &str,
        driver_class: &str,
        db_url: &str,
        db_username: &str,
        db_password: &str,
        repo_root: &Path,
        config_dir: &Path,
    ) -> Result<Self> {
        let security = SecurityEngine::new(
            project_config.security.clone(),
            project_config.limits.clone(),
        );

        let idle_timeout_seconds = env_config.limits.idle_timeout_seconds.unwrap_or(0);

        let audit = AuditLog::open(&project_config.audit, project_name, env_name, "unknown")?;

        Ok(Self {
            sidecar: None,
            security,
            audit,
            project_name: project_name.to_string(),
            env_name: env_name.to_string(),
            client_name: "unknown".to_string(),
            idle_timeout_seconds,
            driver_path: driver_path.to_string(),
            driver_class: driver_class.to_string(),
            db_url: db_url.to_string(),
            db_username: db_username.to_string(),
            db_password: db_password.to_string(),
            repo_root: repo_root.to_path_buf(),
            config_dir: config_dir.to_path_buf(),
        })
    }

    fn ensure_sidecar(&mut self) -> Result<&mut SidecarProcess> {
        if self.sidecar.is_some() {
            return Ok(self.sidecar.as_mut().unwrap());
        }
        tracing::info!("Lazy-starting sidecar");
        let sidecar = SidecarProcess::start_with_timeout(
            &self.driver_path,
            &self.driver_class,
            &self.db_url,
            &self.db_username,
            &self.db_password,
            self.idle_timeout_seconds,
            self.security.limits().statement_timeout_ms,
            false,
        )?;
        tracing::info!("Sidecar ready");
        self.sidecar = Some(sidecar);
        Ok(self.sidecar.as_mut().unwrap())
    }

    pub fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let msg: JsonRpcMessage = match serde_json::from_str(line) {
                Ok(m) => m,
                Err(e) => {
                    let resp = JsonRpcResponse {
                        jsonrpc: "2.0",
                        id: None,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32700,
                            message: "Parse error".into(),
                            data: Some(serde_json::json!({"detail": e.to_string()})),
                        }),
                    };
                    self.write_response(&resp)?;
                    continue;
                }
            };

            let method = match msg.method.as_deref() {
                Some(m) => m,
                None => continue,
            };

            match method {
                "initialize" => self.handle_initialize(&msg)?,
                "tools/list" => self.handle_tools_list(&msg)?,
                "tools/call" => self.handle_tools_call(&msg)?,
                "notifications/initialized" => {}
                _ => {
                    let resp = JsonRpcResponse {
                        jsonrpc: "2.0",
                        id: msg.id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32601,
                            message: format!("Method not found: {method}"),
                            data: None,
                        }),
                    };
                    self.write_response(&resp)?;
                }
            }
        }

        Ok(())
    }

    fn handle_initialize(&mut self, msg: &JsonRpcMessage) -> Result<()> {
        let client_name = msg
            .params
            .as_ref()
            .and_then(|p| p.get("clientInfo"))
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let proto_version = msg
            .params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("2024-11-05")
            .to_string();

        self.client_name = client_name.clone();

        // Pre-start the sidecar so it's ready before the first query
        tracing::info!("Pre-starting sidecar during initialize (client: {client_name})");
        if let Err(e) = self.ensure_sidecar() {
            tracing::warn!("Sidecar pre-start failed during initialize: {e}");
        }

        let resp = JsonRpcResponse {
            jsonrpc: "2.0",
            id: msg.id.clone(),
            result: Some(serde_json::json!({
                "protocolVersion": proto_version,
                "capabilities": {
                    "tools": {
                        "list": {}
                    }
                },
                "serverInfo": {
                    "name": "safeselect",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        };
        self.write_response(&resp)
    }

    fn handle_tools_list(&mut self, msg: &JsonRpcMessage) -> Result<()> {
        let tools = vec![
            ToolDefinition {
                name: "select".into(),
                description: "Execute a SELECT query on the database".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL SELECT query to execute"
                        }
                    },
                    "required": ["sql"]
                }),
            },
            ToolDefinition {
                name: "list_tables".into(),
                description: "List tables in the database, optionally filtered by schema".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "schema": {
                            "type": "string",
                            "description": "Schema filter (optional)"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: "explain".into(),
                description: "Show the execution plan for a query without executing it".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "sql": {
                            "type": "string",
                            "description": "SQL query to explain"
                        }
                    },
                    "required": ["sql"]
                }),
            },
            ToolDefinition {
                name: "disconnect".into(),
                description: "Disconnect from the database (closes the JDBC connection)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "connect".into(),
                description: "Reconnect to the database (re-establishes the JDBC connection)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "config_validate".into(),
                description: "Validate the .safeselect/ configuration".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "environment": {
                            "type": "string",
                            "description": "Environment name to validate (optional — validates project structure if omitted)"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: "config_show".into(),
                description: "Show the resolved configuration for an environment (secrets redacted)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "environment": {
                            "type": "string",
                            "description": "Environment name"
                        }
                    },
                    "required": ["environment"]
                }),
            },
            ToolDefinition {
                name: "config_rename_environment".into(),
                description: "Rename an environment within the project".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "old_name": {
                            "type": "string",
                            "description": "Current environment name"
                        },
                        "new_name": {
                            "type": "string",
                            "description": "New environment name"
                        }
                    },
                    "required": ["old_name", "new_name"]
                }),
            },
            ToolDefinition {
                name: "config_delete_environment".into(),
                description: "Delete an environment configuration from the project".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Environment name to delete"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "config_set_password".into(),
                description: "Store a database password in the macOS Keychain for an environment".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "environment": {
                            "type": "string",
                            "description": "Environment name"
                        },
                        "password": {
                            "type": "string",
                            "description": "Database password"
                        }
                    },
                    "required": ["environment", "password"]
                }),
            },
            ToolDefinition {
                name: "config_reset".into(),
                description: "Reset all environments and their keychain entries for the project".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "confirm": {
                            "type": "boolean",
                            "description": "Must be set to true to confirm the reset"
                        }
                    },
                    "required": ["confirm"]
                }),
            },
            ToolDefinition {
                name: "driver_list".into(),
                description: "List registered JDBC drivers".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "driver_add".into(),
                description: "Register a JDBC driver".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vendor": {
                            "type": "string",
                            "description": "Vendor name (e.g. postgresql)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Path to JDBC JAR file"
                        },
                        "class": {
                            "type": "string",
                            "description": "JDBC driver class name (e.g. org.postgresql.Driver)"
                        },
                        "sha256": {
                            "type": "string",
                            "description": "SHA-256 checksum of the JAR (optional, auto-computed if omitted)"
                        }
                    },
                    "required": ["vendor", "path", "class"]
                }),
            },
            ToolDefinition {
                name: "driver_download".into(),
                description: "Download and register the official PostgreSQL JDBC driver".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "vendor": {
                            "type": "string",
                            "description": "Vendor name (only 'postgresql' is supported)"
                        }
                    },
                    "required": ["vendor"]
                }),
            },
            ToolDefinition {
                name: "agent_detect".into(),
                description: "Detect installed MCP clients on this system".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "agent_install".into(),
                description: "Install a safeselect MCP entry for a client".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "client": {
                            "type": "string",
                            "description": "Client name (opencode, cursor, windsurf, claude-code, codex, copilot, gemini-cli)"
                        },
                        "environment": {
                            "type": "string",
                            "description": "Environment name to serve"
                        },
                        "name": {
                            "type": "string",
                            "description": "Entry name (optional, defaults to '<project-dir>-<environment>')"
                        }
                    },
                    "required": ["client", "environment"]
                }),
            },
            ToolDefinition {
                name: "agent_uninstall".into(),
                description: "Uninstall a safeselect MCP entry from a client".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "client": {
                            "type": "string",
                            "description": "Client name"
                        },
                        "name": {
                            "type": "string",
                            "description": "Entry name to remove"
                        }
                    },
                    "required": ["client", "name"]
                }),
            },
            ToolDefinition {
                name: "agent_status".into(),
                description: "Show safeselect installation status for all MCP clients".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "import_compose".into(),
                description: "Scan docker-compose files for PostgreSQL services and import into .safeselect/".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "scan_path": {
                            "type": "string",
                            "description": "Directory to scan for docker-compose files (default: project root)"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: "check".into(),
                description: "Check database connectivity by starting the sidecar and running a ping".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "uninstall".into(),
                description: "Uninstall safeselect (binary, config, data, audit, keychain)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "confirm": {
                            "type": "boolean",
                            "description": "Must be set to true to confirm uninstall"
                        }
                    },
                    "required": ["confirm"]
                }),
            },
            ToolDefinition {
                name: "reconnect".into(),
                description: "Restart the sidecar process and verify the database connection with a test query".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        ];

        let resp = JsonRpcResponse {
            jsonrpc: "2.0",
            id: msg.id.clone(),
            result: Some(serde_json::json!({ "tools": tools })),
            error: None,
        };
        self.write_response(&resp)
    }

    fn handle_tools_call(&mut self, msg: &JsonRpcMessage) -> Result<()> {
        let params = match msg.params.as_ref() {
            Some(p) => p,
            None => {
                return self.send_error(msg.id.clone(), -32602, "Missing params");
            }
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return self.send_error(msg.id.clone(), -32602, "Missing tool name"),
        };

        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        match tool_name {
            "select" => self.handle_select(msg.id.clone(), &args),
            "list_tables" => self.handle_list_tables(msg.id.clone(), &args),
            "explain" => self.handle_explain(msg.id.clone(), &args),
            "disconnect" => self.handle_disconnect(msg.id.clone()),
            "connect" => self.handle_connect(msg.id.clone()),
            "config_validate" => self.handle_config_validate(msg.id.clone(), &args),
            "config_show" => self.handle_config_show(msg.id.clone(), &args),
            "config_rename_environment" => {
                self.handle_config_rename_environment(msg.id.clone(), &args)
            }
            "config_delete_environment" => {
                self.handle_config_delete_environment(msg.id.clone(), &args)
            }
            "config_set_password" => self.handle_config_set_password(msg.id.clone(), &args),
            "config_reset" => self.handle_config_reset(msg.id.clone(), &args),
            "driver_list" => self.handle_driver_list(msg.id.clone()),
            "driver_add" => self.handle_driver_add(msg.id.clone(), &args),
            "driver_download" => self.handle_driver_download(msg.id.clone(), &args),
            "agent_detect" => self.handle_agent_detect(msg.id.clone()),
            "agent_install" => self.handle_agent_install(msg.id.clone(), &args),
            "agent_uninstall" => self.handle_agent_uninstall(msg.id.clone(), &args),
            "agent_status" => self.handle_agent_status(msg.id.clone()),
            "import_compose" => self.handle_import_compose(msg.id.clone(), &args),
            "check" => self.handle_check(msg.id.clone()),
            "uninstall" => self.handle_uninstall(msg.id.clone(), &args),
            "reconnect" => self.handle_reconnect(msg.id.clone()),
            _ => self.send_error(msg.id.clone(), -32602, format!("Unknown tool: {tool_name}")),
        }
    }

    fn handle_select(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let sql = match args.get("sql").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return self.send_error(id, -32602, "Missing 'sql' argument"),
        };

        let start = std::time::Instant::now();

        match self.security.validate(sql) {
            Ok(()) => {}
            Err(e) => {
                self.audit.record("REJECT", "reject", sql)?;
                let _ = self.send_error(id, -32000, format!("Query rejected: {e}"));
                self.fail_closed("Security violation");
                return Ok(());
            }
        }

        let result = self.execute_with_reconnect(sql);

        match result {
            Ok(query_result) => {
                if let Err(e) = self
                    .security
                    .check_result_size(query_result.row_count, query_result.byte_count)
                {
                    self.audit.record("LIMIT_EXCEEDED", "reject", sql)?;
                    let _ = self.send_error(id, -32000, format!("{e}"));
                    self.fail_closed("Limit exceeded");
                    return Ok(());
                }
                self.audit.record("PASS", "allow", sql)?;
                let elapsed = start.elapsed();
                if elapsed > std::time::Duration::from_secs(1) {
                    tracing::warn!(
                        "Slow query: {elapsed:?} — {} rows, {} bytes",
                        query_result.row_count,
                        query_result.byte_count
                    );
                }
                tracing::debug!(
                    "Query completed in {elapsed:?}: {} rows, {} bytes",
                    query_result.row_count,
                    query_result.byte_count
                );
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string(&query_result)?
                        }]
                    })),
                    error: None,
                };
                self.write_response(&resp)
            }
            Err(e) => {
                let elapsed = start.elapsed();
                tracing::error!("Query failed after {elapsed:?}: {e}");
                self.audit.record("JDBC_ERROR", "error", sql)?;
                self.send_error(id, -32000, format!("Query execution failed: {e}"))
            }
        }
    }

    fn handle_list_tables(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let schema = args.get("schema").and_then(|v| v.as_str());

        let allowed = self.security.allowed_schemas();
        let sql = match schema {
            Some(s) if is_valid_identifier(s) => {
                if !allowed.is_empty() && !allowed.iter().any(|a| a == s) {
                    return self.send_error(
                        id,
                        -32000,
                        format!(
                            "Schema '{s}' is not in the allowed schemas list ({})",
                            allowed.join(", ")
                        ),
                    );
                }
                format!(
                    "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema = '{}' ORDER BY table_schema, table_name",
                    s
                )
            }
            Some(_) => {
                return self.send_error(
                    id,
                    -32602,
                    "Invalid schema name: only alphanumeric and underscores allowed",
                );
            }
            None => {
                if allowed.is_empty() {
                    "SELECT table_schema, table_name, table_type FROM information_schema.tables ORDER BY table_schema, table_name".into()
                } else {
                    let schemas: Vec<String> = allowed
                        .iter()
                        .map(|s| format!("'{}'", s.replace('\'', "''")))
                        .collect();
                    format!(
                        "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema IN ({}) ORDER BY table_schema, table_name",
                        schemas.join(", ")
                    )
                }
            }
        };

        match self.security.validate_system(&sql) {
            Ok(()) => {}
            Err(e) => {
                self.audit.record("REJECT", "reject", &sql)?;
                let _ = self.send_error(id, -32000, format!("Query rejected: {e}"));
                self.fail_closed("Security violation");
                return Ok(());
            }
        }

        match self.execute_with_reconnect(&sql) {
            Ok(result) => {
                self.audit.record("PASS", "allow", &sql)?;
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string(&result)?
                        }]
                    })),
                    error: None,
                };
                self.write_response(&resp)
            }
            Err(e) => {
                self.audit.record("JDBC_ERROR", "error", &sql)?;
                self.send_error(id, -32000, format!("Query failed: {e}"))
            }
        }
    }

    fn handle_explain(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let sql = match args.get("sql").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return self.send_error(id, -32602, "Missing 'sql' argument"),
        };

        let explain_sql = format!("EXPLAIN (FORMAT JSON) {}", sql);

        match self.security.validate(&explain_sql) {
            Ok(()) => {}
            Err(e) => {
                self.audit.record("REJECT", "reject", sql)?;
                let _ = self.send_error(id, -32000, format!("Query rejected: {e}"));
                self.fail_closed("Security violation");
                return Ok(());
            }
        }

        match self.execute_with_reconnect(&explain_sql) {
            Ok(result) => {
                self.audit.record("PASS", "allow", sql)?;
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string(&result)?
                        }]
                    })),
                    error: None,
                };
                self.write_response(&resp)
            }
            Err(e) => {
                self.audit.record("JDBC_ERROR", "error", sql)?;
                self.send_error(id, -32000, format!("Explain failed: {e}"))
            }
        }
    }

    fn handle_disconnect(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let sidecar = match self.sidecar.as_mut() {
            Some(s) => s,
            None => return self.send_error(id, -32000, "Not connected"),
        };
        match sidecar.disconnect() {
            Ok(()) => {
                self.audit
                    .record("DISCONNECT", "allow", "manual disconnect")?;
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": "Disconnected from database."
                        }]
                    })),
                    error: None,
                };
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Disconnect failed: {e}")),
        }
    }

    fn handle_connect(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let sidecar = match self.sidecar.as_mut() {
            Some(s) => s,
            None => return self.send_error(id, -32000, "Not connected"),
        };
        match sidecar.connect() {
            Ok(()) => {
                self.audit.record("CONNECT", "allow", "manual reconnect")?;
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": "Reconnected to database."
                        }]
                    })),
                    error: None,
                };
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Reconnect failed: {e}")),
        }
    }

    /// Execute a query, reconnecting once if the connection is lost.
    fn execute_with_reconnect(
        &mut self,
        sql: &str,
    ) -> std::result::Result<crate::sidecar::QueryResult, crate::error::SafeselectError> {
        let start = std::time::Instant::now();
        tracing::debug!("execute_with_reconnect started");

        self.ensure_sidecar()?;
        tracing::debug!("Sidecar ensured ({:?})", start.elapsed());

        let result = self.sidecar.as_mut().unwrap().execute(sql);
        tracing::debug!("First execute attempt completed ({:?})", start.elapsed());

        if result.is_ok() {
            return result;
        }

        let err_message = result
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        let is_timeout = is_sidecar_timeout(&err_message);
        let is_recoverable = is_timeout || is_recoverable_connection_error(&err_message);

        if !is_recoverable {
            tracing::warn!(
                "First execute failed with non-recoverable error ({:?}): {}",
                start.elapsed(),
                err_message
            );
            return result;
        }

        tracing::warn!(
            "{}: connection lost during execute ({:?}): {}",
            DiagnosticCode::ConnectionLost.as_str(),
            start.elapsed(),
            err_message
        );

        tracing::info!(
            "{}: attempting SSH tunnel recovery",
            DiagnosticCode::SshTunnelRecoveryAttempt.as_str()
        );
        if let Err(e) = setup_ssh_tunnels(&self.repo_root, &[self.env_name.clone()]) {
            tracing::warn!("SSH tunnel recovery attempt failed: {e}");
        }

        if is_timeout {
            // Sidecar is hung, do full restart immediately
            tracing::warn!(
                "{}: execute timed out — restarting sidecar process immediately ({:?})",
                DiagnosticCode::SidecarRestartAttempt.as_str(),
                start.elapsed()
            );
            self.restart_sidecar()?;
            tracing::info!("Sidecar restarted ({:?}), retrying query", start.elapsed());
            let retry = self.sidecar.as_mut().unwrap().execute(sql);
            if retry.is_ok() {
                tracing::info!("{}", DiagnosticCode::RecoveryOk.as_str());
            } else {
                tracing::warn!("{}", DiagnosticCode::RecoveryFailed.as_str());
            }
            retry
        } else {
            // Other error, try JDBC reconnect first
            tracing::warn!(
                "{}: first execute failed ({:?}), attempting JDBC reconnect",
                DiagnosticCode::JdbcReconnectAttempt.as_str(),
                start.elapsed()
            );
            let reconnected = self.sidecar.as_mut().unwrap().connect().is_ok();
            tracing::debug!("JDBC reconnect completed ({:?})", start.elapsed());

            if reconnected {
                let _ =
                    self.audit
                        .record("AUTO_RECONNECT", "allow", "connection lost — reconnected");
                tracing::info!(
                    "JDBC reconnect succeeded, retrying query ({:?})",
                    start.elapsed()
                );
                let retry = self.sidecar.as_mut().unwrap().execute(sql);
                if retry.is_ok() {
                    tracing::info!("{}", DiagnosticCode::RecoveryOk.as_str());
                } else {
                    tracing::warn!("{}", DiagnosticCode::RecoveryFailed.as_str());
                }
                retry
            } else {
                tracing::warn!(
                    "{}: execute + reconnect both failed — restarting sidecar process ({:?})",
                    DiagnosticCode::SidecarRestartAttempt.as_str(),
                    start.elapsed()
                );
                self.restart_sidecar()?;
                tracing::info!("Sidecar restarted ({:?}), retrying query", start.elapsed());
                let retry = self.sidecar.as_mut().unwrap().execute(sql);
                if retry.is_ok() {
                    tracing::info!("{}", DiagnosticCode::RecoveryOk.as_str());
                } else {
                    tracing::warn!("{}", DiagnosticCode::RecoveryFailed.as_str());
                }
                retry
            }
        }
    }

    fn restart_sidecar(&mut self) -> Result<()> {
        if let Some(mut s) = self.sidecar.take() {
            // Use force_kill to avoid timeout when sidecar is hung
            s.force_kill_ref();
        }

        // Wait for PostgreSQL to detect connection closure and clean up resources
        // This prevents zombie queries and connection state issues
        std::thread::sleep(std::time::Duration::from_secs(2));

        let sidecar = SidecarProcess::start_with_timeout(
            &self.driver_path,
            &self.driver_class,
            &self.db_url,
            &self.db_username,
            &self.db_password,
            self.idle_timeout_seconds,
            self.security.limits().statement_timeout_ms,
            false,
        )?;
        tracing::info!("Sidecar restarted successfully");
        self.sidecar = Some(sidecar);
        Ok(())
    }

    fn handle_config_validate(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let environment = args.get("environment").and_then(|v| v.as_str());
        let loader = ConfigLoader::new();

        let text = if let Some(env) = environment {
            match loader.resolve_local(&self.repo_root, env) {
                Ok(_) => format!("Config valid: {}/{}", self.project_name, env),
                Err(e) => return self.send_error(id, -32000, format!("Validation failed: {e}")),
            }
        } else {
            let safeselect_dir = self.repo_root.join(".safeselect");
            let has_project = safeselect_dir.join("project.toml").exists();
            let has_envs = safeselect_dir.join("environments").is_dir();
            if has_project || has_envs {
                format!("Config valid: {}", self.project_name)
            } else {
                return self.send_error(
                    id,
                    -32000,
                    format!("Incomplete .safeselect/ in {}", self.repo_root.display()),
                );
            }
        };

        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_config_show(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let environment = match args.get("environment").and_then(|v| v.as_str()) {
            Some(e) => e,
            None => return self.send_error(id, -32602, "Missing 'environment' argument"),
        };

        let loader = ConfigLoader::new();
        let resolved = match loader.resolve_local(&self.repo_root, environment) {
            Ok(r) => r,
            Err(e) => return self.send_error(id, -32000, format!("Config resolution failed: {e}")),
        };

        let lines = vec![
            format!("Project: {}", self.project_name),
            format!("Environment: {environment}"),
            format!(
                "Driver: {} ({})",
                resolved.driver.vendor, resolved.driver.class
            ),
            format!("JDBC URL: {}", resolved.environment.database.url),
            format!("Username: {}", resolved.environment.database.username),
            "Password: [redacted]".into(),
            String::new(),
            "--- Security Policy ---".into(),
            "Read only: enforced (cannot be disabled)".into(),
            format!(
                "Allowed schemas: {}",
                resolved.project.security.allowed_schemas.join(", ")
            ),
            format!(
                "Denied relations: {}",
                resolved.project.security.denied_relations.join(", ")
            ),
            format!(
                "Single statement: {}",
                resolved.project.security.require_single_statement
            ),
            String::new(),
            "--- Limits ---".into(),
            format!(
                "Statement timeout: {}ms",
                resolved.project.limits.statement_timeout_ms
            ),
            format!("Max rows: {}", resolved.project.limits.max_rows),
            format!(
                "Max result bytes: {}",
                resolved.project.limits.max_result_bytes
            ),
            String::new(),
            "--- TLS ---".into(),
            match resolved.environment.tls {
                Some(ref tls) => format!("Mode: {}", tls.mode),
                None => "TLS: disabled".into(),
            },
            String::new(),
            "--- SSH ---".into(),
            match resolved.environment.ssh {
                Some(ref ssh) => format!("Enabled: {}", ssh.enabled),
                None => "SSH: not configured".into(),
            },
        ];

        let resp = ok_text_response(id, lines.join("\n"));
        self.write_response(&resp)
    }

    fn handle_config_rename_environment(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let old_name = match args.get("old_name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return self.send_error(id, -32602, "Missing 'old_name' argument"),
        };
        let new_name = match args.get("new_name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return self.send_error(id, -32602, "Missing 'new_name' argument"),
        };

        let env_dir = self.repo_root.join(".safeselect").join("environments");
        let old_file = env_dir.join(format!("{old_name}.toml"));
        let new_file = env_dir.join(format!("{new_name}.toml"));

        if !old_file.exists() {
            return self.send_error(id, -32000, format!("Environment '{old_name}' not found"));
        }
        if new_file.exists() {
            return self.send_error(
                id,
                -32000,
                format!("Environment '{new_name}' already exists"),
            );
        }

        let old_account = format!("{}/{old_name}", self.project_name);
        let new_account = format!("{}/{new_name}", self.project_name);

        let old_content = std::fs::read_to_string(&old_file).unwrap_or_default();
        let mut env_config: EnvironmentConfig = match toml::from_str(&old_content) {
            Ok(c) => c,
            Err(_) => EnvironmentConfig {
                version: 1,
                database: crate::config::DatabaseConfig {
                    driver: String::new(),
                    url: String::new(),
                    username: String::new(),
                    secret: None,
                },
                tls: None,
                ssh: None,
                limits: crate::config::LimitsOverride::default(),
            },
        };

        let mut needs_rewrite = false;
        if let Some(ref mut secret) = env_config.database.secret {
            match secret.source.as_str() {
                "macos-keychain" if cfg!(target_os = "macos") => {
                    if let Ok(password) = compose::read_password_from_keychain(&old_account) {
                        let _ = compose::store_password_in_keychain(&new_account, &password);
                        let _ = compose::delete_password_from_keychain(&old_account);
                        secret.account = Some(new_account);
                        needs_rewrite = true;
                    }
                }
                "env" => {
                    let var = format!(
                        "SAFESELECT_PASSWORD_{}",
                        new_name.to_uppercase().replace('-', "_")
                    );
                    secret.variable = Some(var);
                    needs_rewrite = true;
                }
                _ => {}
            }
        }

        match std::fs::rename(&old_file, &new_file) {
            Ok(()) => {
                if needs_rewrite {
                    if let Ok(new_content) = toml::to_string_pretty(&env_config) {
                        let _ = std::fs::write(&new_file, new_content);
                    }
                }
                let mut msg = format!("Renamed '{old_name}' → '{new_name}'");
                if needs_rewrite {
                    msg.push_str("\nSecret migrated automatically.");
                }
                let resp = ok_text_response(id, msg);
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Rename failed: {e}")),
        }
    }

    fn handle_config_delete_environment(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let name = match args.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return self.send_error(id, -32602, "Missing 'name' argument"),
        };

        let env_dir = self.repo_root.join(".safeselect").join("environments");
        let env_file = env_dir.join(format!("{name}.toml"));

        if !env_file.exists() {
            return self.send_error(id, -32000, format!("Environment '{name}' not found"));
        }

        let mut removed = format!("Deleted environment '{name}'");

        if let Ok(content) = std::fs::read_to_string(&env_file) {
            if let Ok(env_config) = toml::from_str::<EnvironmentConfig>(&content) {
                if let Some(secret) = env_config.database.secret {
                    if secret.source == "macos-keychain" {
                        if let Some(ref acct) = secret.account {
                            let _ = compose::delete_password_from_keychain(acct);
                            removed.push_str("\nKeychain entry deleted.");
                        }
                    }
                }
            }
        }

        match std::fs::remove_file(&env_file) {
            Ok(()) => {
                let resp = ok_text_response(id, removed);
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Delete failed: {e}")),
        }
    }

    fn handle_config_set_password(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let environment = match args.get("environment").and_then(|v| v.as_str()) {
            Some(e) => e,
            None => return self.send_error(id, -32602, "Missing 'environment' argument"),
        };
        let password = match args.get("password").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return self.send_error(id, -32602, "Missing 'password' argument"),
        };

        let env_file = self
            .repo_root
            .join(".safeselect")
            .join("environments")
            .join(format!("{environment}.toml"));
        if !env_file.exists() {
            return self.send_error(id, -32000, format!("Environment '{environment}' not found"));
        }

        let account = format!("{}/{environment}", self.project_name);

        if let Err(e) = compose::store_password_in_keychain(&account, password) {
            return self.send_error(
                id,
                -32000,
                format!("Failed to store password in Keychain: {e}"),
            );
        }

        let secret_section = format!(
            "\n[database.secret]\nsource = \"macos-keychain\"\nservice = \"safeselect\"\naccount = \"{account}\"\n"
        );

        let mut content = std::fs::read_to_string(&env_file).unwrap_or_default();
        if content.trim().ends_with("]") {
            content.push('\n');
        }
        content.push_str(&secret_section);
        if let Err(e) = std::fs::write(&env_file, &content) {
            return self.send_error(
                id,
                -32000,
                format!("Failed to update environment file: {e}"),
            );
        }

        let text = format!(
            "Password stored in Keychain ({account})\nUpdated {}.toml",
            environment
        );
        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_config_reset(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let confirm = args
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !confirm {
            return self.send_error(
                id,
                -32000,
                "Set 'confirm' to true to reset all environments",
            );
        }

        let safeselect_dir = self.repo_root.join(".safeselect");
        let env_dir = safeselect_dir.join("environments");

        if !env_dir.is_dir() {
            let resp = ok_text_response(id, "No environments to reset.".into());
            return self.write_response(&resp);
        }

        let mut removed = 0u32;
        if let Ok(entries) = std::fs::read_dir(&env_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(env_cfg) = toml::from_str::<EnvironmentConfig>(&content) {
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

        let text = if removed > 0 {
            format!("Removed {removed} environment(s)")
        } else {
            "No environment files found.".into()
        };

        let project_file = safeselect_dir.join("project.toml");
        if project_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&project_file) {
                if let Ok(mut proj) = toml::from_str::<ProjectConfig>(&content) {
                    proj.generated_by = Some(env!("CARGO_PKG_VERSION").to_string());
                    if let Ok(new_content) = toml::to_string_pretty(&proj) {
                        let _ = std::fs::write(&project_file, new_content);
                    }
                }
            }
        }

        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_driver_list(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let loader = ConfigLoader::new();
        let drivers = match loader.list_drivers() {
            Ok(d) => d,
            Err(e) => return self.send_error(id, -32000, format!("Failed to list drivers: {e}")),
        };

        let text = if drivers.is_empty() {
            format!(
                "No drivers registered in {}. Use driver_add or driver_download.",
                loader.drivers_dir().display()
            )
        } else {
            drivers
                .iter()
                .map(|(name, config)| format!("  {name}: {} ({})", config.class, config.path))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_driver_add(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let vendor = match args.get("vendor").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => return self.send_error(id, -32602, "Missing 'vendor' argument"),
        };
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return self.send_error(id, -32602, "Missing 'path' argument"),
        };
        let class = match args.get("class").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return self.send_error(id, -32602, "Missing 'class' argument"),
        };
        let sha256 = args.get("sha256").and_then(|v| v.as_str());

        let driver_path = Path::new(path);
        if !driver_path.exists() {
            return self.send_error(id, -32000, format!("Driver file not found: {path}"));
        }

        use sha2::{Digest, Sha256};
        let checksum = match sha256 {
            Some(h) => h.to_string(),
            None => {
                let mut file = match std::fs::File::open(driver_path) {
                    Ok(f) => f,
                    Err(e) => {
                        return self.send_error(
                            id,
                            -32000,
                            format!("Failed to open driver file: {e}"),
                        )
                    }
                };
                let mut hasher = Sha256::new();
                let mut buf = Vec::new();
                if std::io::Read::read_to_end(&mut file, &mut buf).is_err() {
                    return self.send_error(id, -32000, "Failed to read driver file");
                }
                hasher.update(&buf);
                hex::encode(hasher.finalize())
            }
        };

        let config = crate::config::DriverConfig {
            version: 1,
            vendor: vendor.to_string(),
            path: path.to_string(),
            class: class.to_string(),
            sha256: checksum.clone(),
        };

        let loader = ConfigLoader::new();
        let driver_dir = loader.drivers_dir();
        if let Err(e) = std::fs::create_dir_all(driver_dir) {
            return self.send_error(id, -32000, format!("Failed to create drivers dir: {e}"));
        }
        let driver_file = driver_dir.join(format!("{vendor}.toml"));
        let content = match toml::to_string(&config) {
            Ok(c) => c,
            Err(e) => return self.send_error(id, -32000, format!("Serialization failed: {e}")),
        };
        if let Err(e) = std::fs::write(&driver_file, content) {
            return self.send_error(id, -32000, format!("Failed to write driver file: {e}"));
        }

        let text = format!(
            "Driver '{vendor}' registered at {}\nSHA-256: {checksum}",
            driver_file.display()
        );
        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_driver_download(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let vendor = match args.get("vendor").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => return self.send_error(id, -32602, "Missing 'vendor' argument"),
        };

        let url = match vendor {
            "postgresql" => "https://jdbc.postgresql.org/download/postgresql-42.7.4.jar",
            v => {
                return self.send_error(
                    id,
                    -32000,
                    format!("Unknown vendor '{v}'. Use driver_add for custom drivers."),
                )
            }
        };

        let loader = ConfigLoader::new();
        let driver_dir = loader.drivers_dir();
        if let Err(e) = std::fs::create_dir_all(driver_dir) {
            return self.send_error(id, -32000, format!("Failed to create drivers dir: {e}"));
        }
        let jar_path = driver_dir.join(format!("{vendor}.jar"));

        use sha2::{Digest, Sha256};
        let response = match reqwest::blocking::get(url) {
            Ok(r) => r,
            Err(e) => return self.send_error(id, -32000, format!("Download failed: {e}")),
        };
        let bytes = match response.bytes() {
            Ok(b) => b,
            Err(e) => return self.send_error(id, -32000, format!("Failed to read response: {e}")),
        };
        if let Err(e) = std::fs::write(&jar_path, &bytes) {
            return self.send_error(id, -32000, format!("Failed to write JAR: {e}"));
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let checksum = hex::encode(hasher.finalize());

        let config = crate::config::DriverConfig {
            version: 1,
            vendor: vendor.to_string(),
            path: jar_path.to_string_lossy().to_string(),
            class: format!("org.{vendor}.Driver"),
            sha256: checksum.clone(),
        };

        let config_path = driver_dir.join(format!("{vendor}.toml"));
        let content = match toml::to_string(&config) {
            Ok(c) => c,
            Err(e) => return self.send_error(id, -32000, format!("Serialization failed: {e}")),
        };
        if let Err(e) = std::fs::write(&config_path, content) {
            return self.send_error(id, -32000, format!("Failed to write config: {e}"));
        }

        let text = format!(
            "Downloaded and registered '{vendor}' driver\n  Path: {}\n  SHA-256: {checksum}",
            jar_path.display()
        );
        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_agent_detect(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let clients = match agents::detect_clients() {
            Ok(c) => c,
            Err(e) => return self.send_error(id, -32000, format!("Detection failed: {e}")),
        };

        let mut lines = vec!["Detected MCP clients:".into()];
        for client in &clients {
            let status = if client.detected { "✓" } else { "✗" };
            lines.push(format!("  {status} {}", client.name));
            if client.detected {
                lines.push(format!("    Config: {}", client.config_path.display()));
            }
        }

        let resp = ok_text_response(id, lines.join("\n"));
        self.write_response(&resp)
    }

    fn handle_agent_install(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let client = match args.get("client").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return self.send_error(id, -32602, "Missing 'client' argument"),
        };
        let environment = match args.get("environment").and_then(|v| v.as_str()) {
            Some(e) => e,
            None => return self.send_error(id, -32602, "Missing 'environment' argument"),
        };
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let entry_name = match name {
            Some(n) => n,
            None => format!("{}-{environment}", self.project_name),
        };

        let repo_root = self.repo_root.clone();
        let config_dir = self.config_dir.clone();

        // Calculate MCP client timeout: statement_timeout + 30s buffer
        let mcp_timeout_ms = self.security.limits().statement_timeout_ms + 30_000;

        match agents::install_entry(
            client,
            environment,
            &entry_name,
            Some(&repo_root),
            Some(&config_dir),
            mcp_timeout_ms,
            false,
        ) {
            Ok(()) => {
                let text = format!("Entry '{entry_name}' installed for {client}");
                let resp = ok_text_response(id, text);
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Install failed: {e}")),
        }
    }

    fn handle_agent_uninstall(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let client = match args.get("client").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return self.send_error(id, -32602, "Missing 'client' argument"),
        };
        let name = match args.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return self.send_error(id, -32602, "Missing 'name' argument"),
        };

        match agents::uninstall_entry(client, name) {
            Ok(()) => {
                let text = format!("Entry '{name}' uninstalled from {client}");
                let resp = ok_text_response(id, text);
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Uninstall failed: {e}")),
        }
    }

    fn handle_agent_status(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let clients = match agents::detect_clients() {
            Ok(c) => c,
            Err(e) => return self.send_error(id, -32000, format!("Detection failed: {e}")),
        };

        let mut lines = vec!["Agent integration status:".into()];
        for client in &clients {
            if client.detected {
                let content = std::fs::read_to_string(&client.config_path).unwrap_or_default();
                let has_entries = content.contains("safeselect");
                let status = if has_entries { "✓" } else { " " };
                let installed = if has_entries { " (installed)" } else { "" };
                lines.push(format!("  {status} {}{}", client.name, installed));
            } else {
                lines.push(format!("  ✗ {}", client.name));
            }
        }

        let resp = ok_text_response(id, lines.join("\n"));
        self.write_response(&resp)
    }

    fn handle_import_compose(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let scan_path = args
            .get("scan_path")
            .and_then(|v| v.as_str())
            .map(Path::new)
            .unwrap_or(&self.repo_root);

        let groups = match compose::scan_all(scan_path) {
            Ok(g) => g,
            Err(e) => return self.send_error(id, -32000, format!("Scan failed: {e}")),
        };

        let all_connections: Vec<compose::ComposeConnection> =
            groups.into_iter().flat_map(|(_, cs)| cs).collect();

        let text = if all_connections.is_empty() {
            "No PostgreSQL services found in docker-compose files.".into()
        } else {
            let project_name = scan_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project");
            match compose::write_config_files(scan_path, &all_connections, project_name) {
                Ok(import) => {
                    if let Err(e) = update_generated_by(&scan_path.join(".safeselect")) {
                        return self.send_error(
                            id,
                            -32000,
                            format!("Import metadata update failed: {e}"),
                        );
                    }
                    let names: Vec<&str> = all_connections
                        .iter()
                        .map(|c| c.env_name.as_str())
                        .collect();
                    if import.created == 0 {
                        "All environments already exist. Nothing imported.".to_string()
                    } else {
                        let mut parts = vec![format!(
                            "Imported {} connection(s): {}",
                            names.len(),
                            names.join(", ")
                        )];
                        if !import.no_password.is_empty() {
                            parts.push(String::new());
                            for (env, _) in &import.no_password {
                                parts.push(format!(
                                    "No password set for '{env}':\n{}",
                                    compose::secret_setup_hint(project_name, env)
                                ));
                            }
                        }
                        parts.join("\n")
                    }
                }
                Err(e) => return self.send_error(id, -32000, format!("Import failed: {e}")),
            }
        };

        let resp = ok_text_response(id, text);
        self.write_response(&resp)
    }

    fn handle_check(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let loader = ConfigLoader::new();
        let resolved = match loader.resolve_local(&self.repo_root, &self.env_name) {
            Ok(r) => r,
            Err(e) => return self.send_error(id, -32000, format!("Config resolution failed: {e}")),
        };

        let mut lines = vec![
            format!(
                "Checking configuration for {}/{}...",
                self.project_name, self.env_name
            ),
            diagnostics::line(
                DiagnosticStatus::Ok,
                DiagnosticCode::ConfigResolved,
                "Config resolved",
            ),
        ];

        lines.push(diagnostics::line(
            DiagnosticStatus::Ok,
            DiagnosticCode::DriverVerified,
            format!("Driver '{}' found and checksum OK", resolved.driver.vendor),
        ));
        lines.push(diagnostics::line(
            DiagnosticStatus::Ok,
            DiagnosticCode::SecretResolved,
            "Secret resolved",
        ));

        if let Some(ref ssh) = resolved.environment.ssh {
            if ssh.enabled {
                let bastion_host = ssh.host.as_deref().unwrap_or("unknown");
                let bastion_port = ssh.port.unwrap_or(22);
                lines.push(format!("  SSH bastion: {bastion_host}:{bastion_port}"));

                let mut postgres_reachable = false;
                if let Some((host, port)) =
                    crate::extract_host_port(&resolved.environment.database.url)
                {
                    postgres_reachable = crate::check_postgres_endpoint(&host, port);
                    if postgres_reachable {
                        lines.push(diagnostics::line(
                            DiagnosticStatus::Ok,
                            DiagnosticCode::PostgresReachable,
                            format!("PostgreSQL reachable at {host}:{port}"),
                        ));
                    }
                }

                if !postgres_reachable {
                    if crate::check_tcp_endpoint(
                        bastion_host,
                        bastion_port,
                        std::time::Duration::from_secs(3),
                    ) {
                        lines.push(diagnostics::line(
                            DiagnosticStatus::Ok,
                            DiagnosticCode::SshBastionReachable,
                            format!("SSH bastion reachable at {bastion_host}:{bastion_port}"),
                        ));
                    } else {
                        lines.push(diagnostics::line(
                            DiagnosticStatus::Fail,
                            DiagnosticCode::SshBastionUnreachable,
                            format!("SSH bastion unreachable at {bastion_host}:{bastion_port} (connect timed out after 3s)"),
                        ));
                        if let Some(ref identity_file) = ssh.identity_file {
                            if !std::path::Path::new(identity_file).exists() {
                                lines.push(diagnostics::line(
                                    DiagnosticStatus::Fail,
                                    DiagnosticCode::SshIdentityMissing,
                                    format!("SSH identity file not found: {identity_file}"),
                                ));
                            }
                        }
                        let resp = ok_text_response(id, lines.join("\n"));
                        return self.write_response(&resp);
                    }
                }

                // Check if PostgreSQL is reachable (direct or via tunnel)
                if let Some((host, port)) =
                    crate::extract_host_port(&resolved.environment.database.url)
                {
                    // If not reachable directly, try establishing SSH tunnel
                    let pg_reachable = if postgres_reachable {
                        true
                    } else {
                        lines.push(diagnostics::line(
                            DiagnosticStatus::Info,
                            DiagnosticCode::SshTunnelAttempt,
                            "Establishing SSH tunnel...",
                        ));
                        if let Err(e) = setup_ssh_tunnels(&self.repo_root, &[self.env_name.clone()])
                        {
                            lines.push(diagnostics::line(
                                DiagnosticStatus::Fail,
                                DiagnosticCode::SshTunnelFailed,
                                format!("SSH tunnel setup failed: {e}"),
                            ));
                            let resp = ok_text_response(id, lines.join("\n"));
                            return self.write_response(&resp);
                        }
                        crate::check_postgres_endpoint(&host, port)
                    };

                    match pg_reachable {
                        true => lines.push(diagnostics::line(
                            DiagnosticStatus::Ok,
                            DiagnosticCode::PostgresReachable,
                            format!("PostgreSQL reachable at {host}:{port}"),
                        )),
                        _ => {
                            lines.push(diagnostics::line(
                                DiagnosticStatus::Fail,
                                DiagnosticCode::PostgresUnreachable,
                                format!("PostgreSQL unreachable at {host}:{port} (read timed out after 2s)"),
                            ));
                            lines.push("  Possible causes:".into());
                            lines
                                .push(format!("    - Database host:port is wrong ({host}:{port})"));
                            lines.push(
                                "    - Database is not running or not accepting connections".into(),
                            );
                            lines.push(
                                "    - SSH tunnel is not established or not forwarding correctly"
                                    .into(),
                            );
                            let resp = ok_text_response(id, lines.join("\n"));
                            return self.write_response(&resp);
                        }
                    }
                }
            }
        }

        lines.push(diagnostics::line(
            DiagnosticStatus::Info,
            DiagnosticCode::SidecarStartAttempt,
            "Attempting sidecar connection...",
        ));

        match self.ensure_sidecar() {
            Ok(_) => {
                lines.push(diagnostics::line(
                    DiagnosticStatus::Ok,
                    DiagnosticCode::SidecarJdbcOk,
                    "Sidecar JDBC connection OK",
                ));
            }
            Err(e) => {
                lines.push(diagnostics::line(
                    DiagnosticStatus::Fail,
                    DiagnosticCode::SidecarConnectionFailed,
                    format!("Sidecar connection failed: {e}"),
                ));
                return self.send_error(id, -32000, lines.join("\n"));
            }
        }

        // Execute verification query
        match self
            .sidecar
            .as_mut()
            .unwrap()
            .execute("SELECT 1 AS connection_test")
        {
            Ok(result) => {
                lines.push(diagnostics::line(
                    DiagnosticStatus::Ok,
                    DiagnosticCode::QuerySelectOneOk,
                    format!(
                        "Connection verified: SELECT 1 returned {} row(s)",
                        result.row_count
                    ),
                ));
                lines.push(diagnostics::line(
                    DiagnosticStatus::Ok,
                    DiagnosticCode::AllChecksPassed,
                    format!(
                        "All checks passed for {}/{}",
                        self.project_name, self.env_name
                    ),
                ));
            }
            Err(e) => {
                lines.push(diagnostics::line(
                    DiagnosticStatus::Fail,
                    DiagnosticCode::QuerySelectOneFailed,
                    format!("Verification query failed: {e}"),
                ));
                return self.send_error(id, -32000, lines.join("\n"));
            }
        }

        let resp = ok_text_response(id, lines.join("\n"));
        self.write_response(&resp)
    }

    fn handle_reconnect(&mut self, id: Option<serde_json::Value>) -> Result<()> {
        let start = std::time::Instant::now();
        tracing::info!("Reconnect started");

        // Load config to check if SSH tunnel needs to be established
        let loader = ConfigLoader::new();
        if let Ok(resolved) = loader.resolve_local(&self.repo_root, &self.env_name) {
            if let Some(ref ssh) = resolved.environment.ssh {
                if ssh.enabled {
                    // Quick bastion reachability check before attempting tunnel
                    let bastion_host = ssh.host.as_deref().unwrap_or("unknown");
                    let bastion_port = ssh.port.unwrap_or(22);
                    tracing::info!(
                        "Checking bastion reachability: {}:{}",
                        bastion_host,
                        bastion_port
                    );

                    use std::net::ToSocketAddrs;
                    let bastion_addr = format!("{bastion_host}:{bastion_port}")
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut a| a.next());

                    match bastion_addr.as_ref() {
                        Some(a)
                            if std::net::TcpStream::connect_timeout(
                                a,
                                std::time::Duration::from_secs(3),
                            )
                            .is_ok() =>
                        {
                            tracing::info!("Bastion reachable ({:?})", start.elapsed());
                        }
                        Some(_) => {
                            return self.send_error(
                                id,
                                -32000,
                                format!(
                                    "SSH bastion unreachable at {}:{} (connect timed out after 3s). Cannot establish tunnel.",
                                    bastion_host, bastion_port
                                ),
                            );
                        }
                        None => {
                            return self.send_error(
                                id,
                                -32000,
                                format!(
                                    "Cannot resolve SSH bastion {}:{}",
                                    bastion_host, bastion_port
                                ),
                            );
                        }
                    }

                    tracing::info!(
                        "Establishing SSH tunnel before reconnect ({:?})",
                        start.elapsed()
                    );
                    if let Err(e) = setup_ssh_tunnels(&self.repo_root, &[self.env_name.clone()]) {
                        return self.send_error(
                            id,
                            -32000,
                            format!("SSH tunnel setup failed: {e}"),
                        );
                    }
                    tracing::info!("SSH tunnel established ({:?})", start.elapsed());
                }
            }
        }

        tracing::info!("Restarting sidecar ({:?})", start.elapsed());
        match self.restart_sidecar() {
            Ok(()) => {
                tracing::info!("Sidecar restarted ({:?})", start.elapsed());
            }
            Err(e) => return self.send_error(id, -32000, format!("Reconnect failed: {e}")),
        }

        let sidecar = match self.sidecar.as_mut() {
            Some(s) => s,
            None => return self.send_error(id, -32000, "Sidecar not available after restart"),
        };

        tracing::info!("Pinging sidecar ({:?})", start.elapsed());
        if let Err(e) = sidecar.ping() {
            return self.send_error(id, -32000, format!("Ping failed: {e}"));
        }
        tracing::info!("Ping OK ({:?})", start.elapsed());

        tracing::info!("Executing verification query ({:?})", start.elapsed());
        match sidecar.execute("SELECT 1 AS connection_test") {
            Ok(result) => {
                tracing::info!("Verification query completed ({:?})", start.elapsed());
                let text = format!(
                    "Reconnected and verified in {:?}.\n  ✓ Sidecar restarted\n  ✓ Ping OK\n  ✓ SELECT 1 returned {} row(s)",
                    start.elapsed(),
                    result.row_count
                );
                let resp = ok_text_response(id, text);
                self.write_response(&resp)
            }
            Err(e) => self.send_error(id, -32000, format!("Verification query failed: {e}")),
        }
    }

    fn handle_uninstall(
        &mut self,
        id: Option<serde_json::Value>,
        args: &serde_json::Value,
    ) -> Result<()> {
        let confirm = args
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !confirm {
            return self.send_error(id, -32000, "Set 'confirm' to true to uninstall safeselect");
        }

        let mut removed_anything = false;
        let mut lines = vec![];

        let bin = dirs::home_dir().map(|h| h.join(".local").join("bin").join("safeselect"));
        if let Some(ref path) = bin.filter(|p| p.exists()) {
            if std::fs::remove_file(path).is_ok() {
                lines.push(format!("  ✓ Removed {}", path.display()));
                removed_anything = true;
            }
        }

        let config_dir = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
            .join(".config/safeselect");
        if config_dir.exists() && std::fs::remove_dir_all(&config_dir).is_ok() {
            lines.push(format!("  ✓ Removed {}", config_dir.display()));
            removed_anything = true;
        }

        if let Some(data_dir) = dirs::data_dir().map(|d| d.join("safeselect")) {
            if data_dir.exists() && std::fs::remove_dir_all(&data_dir).is_ok() {
                lines.push(format!("  ✓ Removed {}", data_dir.display()));
                removed_anything = true;
            }
        }

        let audit_dir = dirs::home_dir().map(|h| h.join(".local").join("state").join("safeselect"));
        if let Some(ref path) = audit_dir {
            if path.exists() && std::fs::remove_dir_all(path).is_ok() {
                lines.push(format!("  ✓ Removed {}", path.display()));
                removed_anything = true;
            }
        }

        let keychain_result = std::process::Command::new("security")
            .args(["delete-generic-password", "-s", "safeselect"])
            .output();
        if let Ok(output) = keychain_result {
            if output.status.success() {
                lines.push("  ✓ Removed macOS Keychain entries for 'safeselect'".into());
                removed_anything = true;
            }
        }

        if !removed_anything {
            lines.push("  Nothing to remove.".into());
        }
        lines.push("  Uninstall complete.".into());

        let resp = ok_text_response(id, lines.join("\n"));
        self.write_response(&resp)
    }

    fn fail_closed(&self, reason: &str) {
        tracing::error!("FAIL-CLOSED: {reason}");
        std::process::exit(1);
    }

    fn send_error<T: ToString>(
        &mut self,
        id: Option<serde_json::Value>,
        code: i64,
        message: T,
    ) -> Result<()> {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        };
        self.write_response(&resp)
    }

    fn write_response(&self, resp: &JsonRpcResponse) -> Result<()> {
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        let line = serde_json::to_string(resp)?;
        writeln!(writer, "{line}")?;
        writer.flush()?;
        Ok(())
    }
}

fn ok_text_response(id: Option<serde_json::Value>, text: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(serde_json::json!({
            "content": [{
                "type": "text",
                "text": text
            }]
        })),
        error: None,
    }
}

pub fn run_setup_server(repo_root: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: JsonRpcMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: "Parse error".into(),
                        data: Some(serde_json::json!({"detail": e.to_string()})),
                    }),
                };
                write_setup_response(&resp)?;
                continue;
            }
        };

        let method = match msg.method.as_deref() {
            Some(m) => m,
            None => continue,
        };

        match method {
            "initialize" => {
                let proto_version = msg
                    .params
                    .as_ref()
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("2024-11-05");

                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: msg.id.clone(),
                    result: Some(serde_json::json!({
                        "protocolVersion": proto_version,
                        "capabilities": {
                            "tools": {
                                "list": {}
                            }
                        },
                        "serverInfo": {
                            "name": "safeselect-setup",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    })),
                    error: None,
                };
                write_setup_response(&resp)?;
            }
            "tools/list" => {
                let tools = vec![
                    ToolDefinition {
                        name: "import_compose".into(),
                        description:
                            "Scan docker-compose files for PostgreSQL services and import them into .safeselect/ configuration. Creates project.toml and environment files automatically."
                                .into(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "scan_path": {
                                    "type": "string",
                                    "description": "Directory to scan for docker-compose files (default: project root)"
                                }
                            }
                        }),
                    },
                    ToolDefinition {
                        name: "delete_environment".into(),
                        description: "Delete an environment configuration from the project".into(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "name": {
                                    "type": "string",
                                    "description": "Environment name to delete"
                                }
                            },
                            "required": ["name"]
                        }),
                    },
                    ToolDefinition {
                        name: "rename_environment".into(),
                        description: "Rename an environment within the project".into(),
                        input_schema: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "old_name": {
                                    "type": "string",
                                    "description": "Current environment name"
                                },
                                "new_name": {
                                    "type": "string",
                                    "description": "New environment name"
                                }
                            },
                            "required": ["old_name", "new_name"]
                        }),
                    },
                ];

                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: msg.id,
                    result: Some(serde_json::json!({ "tools": tools })),
                    error: None,
                };
                write_setup_response(&resp)?;
            }
            "tools/call" => {
                let params = match msg.params.as_ref() {
                    Some(p) => p,
                    None => {
                        let resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id: msg.id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: "Missing params".into(),
                                data: None,
                            }),
                        };
                        write_setup_response(&resp)?;
                        continue;
                    }
                };

                let tool_name = match params.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => {
                        let resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id: msg.id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: "Missing tool name".into(),
                                data: None,
                            }),
                        };
                        write_setup_response(&resp)?;
                        continue;
                    }
                };

                match tool_name {
                    "import_compose" => {
                        let args = params
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));

                        let scan_path = args
                            .get("scan_path")
                            .and_then(|v| v.as_str())
                            .map(Path::new)
                            .unwrap_or(repo_root);

                        match compose::scan_all(scan_path) {
                            Ok(groups) => {
                                let all_connections: Vec<compose::ComposeConnection> =
                                    groups.into_iter().flat_map(|(_, cs)| cs).collect();

                                if all_connections.is_empty() {
                                    let resp = JsonRpcResponse {
                                        jsonrpc: "2.0",
                                        id: msg.id,
                                        result: Some(serde_json::json!({
                                            "content": [{
                                                "type": "text",
                                                "text": "No PostgreSQL services found in docker-compose files."
                                            }]
                                        })),
                                        error: None,
                                    };
                                    write_setup_response(&resp)?;
                                } else {
                                    let project_name = scan_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("project");
                                    let result = compose::write_config_files(
                                        scan_path,
                                        &all_connections,
                                        project_name,
                                    );
                                    match result {
                                        Ok(import) => {
                                            let names: Vec<&str> = all_connections
                                                .iter()
                                                .map(|c| c.env_name.as_str())
                                                .collect();
                                            let text = if import.created == 0 {
                                                "All environments already exist. Nothing imported."
                                                    .to_string()
                                            } else {
                                                let mut parts = vec![format!(
                                                    "Imported {} connection(s): {}",
                                                    names.len(),
                                                    names.join(", ")
                                                )];
                                                if !import.no_password.is_empty() {
                                                    parts.push(String::new());
                                                    for (env, _) in &import.no_password {
                                                        parts.push(format!(
                                                            "No password set for '{env}':\n{}",
                                                            crate::compose::secret_setup_hint(
                                                                project_name,
                                                                env
                                                            )
                                                        ));
                                                    }
                                                }
                                                parts.push(String::new());
                                                parts.push("Run the server with:".to_string());
                                                for env_name in &import.env_names {
                                                    parts.push(format!(
                                                        "  safeselect serve --environment {env_name}"
                                                    ));
                                                }
                                                parts.join("\n")
                                            };
                                            let resp = JsonRpcResponse {
                                                jsonrpc: "2.0",
                                                id: msg.id,
                                                result: Some(serde_json::json!({
                                                    "content": [{
                                                        "type": "text",
                                                        "text": text
                                                    }]
                                                })),
                                                error: None,
                                            };
                                            write_setup_response(&resp)?;
                                        }
                                        Err(e) => {
                                            let resp = JsonRpcResponse {
                                                jsonrpc: "2.0",
                                                id: msg.id,
                                                result: None,
                                                error: Some(JsonRpcError {
                                                    code: -32000,
                                                    message: format!("Import failed: {e}"),
                                                    data: None,
                                                }),
                                            };
                                            write_setup_response(&resp)?;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id: msg.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32000,
                                        message: format!("Scan failed: {e}"),
                                        data: None,
                                    }),
                                };
                                write_setup_response(&resp)?;
                            }
                        }
                    }
                    "delete_environment" => {
                        let args = params
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));

                        let name = match args.get("name").and_then(|v| v.as_str()) {
                            Some(n) => n,
                            None => {
                                let resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id: msg.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32602,
                                        message: "Missing 'name'".into(),
                                        data: None,
                                    }),
                                };
                                write_setup_response(&resp)?;
                                continue;
                            }
                        };

                        let env_dir = repo_root.join(".safeselect").join("environments");
                        let env_file = env_dir.join(format!("{name}.toml"));

                        let text = if !env_file.exists() {
                            format!("Environment '{name}' not found")
                        } else {
                            // Try to clean up the Keychain entry before deleting
                            let old_content = std::fs::read_to_string(&env_file).ok();
                            if let Some(c) = old_content {
                                if let Ok(env_config) =
                                    toml::from_str::<crate::config::EnvironmentConfig>(&c)
                                {
                                    if let Some(secret) = env_config.database.secret {
                                        if secret.source == "macos-keychain" {
                                            if let Some(ref acct) = secret.account {
                                                let _ =
                                                    crate::compose::delete_password_from_keychain(
                                                        acct,
                                                    );
                                            }
                                        }
                                    }
                                }
                            }
                            match std::fs::remove_file(&env_file) {
                                Ok(()) => format!("Deleted environment '{name}'"),
                                Err(e) => format!("Delete failed: {e}"),
                            }
                        };

                        let resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id: msg.id,
                            result: Some(serde_json::json!({
                                "content": [{
                                    "type": "text",
                                    "text": text
                                }]
                            })),
                            error: None,
                        };
                        write_setup_response(&resp)?;
                    }
                    "rename_environment" => {
                        let args = params
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));

                        let old_name = match args.get("old_name").and_then(|v| v.as_str()) {
                            Some(n) => n,
                            None => {
                                let resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id: msg.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32602,
                                        message: "Missing 'old_name'".into(),
                                        data: None,
                                    }),
                                };
                                write_setup_response(&resp)?;
                                continue;
                            }
                        };
                        let new_name = match args.get("new_name").and_then(|v| v.as_str()) {
                            Some(n) => n,
                            None => {
                                let resp = JsonRpcResponse {
                                    jsonrpc: "2.0",
                                    id: msg.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32602,
                                        message: "Missing 'new_name'".into(),
                                        data: None,
                                    }),
                                };
                                write_setup_response(&resp)?;
                                continue;
                            }
                        };

                        let env_dir = repo_root.join(".safeselect").join("environments");
                        let old_file = env_dir.join(format!("{old_name}.toml"));
                        let new_file = env_dir.join(format!("{new_name}.toml"));

                        let text = if !old_file.exists() {
                            format!("Environment '{old_name}' not found")
                        } else if new_file.exists() {
                            format!("Environment '{new_name}' already exists")
                        } else {
                            let project_name = repo_root
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("project");
                            let old_account = format!("{project_name}/{old_name}");
                            let new_account = format!("{project_name}/{new_name}");

                            // Migrate secret before renaming
                            let old_content = std::fs::read_to_string(&old_file).ok();
                            let needs_rewrite = old_content.as_ref().and_then(|c| {
                                let mut env: crate::config::EnvironmentConfig =
                                    toml::from_str(c).ok()?;
                                let secret = env.database.secret.as_mut()?;
                                match secret.source.as_str() {
                                    "macos-keychain" if cfg!(target_os = "macos") => {
                                        let pw = crate::compose::read_password_from_keychain(
                                            &old_account,
                                        )
                                        .ok()?;
                                        crate::compose::store_password_in_keychain(
                                            &new_account,
                                            &pw,
                                        )
                                        .ok()?;
                                        crate::compose::delete_password_from_keychain(&old_account)
                                            .ok()?;
                                        secret.account = Some(new_account);
                                        Some(env)
                                    }
                                    "env" => {
                                        let var = format!(
                                            "SAFESELECT_PASSWORD_{}",
                                            new_name.to_uppercase().replace('-', "_")
                                        );
                                        secret.variable = Some(var);
                                        Some(env)
                                    }
                                    _ => None,
                                }
                            });

                            let migrated = needs_rewrite.is_some();
                            match std::fs::rename(&old_file, &new_file) {
                                Ok(()) => {
                                    if let Some(env) = needs_rewrite {
                                        if let Ok(content) = toml::to_string_pretty(&env) {
                                            let _ = std::fs::write(&new_file, content);
                                        }
                                    }
                                    let mut msg = format!("Renamed '{old_name}' → '{new_name}'");
                                    if migrated {
                                        msg.push_str("\nSecret migrated automatically.");
                                    }
                                    msg
                                }
                                Err(e) => format!("Rename failed: {e}"),
                            }
                        };

                        let resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id: msg.id,
                            result: Some(serde_json::json!({
                                "content": [{
                                    "type": "text",
                                    "text": text
                                }]
                            })),
                            error: None,
                        };
                        write_setup_response(&resp)?;
                    }
                    _ => {
                        let resp = JsonRpcResponse {
                            jsonrpc: "2.0",
                            id: msg.id,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32602,
                                message: format!("Unknown tool: {tool_name}"),
                                data: None,
                            }),
                        };
                        write_setup_response(&resp)?;
                    }
                }
            }
            "notifications/initialized" => {}
            _ => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: msg.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Method not found: {method}"),
                        data: None,
                    }),
                };
                write_setup_response(&resp)?;
            }
        }
    }

    Ok(())
}

fn write_setup_response(resp: &JsonRpcResponse) -> Result<()> {
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let line = serde_json::to_string(resp)?;
    writeln!(writer, "{line}")?;
    writer.flush()?;
    Ok(())
}

fn is_sidecar_timeout(message: &str) -> bool {
    message.contains("did not respond within") || message.contains("poll error")
}

fn is_recoverable_connection_error(message: &str) -> bool {
    let msg = message.to_lowercase();
    msg.contains("sql_error")
        || msg.contains("sqlstate 08")
        || msg.contains("sql_state\":\"08")
        || msg.contains("08006")
        || msg.contains("08001")
        || msg.contains("57p01")
        || msg.contains("connection refused")
        || msg.contains("connection is closed")
        || msg.contains("broken pipe")
        || msg.contains("eof")
        || msg.contains("sidecar process terminated")
        || msg.contains("not_connected")
        || msg.contains("database not connected")
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_' {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}
