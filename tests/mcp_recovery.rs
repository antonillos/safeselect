//! Exercise recovery through the real stdio MCP server, without a database or
//! credentials. The isolated Java executable simulates the sidecar protocol.
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const FAKE_JAVA: &str = r#"#!/usr/bin/env python3
import json
import pathlib
import sys

if '-version' in sys.argv:
    print('openjdk version "17.0.1"', file=sys.stderr)
    sys.exit(0)

counter = pathlib.Path(__file__).with_name('starts')
starts = int(counter.read_text()) + 1 if counter.exists() else 1
counter.write_text(str(starts))
sys.stdin.readline()  # password handshake (empty for this synthetic backend)
print('ready', flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    if method == 'verify_document_connection' and starts == 1:
        sys.stdout.write('{"id":' + str(request['id']) + ',"ok":')
        sys.stdout.flush()
        sys.stdin.readline()  # deliberately stall in a partial JSON-line
        break
    if method == 'ping':
        result = 'pong'
    elif method == 'disconnect':
        result = {'status': 'disconnected'}
    elif method == 'check_connection_health':
        result = 'connected'
    elif method == 'verify_document_connection':
        result = {'ok': 1.0}
    else:
        raise AssertionError('unexpected sidecar method')
    print(json.dumps({'id': request['id'], 'ok': result}), flush=True)
"#;

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    root: PathBuf,
}

impl Session {
    fn start() -> Self {
        Self::start_with(FAKE_JAVA, None)
    }

    fn start_with(java_script: &str, interval: Option<u64>) -> Self {
        let root =
            std::env::temp_dir().join(format!("safeselect-mcp-recovery-{}", uuid::Uuid::new_v4()));
        let project = root.join("project");
        let environments = project.join(".safeselect/environments");
        let bin = root.join("jdk/bin");
        std::fs::create_dir_all(&environments).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            project.join(".safeselect/project.toml"),
            format!("version = 1\n[limits]\nstatement_timeout_ms = 100\n[audit]\nenabled = true\ndirectory = \"{}\"\n", root.join("audit").display()),
        )
        .unwrap();
        std::fs::write(
            environments.join("testing.toml"),
            "version = 1\n[database]\nkind = \"document\"\nvendor = \"mongodb\"\nurl = \"mongodb://127.0.0.1:1/synthetic\"\n",
        ).unwrap();
        let java = bin.join("java");
        std::fs::write(&java, java_script).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        if let Some(interval) = interval {
            std::fs::write(
                root.join("config/config.toml"),
                format!("[mcp]\nhealth_check_interval_seconds = {interval}\n"),
            )
            .unwrap();
        }
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_safeselect"))
            .args(["serve", "--environment", "testing"])
            .current_dir(project)
            .env("JAVA_HOME", root.join("jdk"))
            .env("SAFESELECT_CONFIG_DIR", root.join("config"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            root,
        }
    }

    fn send(&mut self, id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
        let request =
            serde_json::json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params});
        writeln!(self.stdin, "{request}").unwrap();
        self.stdin.flush().unwrap();
        let mut fd = libc::pollfd {
            fd: self.stdout.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert!(
            unsafe { libc::poll(&mut fd, 1, 15_000) } > 0,
            "MCP response must have a bounded wait"
        );
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        if line.is_empty() {
            use std::io::Read;
            let mut error = String::new();
            self.child
                .stderr
                .as_mut()
                .unwrap()
                .read_to_string(&mut error)
                .unwrap();
            panic!("MCP server exited before replying: {error}");
        }
        let response: serde_json::Value =
            serde_json::from_str(&line).expect("MCP must return complete JSON-RPC");
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        response
    }

    fn tool(&mut self, id: u64, name: &str) -> serde_json::Value {
        self.send(
            id,
            "tools/call",
            serde_json::json!({"name":name, "arguments":{}}),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn stalled_check_can_reconnect_and_check_again_in_the_same_mcp_session() {
    let mut session = Session::start();
    let mcp_pid = session.child.id();
    let initialized = session.send(
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion":"2025-06-18", "clientInfo":{"name":"recovery-test", "version":"1"}
        }),
    );
    assert!(initialized.get("result").is_some(), "{initialized}");

    let start = Instant::now();
    let failed = session.tool(2, "check");
    assert!(
        failed.get("error").is_some() || failed["result"]["isError"] == true,
        "{failed}"
    );
    assert!(
        failed
            .to_string()
            .contains("SAFESELECT_BACKEND_VERIFICATION_FAILED"),
        "{failed}"
    );
    assert!(start.elapsed() < Duration::from_secs(12));

    // A failed backend check must not close or block the stdio MCP transport.
    let tools = session.send(3, "tools/list", serde_json::json!({}));
    assert!(tools["result"]["tools"].is_array(), "{tools}");
    let reconnected = session.tool(4, "reconnect");
    assert!(reconnected.get("error").is_none(), "{reconnected}");
    assert_ne!(reconnected["result"]["isError"], true, "{reconnected}");
    assert!(
        reconnected.to_string().contains("Reconnected and verified"),
        "{reconnected}"
    );
    let checked = session.tool(5, "check");
    assert!(
        checked.to_string().contains("SAFESELECT_ALL_CHECKS_PASSED"),
        "{checked}"
    );
    assert_eq!(session.child.id(), mcp_pid);
    assert!(session.child.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "2"
    );
}

