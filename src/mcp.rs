use crate::audit::AuditLog;
use crate::compose;
use crate::config::{EnvironmentConfig, ProjectConfig};
use crate::error::{Result, SafeselectError};
use crate::security::SecurityEngine;
use crate::sidecar::SidecarProcess;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

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
    // Stored to lazily start the sidecar on first query
    driver_path: String,
    driver_class: String,
    db_url: String,
    db_username: String,
    db_password: String,
}

impl McpServer {
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
    ) -> Result<Self> {
        let security = SecurityEngine::new(project_config.security.clone(), project_config.limits.clone());

        let idle_timeout_seconds = env_config.limits.idle_timeout_seconds.unwrap_or(0);

        let audit = AuditLog::open(
            &project_config.audit,
            project_name,
            env_name,
            "unknown",
        )?;

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
        let (client_name, proto_version) = match msg.params {
            Some(ref params) => {
                let name = params
                    .get("clientInfo")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let version = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("2024-11-05");
                (name.to_string(), version.to_string())
            }
            None => ("unknown".into(), "2024-11-05".into()),
        };
        self.client_name = client_name;

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

        let args = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));

        match tool_name {
            "select" => self.handle_select(msg.id.clone(), &args),
            "list_tables" => self.handle_list_tables(msg.id.clone(), &args),
            "explain" => self.handle_explain(msg.id.clone(), &args),
            "disconnect" => self.handle_disconnect(msg.id.clone()),
            "connect" => self.handle_connect(msg.id.clone()),
            _ => self.send_error(msg.id.clone(), -32602, format!("Unknown tool: {tool_name}")),
        }
    }

    fn handle_select(&mut self, id: Option<serde_json::Value>, args: &serde_json::Value) -> Result<()> {
        let sql = match args.get("sql").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return self.send_error(id, -32602, "Missing 'sql' argument"),
        };

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
                if let Err(e) = self.security.check_result_size(query_result.row_count, query_result.byte_count) {
                    self.audit.record("LIMIT_EXCEEDED", "reject", sql)?;
                    let _ = self.send_error(id, -32000, format!("{e}"));
                    self.fail_closed("Limit exceeded");
                    return Ok(());
                }
                self.audit.record("PASS", "allow", sql)?;
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
                self.audit.record("JDBC_ERROR", "error", sql)?;
                self.send_error(id, -32000, format!("Query execution failed: {e}"))
            }
        }
    }

    fn handle_list_tables(&mut self, id: Option<serde_json::Value>, args: &serde_json::Value) -> Result<()> {
        let schema = args.get("schema").and_then(|v| v.as_str());

        let allowed = self.security.allowed_schemas();
        let sql = match schema {
            Some(s) if is_valid_identifier(s) => {
                if !allowed.is_empty() && !allowed.iter().any(|a| a == s) {
                    return self.send_error(id, -32000, format!(
                        "Schema '{s}' is not in the allowed schemas list ({})",
                        allowed.join(", ")
                    ));
                }
                format!(
                    "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema = '{}' ORDER BY table_schema, table_name",
                    s
                )
            }
            Some(_) => {
                return self.send_error(id, -32602, "Invalid schema name: only alphanumeric and underscores allowed");
            }
            None => {
                if allowed.is_empty() {
                    "SELECT table_schema, table_name, table_type FROM information_schema.tables ORDER BY table_schema, table_name".into()
                } else {
                    let schemas: Vec<String> = allowed.iter().map(|s| {
                        format!("'{}'", s.replace('\'', "''"))
                    }).collect();
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

    fn handle_explain(&mut self, id: Option<serde_json::Value>, args: &serde_json::Value) -> Result<()> {
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
                self.audit.record("DISCONNECT", "allow", "manual disconnect")?;
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
    fn execute_with_reconnect(&mut self, sql: &str) -> std::result::Result<crate::sidecar::QueryResult, crate::error::SafeselectError> {
        self.ensure_sidecar()?;
        let result = self.sidecar.as_mut().unwrap().execute(sql);
        if result.is_ok() {
            return result;
        }
        let reconnected = self.sidecar.as_mut().unwrap().connect().is_ok();
        if reconnected {
            let _ = self.audit.record("AUTO_RECONNECT", "allow", "connection lost — reconnected");
            self.sidecar.as_mut().unwrap().execute(sql)
        } else {
            result
        }
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
                                                            crate::compose::secret_setup_hint(project_name, env)
                                                        ));
                                                    }
                                                }
                                                parts.push(String::new());
                                                parts.push(
                                                    "Run the server with:".to_string(),
                                                );
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
                                                let _ = crate::compose::delete_password_from_keychain(acct);
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
                                        crate::compose::delete_password_from_keychain(
                                            &old_account,
                                        )
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
                                    let mut msg =
                                        format!("Renamed '{old_name}' → '{new_name}'");
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

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_' {
        return false;
    }
    bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}
