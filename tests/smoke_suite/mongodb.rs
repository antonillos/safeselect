//! Shared MongoDB real-test helpers.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub const TEST_PASSWORD: &str = "testpass";

pub fn test_db() -> String {
    let suffix = std::env::var("SAFESELECT_TEST_SUFFIX")
        .unwrap_or_else(|_| std::process::id().to_string())
        .replace('-', "_");
    format!("safeselect_mongo_{suffix}")
}

pub fn test_user() -> String {
    let suffix = std::env::var("SAFESELECT_TEST_SUFFIX")
        .unwrap_or_else(|_| std::process::id().to_string())
        .replace('-', "_");
    format!("safeselect_mongo_user_{suffix}")
}

pub fn safeselect_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_safeselect"))
}

pub fn setup_database() {
    wait_for_mongodb();
    cleanup_database();

    mongo_eval(&format!(
        r#"
const dbName = "{db_name}";
const userName = "{user_name}";
const userPassword = "{password}";
const db = db.getSiblingDB(dbName);
db.createCollection("safe_docs");
db.createCollection("large_docs");
db.createCollection("secret_docs");
db.safe_docs.insertMany([
  {{ _id: 1, name: "alpha", active: true, category: "safe" }},
  {{ _id: 2, name: "beta", active: true, category: "safe" }},
  {{ _id: 3, name: "gamma", active: false, category: "safe" }}
]);
db.large_docs.insertOne({{ _id: 1, payload: "{large_payload}" }});
db.secret_docs.insertOne({{ _id: 1, secret: "top-secret" }});
db.createUser({{
  user: userName,
  pwd: userPassword,
  roles: [{{ role: "read", db: dbName }}]
}});
"#,
        db_name = test_db(),
        user_name = test_user(),
        password = TEST_PASSWORD,
        large_payload = "z".repeat(2000),
    ));
}

pub fn cleanup_database() {
    mongo_eval(&format!(
        r#"
const dbName = "{db_name}";
const userName = "{user_name}";
const db = db.getSiblingDB(dbName);
try {{ db.dropUser(userName); }} catch (e) {{}}
db.dropDatabase();
"#,
        db_name = test_db(),
        user_name = test_user(),
    ));
}

pub fn wait_for_mongodb() {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::panic::catch_unwind(|| mongo_eval("db.adminCommand({ ping: 1 })")).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("MongoDB did not become ready");
}

pub fn write_config(root: &Path) {
    let safeselect_dir = root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir).unwrap();

    std::fs::write(
        safeselect_dir.join("project.toml"),
        format!(
            r#"
version = 1
display_name = "SafeSelect Mongo Security Test"

[security]
allowed_databases = ["{db_name}"]
allowed_collections = ["{db_name}.safe_docs", "{db_name}.large_docs"]
denied_collections = ["{db_name}.secret_docs"]
require_single_statement = true

[limits]
statement_timeout_ms = 1000
max_rows = 2
max_result_bytes = 1000

[audit]
enabled = true
"#,
            db_name = test_db(),
        ),
    )
    .unwrap();

    std::fs::write(
        env_dir.join("testing.toml"),
        format!(
            r#"
version = 1

[database]
kind = "document"
vendor = "mongodb"
url = "mongodb://{user}:__SAFESELECT_PASSWORD__@{host}:{port}/{db}?authSource={db}"
username = "{user}"

[database.secret]
source = "env"
variable = "SAFESELECT_MONGODB_TEST_PASSWORD"
"#,
            user = test_user(),
            host = mongo_host(),
            port = mongo_port(),
            db = test_db(),
        ),
    )
    .unwrap();
}

pub fn collection_count(collection: &str) -> String {
    mongo_eval(&format!(
        "db.getSiblingDB('{db_name}').{collection}.countDocuments({{}})",
        db_name = test_db()
    ))
}

pub fn collection_exists(collection: &str) -> String {
    mongo_eval(&format!(
        "db.getSiblingDB('{db_name}').getCollectionNames().includes('{collection}')",
        db_name = test_db()
    ))
}

