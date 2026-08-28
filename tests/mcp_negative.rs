use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct McpHarness {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: ChildStderr,
}

impl McpHarness {
    fn start() -> (Self, PathBuf) {
        let tmp =
            std::env::temp_dir().join(format!("safeselect-mcp-negative-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&tmp);
        let repo_root = tmp.join("repo");
        let safeselect_dir = repo_root.join(".safeselect");
        let environment_dir = safeselect_dir.join("environments");
        let config_dir = tmp.join("config");
        let driver_dir = config_dir.join("drivers");
        let audit_dir = tmp.join("audit");
        std::fs::create_dir_all(&environment_dir).unwrap();
        std::fs::create_dir_all(&driver_dir).unwrap();

        std::fs::write(
            safeselect_dir.join("project.toml"),
            format!(
                r#"
version = 1
display_name = "MCP Negative Validation"

[security]
require_single_statement = true

[audit]
enabled = true
directory = "{}"
max_file_bytes = 1000000
retain_files = 2
"#,
                audit_dir.display()
            ),
        )
        .unwrap();
        std::fs::write(
            environment_dir.join("testing.toml"),
            r#"
version = 1

[database]
kind = "jdbc"
vendor = "postgresql"
driver = "postgresql"
url = "jdbc:postgresql://127.0.0.1:1/unused"
username = "unused"

[database.secret]
source = "env"
variable = "SAFESELECT_MCP_NEGATIVE_TEST_PASSWORD"
"#,
        )
        .unwrap();

        let jar_path = tmp.join("unused.jar");
        std::fs::write(&jar_path, []).unwrap();
        std::fs::write(
            driver_dir.join("postgresql.toml"),
            format!(
                r#"
version = 1
vendor = "postgresql"
path = "{}"
class = "org.postgresql.Driver"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
"#,
                jar_path.display()
            ),
        )
        .unwrap();

        let mut child = Command::new(safeselect_bin())
            .args([
                "serve",
                "--project",
                repo_root.to_str().unwrap(),
                "--environment",
                "testing",
            ])
            .env("SAFESELECT_CONFIG_DIR", &config_dir)
            .env("SAFESELECT_MCP_NEGATIVE_TEST_PASSWORD", "unused")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to start MCP server");

        (
            Self {
                stdin: child.stdin.take().unwrap(),
                stdout: BufReader::new(child.stdout.take().unwrap()),
                stderr: child.stderr.take().unwrap(),
                child,
            },
            tmp,
        )
    }

    fn send(&mut self, request: &serde_json::Value) -> serde_json::Value {
        self.send_raw(&request.to_string())
    }

    fn send_raw(&mut self, request: &str) -> serde_json::Value {
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        if line.is_empty() {
            let mut stderr = String::new();
            std::io::Read::read_to_string(&mut self.stderr, &mut stderr).unwrap();
            panic!("MCP server closed stdout before responding\nstderr:\n{stderr}");
        }
        serde_json::from_str(&line).expect("stdout must contain JSON-RPC only")
    }

    fn send_without_response(&mut self, request: &str) {
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn finish(mut self) -> String {
        drop(self.stdin);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "MCP server did not exit after EOF"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut stderr = String::new();
        std::io::Read::read_to_string(&mut self.stderr, &mut stderr).unwrap();
        stderr
    }
}

fn safeselect_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_safeselect"))
}

fn assert_error(response: &serde_json::Value, id: serde_json::Value, code: i64) {
    assert_eq!(
        response["jsonrpc"], "2.0",
        "invalid JSON-RPC response: {response}"
    );
    assert_eq!(response["id"], id, "response id mismatch: {response}");
    assert_eq!(
        response["error"]["code"], code,
        "unexpected response: {response}"
    );
    assert!(
        response["error"]["data"]["next_suggestion"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "error must contain safe next-step guidance: {response}"
    );
}

#[test]
fn mcp_negative_validation_preserves_jsonrpc_and_recovers_until_eof() {
    let (mut mcp, tmp) = McpHarness::start();
    let secret = "never-echo-this-secret";
    let deep_argument = serde_json::json!({"nested": [vec!["x"; 256]]});
    let large_payload = "x".repeat(256 * 1024);
    let malformed = mcp.send_raw("{\"jsonrpc\":\"2.0\",\"id\":0");
    assert_error(&malformed, serde_json::Value::Null, -32700);

    let cases = [
        (
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": 42}),
            serde_json::Value::Null,
            -32700,
        ),
        (
            serde_json::json!({"jsonrpc": "2.0", "id": 2}),
            serde_json::json!(2),
            -32600,
        ),
        (
            serde_json::json!({"jsonrpc": "1.0", "id": 3, "method": "tools/list"}),
            serde_json::json!(3),
            -32600,
        ),
        (
            serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "not/real", "extra": true}),
            serde_json::json!(4),
            -32601,
        ),
        (
            serde_json::json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call"}),
            serde_json::json!(5),
            -32602,
        ),
        (
            serde_json::json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "not_real", "arguments": null}}),
            serde_json::json!(6),
            -32602,
        ),
    ];

    for (request, id, code) in cases {
        let response = mcp.send(&request);
        assert_error(&response, id, code);
        assert!(
            !response.to_string().contains(secret),
            "error response leaked test secret: {response}"
        );
    }

    let response = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/list",
        "params": deep_argument,
        "extra": secret,
    }));
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 7);
    assert!(
        response["result"]["tools"].is_array(),
        "unexpected response: {response}"
    );
    assert!(
        !response.to_string().contains(secret),
        "successful response leaked unknown payload data: {response}"
    );

    let response = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/list",
        "extra": large_payload,
    }));
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 8);
    assert!(
        response["result"]["tools"].is_array(),
        "large unknown frame did not recover safely: {response}"
    );

    let stderr = mcp.finish();
    assert!(
        !stderr.contains(secret),
        "stderr leaked unknown request payload: {stderr}"
    );
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn postgres_posture_preflight_does_not_block_or_close_mcp() {
    let (mut mcp, tmp) = McpHarness::start();

    let query = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "select", "arguments": {"sql": "SELECT 1"}}
    }));
    assert!(
        query["error"].is_null(),
        "query must not be posture-blocked: {query}"
    );
    assert_eq!(
        query["result"]["isError"], true,
        "fixture connection should fail normally: {query}"
    );

    let listed = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    }));
    assert!(
        listed["error"].is_null(),
        "MCP must remain available: {listed}"
    );
    assert!(
        listed["result"]["tools"].is_array(),
        "tools/list must still work: {listed}"
    );

    let _ = mcp.finish();
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn mcp_manifest_protocol_cases_are_enforced() {
    let (mut mcp, tmp) = McpHarness::start();

    let first = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "ping"
    }));
    assert_error(&first, serde_json::json!(100), -32601);

    let duplicate = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 100,
        "method": "ping"
    }));
    assert_error(&duplicate, serde_json::json!(100), -32600);
    assert!(duplicate["error"]["message"] == "Duplicate request id");

    mcp.send_without_response("{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\"}");
    let crlf = mcp.send_raw("{\"jsonrpc\":\"2.0\",\"id\":101,\"method\":\"ping\"}\r");
    assert_error(&crlf, serde_json::json!(101), -32601);

    let stderr = mcp.finish();
    assert!(!stderr.contains("super-secret"));
    let _ = std::fs::remove_dir_all(tmp);
}

