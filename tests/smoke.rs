use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn safeselect_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_safeselect"))
}

fn safeselect_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn strip_ansi(s: &str) -> String {
    s.chars()
        .fold((String::new(), false), |(mut out, mut escape), c| {
            if escape {
                if c == 'm' {
                    escape = false;
                }
            } else if c == '\x1b' {
                escape = true;
            } else {
                out.push(c);
            }
            (out, escape)
        })
        .0
}

fn run(args: &[&str]) -> (String, String, bool) {
    run_in_dir(args, None)
}

fn run_in_dir(args: &[&str], current_dir: Option<&std::path::Path>) -> (String, String, bool) {
    let mut command = Command::new(safeselect_bin());
    command.args(args).env("NO_COLOR", "1");
    if let Some(dir) = current_dir {
        command.current_dir(dir);
    }
    let output = command.output().expect("failed to run safeselect");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    (strip_ansi(&stdout), strip_ansi(&stderr), success)
}

fn repo_file(path: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

#[test]
fn test_help() {
    let (stdout, _, success) = &run(&["--help"]);
    assert!(success);
    assert!(stdout.contains("Fail-closed read-only database access"));
    assert!(stdout.contains("serve"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("driver"));
    assert!(stdout.contains("agent"));
    assert!(stdout.contains("check"));
    assert!(stdout.contains("uninstall"));
}

#[test]
fn test_version() {
    let (stdout, _, success) = &run(&["--version"]);
    assert!(success);
    assert!(stdout.contains(safeselect_version()));
}

#[test]
fn test_config_validate_no_project() {
    let temp_dir = std::env::temp_dir().join(format!(
        "safeselect-smoke-no-project-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let (stdout, _stderr, success) = &run_in_dir(&["config", "validate"], Some(&temp_dir));
    assert!(success);
    assert!(stdout.contains("No .safeselect/ directory found"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_config_validate_missing_project() {
    let (_stdout, stderr, success) = &run(&[
        "config",
        "validate",
        "--project",
        "/nonexistent/safeselect/repo",
    ]);
    assert!(!success);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_driver_list_empty() {
    let (stdout, _stderr, _success) = &run(&["driver", "list"]);
    assert!(stdout.contains("postgresql") || stdout.contains("drivers"));
}

#[test]
fn test_agent_detect() {
    let (stdout, _stderr, success) = &run(&["agent", "detect"]);
    assert!(success);
    assert!(stdout.contains("Detected MCP clients"));
}

#[test]
fn test_unknown_command() {
    let (_stdout, stderr, success) = &run(&["this-command-does-not-exist"]);
    assert!(!success);
    assert!(stderr.contains("error") || stderr.contains("unrecognized"));
}

#[test]
fn test_serve_missing_project() {
    let (_stdout, stderr, success) = &run(&[
        "serve",
        "--project",
        "/nonexistent/safeselect/repo",
        "--environment",
        "testing",
    ]);
    assert!(!success);
    assert!(stderr.contains("does not exist") || stderr.contains("not found"));
}

#[test]
fn test_mcp_stacked_query_rejection_is_audited_before_fail_closed_exit() {
    let tmp = std::env::temp_dir().join(format!(
        "safeselect-fail-closed-audit-{}",
        std::process::id()
    ));
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
display_name = "Fail Closed Audit Test"

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
variable = "SAFESELECT_FAIL_CLOSED_TEST_PASSWORD"
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

    let forbidden_sql = "/* harmless prefix */\nCoMmIt ;\nDrOp TABLE public.audit_probe";
    let mut child = Command::new(safeselect_bin())
        .args([
            "serve",
            "--project",
            repo_root.to_str().unwrap(),
            "--environment",
            "testing",
        ])
        .env("SAFESELECT_CONFIG_DIR", &config_dir)
        .env("SAFESELECT_FAIL_CLOSED_TEST_PASSWORD", "unused")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start MCP server");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {"name": "fail-closed-audit-test"}
            }
        })
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "select",
                "arguments": {"sql": forbidden_sql}
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();
    drop(stdin);

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("MCP server did not fail closed after a security rejection");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(1));

    let mut stdout_text = String::new();
    stdout
        .read_to_string(&mut stdout_text)
        .expect("failed to read MCP stdout");
    let frames: Vec<serde_json::Value> = stdout_text
        .lines()
        .map(|line| serde_json::from_str(line).expect("stdout must contain only JSON-RPC frames"))
        .collect();
    let rejection = frames
        .iter()
        .find(|frame| frame.get("id") == Some(&serde_json::json!(2)))
        .expect("missing JSON-RPC rejection response");
    assert_eq!(rejection["error"]["code"], -32000);
    assert!(
        rejection["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Query rejected")),
        "unexpected rejection response: {rejection}"
    );
    assert!(
        rejection["error"]["data"]["next_suggestion"]
            .as_str()
            .is_some_and(|suggestion| !suggestion.is_empty()),
        "rejection must retain trusted next-step guidance: {rejection}"
    );

    let project_audit_dir = audit_dir.join("Fail Closed Audit Test").join("testing");
    let audit_files: Vec<_> = std::fs::read_dir(&project_audit_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(audit_files.len(), 1);

    let audit = std::fs::read_to_string(&audit_files[0]).unwrap();
    let entries: Vec<serde_json::Value> = audit
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "REJECT");
    assert_eq!(entries[0]["decision"], "reject");
    assert_eq!(
        entries[0]["query_hash"],
        hex::encode(Sha256::digest(forbidden_sql.as_bytes()))
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_setup_mode_documentation_matches_cli() {
    let readme = repo_file("README.md");
    assert!(
        readme.contains("enters setup mode automatically"),
        "README should document implicit setup mode"
    );
    assert!(
        !readme.contains("safeselect serve --setup"),
        "README must not document a non-existent serve --setup flag"
    );

    let (_stdout, stderr, success) = run(&["serve", "--help"]);
    assert!(success, "serve --help failed: {stderr}");
    assert!(
        !stderr.contains("--setup"),
        "CLI help unexpectedly exposes --setup"
    );
}

#[test]
fn test_homebrew_formula_tracks_current_release_shape() {
    let formula = repo_file("packaging/homebrew/safeselect.rb");
    assert!(formula.contains("version \"0.3.0\""));
    assert!(!formula.contains("depends_on \"openjdk@17\""));
    assert!(formula.contains("safeselect-v#{version}-aarch64-apple-darwin.tar.gz"));
    assert!(formula.contains("safeselect-v#{version}-x86_64-apple-darwin.tar.gz"));
    assert!(!formula.contains("v0.1.0"));
    assert!(!formula.contains("PLACEHOLDER_"));
}

#[test]
fn test_check_missing_project() {
    let (_stdout, stderr, success) = &run(&[
        "check",
        "--project",
        "/nonexistent/safeselect/repo",
        "--environment",
        "testing",
    ]);
    assert!(!success);
    assert!(stderr.contains("not found"));
}
