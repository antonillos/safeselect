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

    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut tools_response = String::new();
    reader
        .read_line(&mut tools_response)
        .expect("failed to read MCP tools response");
    let tools_rpc: serde_json::Value =
        serde_json::from_str(&tools_response).expect("tools/list should return JSON-RPC");
    let tools = tools_rpc["result"]["tools"]
        .as_array()
        .expect("tools/list should return tool definitions");
    assert!(
        tools_response.contains("describe_table")
            && tools_response.contains("list_functions")
            && tools_response.contains("list_triggers")
            && tools_response.contains("list_scheduled_jobs")
            && tools_response.contains("list_table_indexes")
            && tools_response.contains("get_database_stats")
            && tools_response.contains("get_table_stats")
            && !tools_response.contains("discover_document_schema")
            && tools.iter().all(|tool| {
                tool["outputSchema"]["required"]
                    .as_array()
                    .is_some_and(|required| {
                        required.contains(&serde_json::json!("next_suggestion"))
                    })
            }),
        "unexpected PostgreSQL tools response: {tools_response}"
    );

    for (id, relation, expected_columns) in [
        (3, "safe_table", &["id", "name", "payload"][..]),
        (4, "safe_view", &["id", "name"][..]),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": "describe_table",
                    "arguments": {
                        "schema": "public",
                        "table": relation
                    }
                }
            })
        )
        .unwrap();
        stdin.flush().unwrap();

        let mut describe_response = String::new();
        reader
            .read_line(&mut describe_response)
            .expect("failed to read describe_table response");
        let describe_rpc: serde_json::Value = serde_json::from_str(&describe_response)
            .expect("describe_table response should be valid JSON-RPC");
        let structured = &describe_rpc["result"]["structuredContent"];
        let description = &structured["untrusted_data"]["value"];
        let returned_columns = description["columns"]
            .as_array()
            .expect("describe_table should return columns");
        assert!(
            structured["next_suggestion"].is_string()
                && returned_columns
                    .iter()
                    .all(|column| column["ordinal_position"].is_number())
                && returned_columns
                    .iter()
                    .all(|column| column["udt_name"].is_string())
                && expected_columns.iter().all(|expected| returned_columns
                    .iter()
                    .any(|column| column["column_name"].as_str() == Some(expected))),
            "unexpected describe_table response for {relation}: {describe_response}"
        );
    }

    for (id, tool, expected_value) in [
        (7, "list_functions", "safe_trigger_function"),
        (8, "list_triggers", "safe_trigger"),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "name": tool,
                    "arguments": {"schema": "public"}
                }
            })
        )
        .unwrap();
        stdin.flush().unwrap();

        let mut response = String::new();
        reader
            .read_line(&mut response)
            .expect("failed to read PostgreSQL catalog discovery response");
        assert!(
            response.contains(expected_value)
                && response.contains("structuredContent")
                && response.contains("next_suggestion"),
            "unexpected {tool} response: {response}"
        );
    }

    for (id, tool, arguments, expected_message) in [
        (
            9,
            "list_functions",
            serde_json::json!({"schema": "pg_catalog"}),
            "does not support PostgreSQL system schemas",
        ),
        (
            10,
            "list_scheduled_jobs",
            serde_json::json!({"unexpected": true}),
            "Call it with an empty arguments object",
        ),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments}
            })
        )
        .unwrap();
        stdin.flush().unwrap();

        let mut response = String::new();
        reader
            .read_line(&mut response)
            .expect("failed to read invalid catalog discovery response");
        assert!(
            response.contains(expected_message) && response.contains("\"code\":-32602"),
            "unexpected invalid {tool} response: {response}"
        );
    }

    for (id, tool, arguments, expected_value, expected_next_step) in [
        (
            11,
            "list_table_indexes",
            serde_json::json!({"schema": "public", "table": "safe_table"}),
            "index_name",
            "explain",
        ),
        (
            12,
            "get_database_stats",
            serde_json::json!({}),
            "database_size",
            "list_tables",
        ),
        (
            13,
            "get_table_stats",
            serde_json::json!({"schema": "public", "table": "safe_table"}),
            "estimated_live_rows",
            "list_table_indexes",
        ),
    ] {
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments}
            })
        )
        .unwrap();
        stdin.flush().unwrap();

        let mut response = String::new();
        reader
            .read_line(&mut response)
            .expect("failed to read PostgreSQL index/statistics response");
        assert!(
            response.contains(expected_value)
                && response.contains("structuredContent")
                && response.contains("next_suggestion")
                && response.contains(expected_next_step),
            "unexpected {tool} response: {response}"
        );
    }

    // First query: intentional SQL error (table does not exist)
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
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
            && error_response.contains("does not exist")
            && error_response.contains("list_tables")
            && error_response.contains("safeselect-untrusted-data-")
            && error_response.contains("next_suggestion"),
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
            "id": 6,
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
        !ok_response.is_empty()
            && ok_response.contains("ok")
            && ok_response.contains("structuredContent")
            && ok_response.contains("next_suggestion"),
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
        (stderr.contains("RESULT_LIMIT_EXCEEDED") && stderr.contains("Result size limit exceeded"))
            || (stderr.contains("Limit exceeded") && stderr.contains("limit is 1000")),
        "limit error was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_timeout_control_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT pg_sleep(5)");
    assert!(!success, "pg_sleep unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("Query rejected")
            && (stderr.contains("function PG_SLEEP not allowed")
                || stderr.contains("Unqualified function 'pg_sleep' is not allowed")),
        "timeout control rejection was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn log_step(message: &str) {
    eprintln!("[smoke-real] {message}");
}

fn log_check(message: &str) {
    eprintln!("[check][smoke-real] {message}");
}