fn database_status(session: &mut Session, id: u64) -> String {
    session.tool(id, "database_info")["result"]["structuredContent"]["untrusted_data"]["value"]
        ["database_connection_status"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn periodic_failure_preserves_busy_mcp_and_does_not_reconnect() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'connected' if not counter.with_name('checked').exists() else 'unavailable'\n        counter.with_name('checked').touch()");
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut id = 2;
    while database_status(&mut session, id) != "disconnected" {
        assert!(Instant::now() < deadline, "monitor was starved by requests");
        id += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(session.child.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "1"
    );
    assert!(
        session.send(id + 1, "tools/list", serde_json::json!({}))["result"]["tools"].is_array()
    );
}

#[test]
fn invalid_global_interval_fails_before_starting_sidecar() {
    let mut session = Session::start_with(FAKE_JAVA, Some(0));
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!session.child.wait().unwrap().success());
    assert!(!session.root.join("jdk/bin/starts").exists());
    use std::io::Read;
    let mut error = String::new();
    session
        .child
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert!(error.contains("health_check_interval_seconds"));
}

#[test]
fn idle_mcp_rechecks_without_reconnecting_and_reports_recovery() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'connected' if not counter.with_name('checked').exists() or counter.with_name('restored').exists() else 'unavailable'\n        counter.with_name('checked').touch()\n        with counter.with_name('health-checks').open('a') as checks:\n            checks.write('checked\\n')");
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    // Wait for two background checks without sending any MCP input.
    let checks = session.root.join("jdk/bin/health-checks");
    let deadline = Instant::now() + Duration::from_secs(6);
    while std::fs::read_to_string(&checks)
        .unwrap_or_default()
        .lines()
        .count()
        < 3
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(database_status(&mut session, 2), "disconnected");
    std::fs::write(session.root.join("jdk/bin/restored"), "").unwrap();
    let mut id = 3;
    while database_status(&mut session, id) != "connected" {
        assert!(Instant::now() < deadline);
        id += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "1"
    );
}

#[test]
fn startup_health_failure_keeps_metadata_available_and_reconnects_only_on_use() {
    let script = FAKE_JAVA.replace(
        "result = 'connected'",
        "result = 'unavailable' if starts == 1 else 'connected'",
    );
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(database_status(&mut session, 2), "disconnected");
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "1"
    );
    let checked = session.tool(3, "check");
    assert!(checked.get("result").is_some(), "{checked}");
    assert_eq!(database_status(&mut session, 4), "connected");
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "2"
    );
}

#[test]
fn unavailable_document_tool_returns_error_without_closing_mcp() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'unavailable'");
    let mut session = Session::start_with(&script, None);
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    let result = session.tool(2, "list_databases");
    assert!(result.get("error").is_some(), "{result}");
    assert!(session.send(3, "tools/list", serde_json::json!({}))["result"]["tools"].is_array());
    assert_eq!(database_status(&mut session, 4), "disconnected");
}

#[test]
fn disconnect_suspends_monitor_and_manual_reconnect_resumes_it() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'connected' if starts == 1 or not counter.with_name('reconnected').exists() else 'unavailable'\n        if starts > 1: counter.with_name('reconnected').touch()");
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    assert!(session.tool(2, "disconnect").get("result").is_some());
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(database_status(&mut session, 3), "disconnected");
    assert!(session.tool(4, "reconnect").get("result").is_some());
    assert_eq!(database_status(&mut session, 5), "connected");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut id = 6;
    while database_status(&mut session, id) != "disconnected" {
        assert!(Instant::now() < deadline, "monitor did not resume");
        id += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(session.child.try_wait().unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "2"
    );
}

#[test]
fn database_access_recovers_only_on_use_after_background_failure() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'connected' if starts > 1 or not counter.with_name('checked').exists() else 'unavailable'\n        counter.with_name('checked').touch()")
        .replace("    elif method == 'verify_document_connection':", "    elif method == 'list_databases':\n        result = ['synthetic']\n    elif method == 'verify_document_connection':");
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut id = 2;
    while database_status(&mut session, id) != "disconnected" {
        assert!(Instant::now() < deadline);
        id += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "1"
    );
    let result = session.tool(id + 1, "list_databases");
    assert!(result.get("result").is_some(), "{result}");
    assert_eq!(database_status(&mut session, id + 2), "connected");
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "2"
    );
}

#[test]
fn overdue_check_waits_for_active_database_operation_without_overlapping_ipc() {
    let script = FAKE_JAVA.replace("result = 'connected'", "result = 'connected' if not counter.with_name('checked').exists() else 'unavailable'\n        counter.with_name('checked').touch()")
        .replace("    elif method == 'verify_document_connection':", "    elif method == 'list_databases':\n        import time\n        time.sleep(1.2)\n        result = ['synthetic']\n    elif method == 'verify_document_connection':");
    let mut session = Session::start_with(&script, Some(1));
    assert!(session
        .send(1, "initialize", serde_json::json!({}))
        .get("result")
        .is_some());
    let result = session.tool(2, "list_databases");
    assert!(result.get("result").is_some(), "{result}");
    assert_eq!(database_status(&mut session, 3), "disconnected");
    assert_eq!(
        std::fs::read_to_string(session.root.join("jdk/bin/starts")).unwrap(),
        "1"
    );
}
