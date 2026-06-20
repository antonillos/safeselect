//! Real PostgreSQL smoke tests for user-visible behavior.
//!
//! Covers happy path, SQL errors, security rejections, result limits, and
//! timeout-related controls using a real database. Gated separately because it
//! requires PostgreSQL and downloads/registers a JDBC driver in a temp config.

use super::postgres;

pub fn run() {
    if std::env::var("SAFESELECT_REAL_SMOKE_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_REAL_SMOKE_TEST=1 to run real smoke tests");
        return;
    }
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("smoke_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!(
        "safeselect-real-smoke-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    postgres::setup_database();
    postgres::write_config(&repo_root);
    postgres::download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        assert_check_ok(&repo_root, &config_dir);
        assert_select_ok(&repo_root, &config_dir);
        assert_sql_error_visible(&repo_root, &config_dir);
        assert_security_rejection_visible(&repo_root, &config_dir);
        assert_result_limit_visible(&repo_root, &config_dir);
        assert_timeout_control_visible(&repo_root, &config_dir);
    });

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
    assert!(success, "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("All checks passed"), "unexpected check output: {stdout}");
}

fn assert_select_ok(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT 1 AS ok");
    assert!(success, "SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stdout.contains("| 1"), "unexpected SELECT output: {stdout}");
    assert!(stdout.contains("rows"), "SELECT output should include row count: {stdout}");
}

fn assert_sql_error_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) = postgres::run_safeselect(
        repo_root,
        config_dir,
        "SELECT * FROM public.table_that_does_not_exist",
    );
    assert!(!success, "missing table query unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("ERROR: SQL query failed")
            && stderr.contains("SQL execution failed [SQL_ERROR]")
            && stderr.contains("does not exist"),
        "SQL error was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
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
        stderr.contains("RESULT_LIMIT_EXCEEDED") && stderr.contains("Result size limit exceeded"),
        "limit error was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_timeout_control_visible(repo_root: &std::path::Path, config_dir: &std::path::Path) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT pg_sleep(5)");
    assert!(!success, "pg_sleep unexpectedly succeeded: {stdout}");
    assert!(
        stderr.contains("Query rejected")
            && stderr.contains("function PG_SLEEP not allowed"),
        "timeout control rejection was not visible enough\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
