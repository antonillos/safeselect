use crate::audit::AuditLog;
use crate::config::{EnvironmentConfig, ProjectConfig};
use crate::error::Result;
use crate::security::SecurityEngine;
use crate::sidecar::{QueryResult, SidecarProcess};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};

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
    input_schema: serde_json::Value,
}

pub struct McpServer {
    sidecar: SidecarProcess,
    security: SecurityEngine,
    audit: AuditLog,
    project_name: String,
    env_name: String,
    client_name: String,
}

impl McpServer {
    pub fn new(
        sidecar: SidecarProcess,
        project_config: ProjectConfig,
        _env_config: EnvironmentConfig,
        project_name: &str,
        env_name: &str,
    ) -> Result<Self> {
        let security = SecurityEngine::new(project_config.security.clone(), project_config.limits.clone());

        let _limits = project_config.limits.clone();

        let audit = AuditLog::open(
            &project_config.audit,
            project_name,
            env_name,
            "unknown",
        )?;

        Ok(Self {
            sidecar,
            security,
            audit,
            project_name: project_name.to_string(),
            env_name: env_name.to_string(),
            client_name: "unknown".to_string(),
        })
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
        if let Some(ref params) = msg.params {
            if let Some(client_info) = params.get("clientInfo") {
                if let Some(name) = client_info.get("name").and_then(|v| v.as_str()) {
                    self.client_name = name.to_string();
                }
            }
        }

        let resp = JsonRpcResponse {
            jsonrpc: "2.0",
            id: msg.id.clone(),
            result: Some(serde_json::json!({
                "protocolVersion": "0.1.0",
                "capabilities": {
                    "tools": {}
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

        let result = self.sidecar.execute(sql);

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
        let sql = match schema {
            Some(s) => format!(
                "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema = '{}' ORDER BY table_schema, table_name",
                s.replace('\'', "''")
            ),
            None => {
                "SELECT table_schema, table_name, table_type FROM information_schema.tables ORDER BY table_schema, table_name".into()
            }
        };

        match self.security.validate(&sql) {
            Ok(()) => {}
            Err(e) => {
                self.audit.record("REJECT", "reject", &sql)?;
                let _ = self.send_error(id, -32000, format!("Query rejected: {e}"));
                self.fail_closed("Security violation");
                return Ok(());
            }
        }

        match self.sidecar.execute(&sql) {
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

        match self.sidecar.execute(&explain_sql) {
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
