//! Real PostgreSQL smoke tests for user-visible behavior.
//!
//! Covers happy path, SQL errors, security rejections, result limits, and
//! timeout-related controls using a real database. Gated separately because it
//! requires PostgreSQL and downloads/registers a JDBC driver in a temp config.

use super::postgres;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

pub fn run() {
    if std::env::var("SAFESELECT_REAL_SMOKE_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_REAL_SMOKE_TEST=1 to run real smoke tests");
        return;
    }
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("smoke_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!("safeselect-real-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    log_step("starting real PostgreSQL smoke suite");
    log_step(&format!("workspace: {}", tmp.display()));
    log_step("setting up PostgreSQL fixtures");
    postgres::setup_database();
    log_step("writing SafeSelect test config");
    postgres::write_config(&repo_root);
    log_step("downloading PostgreSQL JDBC driver");
    postgres::download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        log_check("`safeselect check` happy path");
        assert_check_ok(&repo_root, &config_dir);
        log_check("SELECT happy path");
        assert_select_ok(&repo_root, &config_dir);
        log_check("user-visible SQL error reporting");
        assert_sql_error_visible(&repo_root, &config_dir);
        log_check("MCP server survives SQL errors");
        assert_mcp_sql_error_stays_alive(&repo_root, &config_dir);
        log_check("security rejection visibility");
        assert_security_rejection_visible(&repo_root, &config_dir);
        log_check("result limit visibility");
        assert_result_limit_visible(&repo_root, &config_dir);
        log_check("timeout-control rejection visibility");
        assert_timeout_control_visible(&repo_root, &config_dir);
    });

    log_step("cleaning up PostgreSQL fixtures");
    postgres::cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn assert_check_ok(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) = postgres::run_safeselect_args(
        repo_root,
        config_dir,
        &["check", "--environment", "testing"],
    );
    assert!(
        success,
        "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("All checks passed"),
        "unexpected check output: {stdout}"
    );
}

fn assert_select_ok(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT 1 AS ok");
    assert!(
        success,
        "SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("| 1"), "unexpected SELECT output: {stdout}");
    assert!(
        stdout.contains("rows"),
        "SELECT output should include row count: {stdout}"
    );
}

fn assert_sql_error_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) = postgres::run_safeselect(
        repo_root,
        config_dir,
        "SELECT * FROM public.table_that_does_not_exist",
    );
    assert!(
        !success,
        "missing table query unexpectedly succeeded: {stdout}"
    );
    assert!(
        stderr.contains("ERROR: SQL query failed")
            && stderr.contains("SQL execution failed [SQL_ERROR]")
            && stderr.contains("does not exist"),
        "SQL error was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_mcp_sql_error_stays_alive(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let project_config = repo_root.join(".safeselect/project.toml");
    let config = std::fs::read_to_string(&project_config).unwrap();
    std::fs::write(
        &project_config,
        config.replace("enabled = false", "enabled = true"),
    )
    .unwrap();

    let mut child = Command::new(postgres::safeselect_bin())
        .args([
            "serve",
            "--project",
            repo_root.to_str().unwrap(),
            "--environment",
            "testing",
        ])
        .env("SAFESELECT_CONFIG_DIR", config_dir)
        .env("SAFESELECT_SECURITY_TEST_PASSWORD", postgres::TEST_PASSWORD)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start MCP server");
    log_step(&format!("spawned MCP server pid={}", child.id()));

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "safeselect-smoke-test"
                }
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut initialize_response = String::new();
    reader
        .read_line(&mut initialize_response)
        .expect("failed to read MCP initialize response");
    if initialize_response.is_empty() {
        let mut stderr = String::new();
        if let Some(mut stream) = child.stderr.take() {
            let _ = stream.read_to_string(&mut stderr);
        }
        panic!("MCP server exited without initialize response, stderr: {stderr}");
    }
    assert!(
        initialize_response.contains("safeselect"),
        "unexpected MCP initialize response: {initialize_response}"
    );

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    // First query: intentional SQL error (table does not exist)
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "select",
                "arguments": {
                    "sql": "SELECT * FROM public.table_that_does_not_exist"
                }
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut error_response = String::new();
    reader
        .read_line(&mut error_response)
        .expect("failed to read MCP error response");

    if error_response.is_empty() {
        let mut stderr = String::new();
        if let Some(mut stream) = child.stderr.take() {
            let _ = stream.read_to_string(&mut stderr);
        }
        panic!("MCP server exited on SQL error instead of staying alive, stderr: {stderr}");
    }

    assert!(
        error_response.contains("Query execution failed")
            && error_response.contains("SQL execution failed [SQL_ERROR]")
            && error_response.contains("does not exist"),
        "SQL error was not visible in MCP response: {error_response}"
    );

    // Server must still be alive after a SQL error
    assert!(
        child.try_wait().unwrap().is_none(),
        "MCP server exited after a SQL error — it must stay alive for user SQL mistakes"
    );

    // Second query: valid SELECT to confirm the server is still serving
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "select",
                "arguments": {
                    "sql": "SELECT 1 AS ok"
                }
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut ok_response = String::new();
    reader
        .read_line(&mut ok_response)
        .expect("failed to read follow-up MCP response");

    assert!(
        !ok_response.is_empty() && ok_response.contains("ok"),
        "MCP server did not respond to follow-up query after SQL error: {ok_response}"
    );

    log_step("stopping MCP server after successful follow-up query");
    let _ = child.kill();
    let _ = child.wait();
}

fn assert_security_rejection_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "DELETE FROM public.safe_table");
    assert!(!success, "DELETE unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("Query rejected") && stderr.contains("Read-only mode"),
        "security rejection was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_result_limit_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) = postgres::run_safeselect(
        repo_root,
        config_dir,
        "SELECT payload FROM public.large_payload WHERE id = 1",
    );
    assert!(!success, "large result unexpectedly succeeded: {stdout}");
    assert!(
        (stderr.contains("RESULT_LIMIT_EXCEEDED")
            && stderr.contains("Result size limit exceeded"))
            || (stderr.contains("Limit exceeded") && stderr.contains("limit is 1000")),
        "limit error was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_timeout_control_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT pg_sleep(5)");
    assert!(!success, "pg_sleep unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("Query rejected") && stderr.contains("function PG_SLEEP not allowed"),
        "timeout control rejection was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn log_step(message: &str) {
    eprintln!("[smoke-real] {message}");
}

fn log_check(message: &str) {
    eprintln!("[check][smoke-real] {message}");
}
