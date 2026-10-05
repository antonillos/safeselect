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
        std::fs::write(&java, FAKE_JAVA).unwrap();
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_safeselect"))
            .args(["serve", "--environment", "testing"])
            .current_dir(project)
            .env("JAVA_HOME", root.join("jdk"))
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