pub struct McpHarness {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    stderr: Option<ChildStderr>,
}

impl McpHarness {
    pub fn start(root: &Path, config_dir: &Path) -> Self {
        let mut child = Command::new(safeselect_bin())
            .args([
                "serve",
                "--project",
                root.to_str().unwrap(),
                "--environment",
                "testing",
            ])
            .env("SAFESELECT_CONFIG_DIR", config_dir)
            .env("SAFESELECT_MONGODB_TEST_PASSWORD", TEST_PASSWORD)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start MCP server");

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take();
        let mut harness = Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            stderr,
        };
        harness.initialize();
        harness
    }

    fn initialize(&mut self) {
        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "safeselect-mongo-security-test"
                }
            }
        }));
        let response = self.read_json_response();
        let text = response
            .get("result")
            .and_then(|result| result.get("serverInfo"))
            .map(|_| response.to_string())
            .unwrap_or_else(|| response.to_string());
        assert!(
            text.contains("safeselect"),
            "unexpected initialize response: {text}"
        );

        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    pub fn call_tool(&mut self, id: u64, name: &str, arguments: Value) -> ToolResponse {
        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }));

        let response = self.read_json_response();
        if let Some(error) = response.get("error") {
            let text = error
                .get("message")
                .and_then(|message| message.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            return ToolResponse {
                success: false,
                text,
            };
        }

        let result = response.get("result").cloned().unwrap_or(Value::Null);
        let text = result
            .get("content")
            .and_then(|content| content.get(0))
            .and_then(|content| content.get("text"))
            .and_then(|text| text.as_str())
            .unwrap_or("")
            .to_string();
        let is_error = result
            .get("isError")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(false);
        ToolResponse {
            success: !is_error,
            text,
        }
    }

    pub fn list_tools(&mut self, id: u64) -> Value {
        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list"
        }));
        self.read_json_response()
    }

    fn send_json(&mut self, value: &Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_json_response(&mut self) -> Value {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .expect("failed to read MCP response");
        if line.is_empty() {
            let mut stderr = String::new();
            if let Some(stream) = self.stderr.as_mut() {
                let _ = stream.read_to_string(&mut stderr);
            }
            panic!("MCP server exited unexpectedly, stderr:\n{stderr}");
        }
        serde_json::from_str(line.trim()).expect("invalid JSON-RPC response")
    }
}

impl Drop for McpHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ToolResponse {
    pub success: bool,
    pub text: String,
}

fn mongo_eval(script: &str) -> String {
    let output = Command::new("docker")
        .args([
            "exec",
            mongo_container().as_str(),
            "mongosh",
            admin_uri().as_str(),
            "--quiet",
            "--eval",
            script,
        ])
        .output()
        .expect("failed to run mongosh in Docker");

    assert!(
        output.status.success(),
        "mongosh failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn mongo_container() -> String {
    if let Ok(container) = std::env::var("SAFESELECT_MONGODB_DOCKER_CONTAINER") {
        return container;
    }

    let output = Command::new("docker")
        .args(["ps", "--format", "{{.Names}}"])
        .output()
        .expect("failed to list Docker containers");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| line.contains("mongodb"))
        .unwrap_or("safeselect-mongodb-1")
        .to_string()
}

fn admin_uri() -> String {
    format!(
        "mongodb://{}:{}@localhost:27017/admin",
        mongo_admin_user(),
        mongo_admin_password()
    )
}

fn mongo_host() -> String {
    std::env::var("SAFESELECT_MONGODB_HOST").unwrap_or_else(|_| "localhost".into())
}

fn mongo_port() -> String {
    std::env::var("SAFESELECT_MONGODB_PORT").unwrap_or_else(|_| "27017".into())
}

fn mongo_admin_user() -> String {
    std::env::var("SAFESELECT_MONGODB_ADMIN_USER").unwrap_or_else(|_| "root".into())
}

fn mongo_admin_password() -> String {
    std::env::var("SAFESELECT_MONGODB_ADMIN_PASSWORD").unwrap_or_else(|_| TEST_PASSWORD.into())
}