#[test]
fn mcp_exposes_static_read_only_prompt_and_resource_without_database_access() {
    let (mut mcp, tmp) = McpHarness::start();

    let initialize = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test", "version": "1"}}
    }));
    assert_eq!(
        initialize["result"]["capabilities"]["prompts"]["listChanged"],
        false
    );
    assert_eq!(
        initialize["result"]["capabilities"]["resources"]["subscribe"],
        false
    );

    let prompts =
        mcp.send(&serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "prompts/list"}));
    assert_eq!(
        prompts["result"]["prompts"][0]["name"],
        "read_only_database_debugging"
    );
    assert_eq!(
        prompts["result"]["prompts"][0]["arguments"],
        serde_json::json!([])
    );
    let prompt = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0", "id": 3, "method": "prompts/get",
        "params": {"name": "read_only_database_debugging"}
    }));
    assert!(prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .is_some_and(|text| text.contains("never grants write access")));

    let resources =
        mcp.send(&serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}));
    assert_eq!(
        resources["result"]["resources"][0]["uri"],
        "safeselect://guide/read-only-database-debugging"
    );
    let resource = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0", "id": 5, "method": "resources/read",
        "params": {"uri": "safeselect://guide/read-only-database-debugging"}
    }));
    assert!(resource["result"]["contents"][0]["text"]
        .as_str()
        .is_some_and(|text| {
            text.contains("does not replace least-privilege database users")
                && !text.contains("password")
        }));

    let database_info = mcp.send(&serde_json::json!({
        "jsonrpc": "2.0", "id": 6, "method": "tools/call",
        "params": {"name": "database_info", "arguments": {}}
    }));
    assert_eq!(
        database_info["result"]["structuredContent"]["untrusted_data"]["value"]
            ["resources_supported"],
        true
    );
    assert_eq!(
        database_info["result"]["structuredContent"]["untrusted_data"]["value"]
            ["database_resources_supported"],
        false
    );

    for (id, method, params) in [
        (7, "prompts/get", serde_json::json!({"name": "unknown"})),
        (
            8,
            "resources/read",
            serde_json::json!({"uri": "safeselect://unknown"}),
        ),
    ] {
        let response = mcp.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        assert_error(&response, serde_json::json!(id), -32602);
    }

    let _ = mcp.finish();
    let _ = std::fs::remove_dir_all(tmp);
}
