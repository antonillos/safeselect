//! Real PostgreSQL security regression suite.

use crate::postgres;
use std::path::Path;

pub fn run() {
    if std::env::var("SAFESELECT_SECURITY_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_SECURITY_TEST=1 to run real PostgreSQL security tests");
        return;
    }
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("security_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!("safeselect-security-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    log_step("starting real PostgreSQL security suite");
    log_step(&format!("workspace: {}", tmp.display()));
    log_step("setting up PostgreSQL fixtures");
    postgres::setup_database();
    log_step("writing SafeSelect test config");
    postgres::write_config(&repo_root);
    log_step("downloading PostgreSQL JDBC driver");
    postgres::download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        let baseline = database_state();
        log_step(&format!("captured baseline database state: {:?}", baseline));

        log_check("SELECT happy path");
        let (stdout, stderr, success) =
            postgres::run_safeselect(&repo_root, &config_dir, "SELECT 1 AS ok");
        assert!(
            success,
            "SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(stdout.contains("| 1"), "unexpected SELECT output: {stdout}");

        log_check("EXPLAIN SELECT happy path");
        let (stdout, stderr, success) = postgres::run_safeselect(
            &repo_root,
            &config_dir,
            "EXPLAIN SELECT id FROM public.safe_table WHERE id = 1",
        );
        assert!(
            success,
            "EXPLAIN SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("Seq Scan")
                || stdout.contains("Index Scan")
                || stdout.contains("QUERY PLAN"),
            "unexpected EXPLAIN output: {stdout}"
        );

        log_check("WITH ORDINALITY happy path");
        let (stdout, stderr, success) = postgres::run_safeselect(
            &repo_root,
            &config_dir,
            "SELECT * FROM unnest(ARRAY[10, 20]) WITH ORDINALITY AS t(value, ord)",
        );
        assert!(
            success,
            "WITH ORDINALITY SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("| 10    | 1") || stdout.contains("| 10 | 1   |"),
            "unexpected WITH ORDINALITY output: {stdout}"
        );

        log_check("WITH ... SELECT happy path");
        let (stdout, stderr, success) = postgres::run_safeselect(
                &repo_root,
                &config_dir,
                "WITH filtered AS (SELECT id, name FROM public.safe_table WHERE id <= 2) SELECT * FROM filtered ORDER BY id",
            );
        assert!(
            success,
            "WITH SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("alpha") && stdout.contains("beta"),
            "unexpected WITH SELECT output: {stdout}"
        );

        for (name, sql) in [
            ("DELETE", "DELETE FROM public.safe_table WHERE id = 1"),
            (
                "UPDATE",
                "UPDATE public.safe_table SET name = 'changed' WHERE id = 1",
            ),
            (
                "INSERT",
                "INSERT INTO public.safe_table VALUES (4, 'delta', 'd')",
            ),
            ("DROP", "DROP TABLE public.safe_table"),
            (
                "WITH delete",
                "WITH deleted AS (DELETE FROM public.safe_table RETURNING *) SELECT * FROM deleted",
            ),
            (
                "WITH final delete",
                "WITH x AS (SELECT 1) DELETE FROM public.safe_table",
            ),
            (
                "EXPLAIN DELETE",
                "EXPLAIN DELETE FROM public.safe_table WHERE id = 1",
            ),
            (
                "EXPLAIN ANALYZE SELECT",
                "EXPLAIN ANALYZE SELECT * FROM public.safe_table",
            ),
            (
                "EXPLAIN ANALYZE DELETE",
                "EXPLAIN ANALYZE DELETE FROM public.safe_table WHERE id = 1",
            ),
            (
                "CREATE TABLE AS",
                "CREATE TABLE public.evil_copy AS SELECT * FROM public.safe_table",
            ),
            (
                "CREATE TEMP TABLE AS",
                "CREATE TEMP TABLE tmp_evil AS SELECT * FROM public.safe_table",
            ),
            (
                "COPY TO PROGRAM",
                "COPY (SELECT name FROM public.safe_table) TO PROGRAM 'cat'",
            ),
            (
                "DO block",
                "DO $$ BEGIN DELETE FROM public.safe_table; END $$",
            ),
            ("LOCK TABLE", "LOCK TABLE public.safe_table IN ACCESS EXCLUSIVE MODE"),
            (
                "MERGE",
                "MERGE INTO public.safe_table AS t USING (SELECT 1 AS id) AS s ON t.id = s.id WHEN MATCHED THEN UPDATE SET name = 'oops'",
            ),
            (
                "CALL",
                "CALL pg_catalog.set_config('role', 'postgres', false)",
            ),
            (
                "PREPARE",
                "PREPARE evil AS DELETE FROM public.safe_table WHERE id = 1",
            ),
            (
                "DECLARE cursor",
                "DECLARE evil CURSOR FOR SELECT * FROM public.safe_table",
            ),
            ("transaction begin", "BEGIN"),
            ("transaction commit", "COMMIT"),
            ("transaction rollback", "ROLLBACK"),
            ("denied relation", "SELECT * FROM public.secret_table"),
            (
                "session change",
                "SELECT set_config('role', 'postgres', false)",
            ),
            ("sleep", "SELECT pg_sleep(5)"),
        ] {
            assert_rejected(&repo_root, &config_dir, name, sql, &baseline);
        }

        assert_rejected(
            &repo_root,
            &config_dir,
            "row limit",
            "SELECT id FROM public.safe_table ORDER BY id",
            &baseline,
        );
        assert_rejected(
            &repo_root,
            &config_dir,
            "byte limit",
            "SELECT payload FROM public.large_payload WHERE id = 1",
            &baseline,
        );

        log_check("baseline remained unchanged after all rejections");
        assert_eq!(database_state(), baseline);
    });

    log_step("cleaning up PostgreSQL fixtures");
    postgres::cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn assert_rejected(
    root: &Path,
    config_dir: &Path,
    name: &str,
    sql: &str,
    baseline: &DatabaseState,
) {
    log_check(&format!("expect rejection: {name}"));
    log_step(&format!("sql={sql}"));
    let (stdout, stderr, success) = postgres::run_safeselect(root, config_dir, sql);
    assert!(
        !success,
        "{name} unexpectedly succeeded\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Query rejected")
            || stderr.contains("Read-only mode")
            || stderr.contains("RESULT_LIMIT_EXCEEDED")
            || stderr.contains("Limit exceeded"),
        "{name} failed for the wrong reason\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        &database_state(),
        baseline,
        "{name} changed database state despite rejection"
    );
    log_step(&format!("confirmed rejection without mutation: {name}"));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseState {
    safe_table_summary: String,
    safe_table_rows: String,
    secret_table_rows: String,
    temp_evil_exists: String,
    evil_copy_exists: String,
}

fn database_state() -> DatabaseState {
    DatabaseState {
        safe_table_summary: postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) || ':' || string_agg(name, ',' ORDER BY id) FROM public.safe_table;",
        ),
        safe_table_rows: postgres::psql(
            &postgres::test_db(),
            "SELECT string_agg(id || ':' || octet_length(payload), ',' ORDER BY id) FROM public.safe_table;",
        ),
        secret_table_rows: postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) FROM public.secret_table;",
        ),
        temp_evil_exists: postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) FROM pg_tables WHERE schemaname LIKE 'pg_temp%' AND tablename = 'tmp_evil';",
        ),
        evil_copy_exists: postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tablename = 'evil_copy';",
        ),
    }
}

fn log_step(message: &str) {
    eprintln!("[security-real] {message}");
}

fn log_check(message: &str) {
    eprintln!("[check][security-real] {message}");
}
