//! Real PostgreSQL security integration test support.
//!
//! Requires:
//! - `SAFESELECT_SECURITY_TEST=1`
//! - PostgreSQL reachable at `SAFESELECT_SECURITY_HOST`/`SAFESELECT_SECURITY_PORT`
//! - Admin credentials able to create/drop a test database and role
//!
//! Defaults match the local Docker environment used during development:
//! `host=localhost`, `port=5432`, `admin_user=postgres`, `password=testpass`.

use std::path::{Path, PathBuf};
use std::process::Command;

const TEST_DB: &str = "safeselect_security_test";
const TEST_USER: &str = "safeselect_test_user";
const TEST_PASSWORD: &str = "testpass";

pub fn run() {
    if std::env::var("SAFESELECT_SECURITY_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_SECURITY_TEST=1 to run real PostgreSQL security tests");
        return;
    }

    let tmp = std::env::temp_dir().join(format!(
        "safeselect-security-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    setup_database();
    write_config(&repo_root);
    download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        let (stdout, stderr, success) = run_safeselect(&repo_root, &config_dir, "SELECT 1 AS ok");
        assert!(success, "SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
        assert!(stdout.contains("| 1"), "unexpected SELECT output: {stdout}");

        let (stdout, stderr, success) = run_safeselect(
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

        for (name, sql) in [
            ("DELETE", "DELETE FROM public.safe_table WHERE id = 1"),
            ("UPDATE", "UPDATE public.safe_table SET name = 'changed' WHERE id = 1"),
            ("INSERT", "INSERT INTO public.safe_table VALUES (4, 'delta', 'd')"),
            ("DROP", "DROP TABLE public.safe_table"),
            ("WITH select", "WITH x AS (SELECT 1) SELECT * FROM x"),
            (
                "WITH delete",
                "WITH deleted AS (DELETE FROM public.safe_table RETURNING *) SELECT * FROM deleted",
            ),
            ("EXPLAIN DELETE", "EXPLAIN DELETE FROM public.safe_table WHERE id = 1"),
            (
                "EXPLAIN ANALYZE SELECT",
                "EXPLAIN ANALYZE SELECT * FROM public.safe_table",
            ),
            (
                "EXPLAIN ANALYZE DELETE",
                "EXPLAIN ANALYZE DELETE FROM public.safe_table WHERE id = 1",
            ),
            ("denied relation", "SELECT * FROM public.secret_table"),
            (
                "session change",
                "SELECT set_config('role', 'postgres', false)",
            ),
            ("sleep", "SELECT pg_sleep(5)"),
        ] {
            assert_rejected(&repo_root, &config_dir, name, sql);
        }

        assert_rejected(
            &repo_root,
            &config_dir,
            "row limit",
            "SELECT id FROM public.safe_table ORDER BY id",
        );
        assert_rejected(
            &repo_root,
            &config_dir,
            "byte limit",
            "SELECT payload FROM public.large_payload WHERE id = 1",
        );

        let counts = psql(
            TEST_DB,
            "SELECT count(*) || ':' || string_agg(name, ',' ORDER BY id) FROM public.safe_table;",
        );
        assert_eq!(counts, "3:alpha,beta,gamma");
        let secret_count = psql(TEST_DB, "SELECT count(*) FROM public.secret_table;");
        assert_eq!(secret_count, "1");
    });

    cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn safeselect_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_safeselect"))
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

fn pg_host() -> String {
    std::env::var("SAFESELECT_SECURITY_HOST").unwrap_or_else(|_| "localhost".into())
}

fn pg_port() -> String {
    std::env::var("SAFESELECT_SECURITY_PORT").unwrap_or_else(|_| "5432".into())
}

fn pg_admin_user() -> String {
    std::env::var("SAFESELECT_SECURITY_ADMIN_USER").unwrap_or_else(|_| "postgres".into())
}

fn pg_admin_password() -> String {
    std::env::var("SAFESELECT_SECURITY_ADMIN_PASSWORD").unwrap_or_else(|_| TEST_PASSWORD.into())
}

fn psql(database: &str, sql: &str) -> String {
    let output = if let Ok(container) = std::env::var("SAFESELECT_SECURITY_DOCKER_CONTAINER") {
        Command::new("docker")
            .args([
                "exec",
                "-e",
                &format!("PGPASSWORD={}", pg_admin_password()),
                &container,
                "psql",
                "-U",
                &pg_admin_user(),
                "-d",
                database,
                "-v",
                "ON_ERROR_STOP=1",
                "-At",
                "-c",
                sql,
            ])
            .output()
            .expect("failed to run psql in Docker")
    } else {
        Command::new("psql")
            .args([
                "-h",
                &pg_host(),
                "-p",
                &pg_port(),
                "-U",
                &pg_admin_user(),
                "-d",
                database,
                "-v",
                "ON_ERROR_STOP=1",
                "-At",
                "-c",
                sql,
            ])
            .env("PGPASSWORD", pg_admin_password())
            .output()
            .expect("failed to run psql")
    };

    assert!(
        output.status.success(),
        "psql failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_database() {
    psql("postgres", &format!("DROP DATABASE IF EXISTS {TEST_DB};"));
    psql("postgres", &format!("DROP ROLE IF EXISTS {TEST_USER};"));
    psql(
        "postgres",
        &format!("CREATE ROLE {TEST_USER} LOGIN PASSWORD '{TEST_PASSWORD}';"),
    );
    psql("postgres", &format!("CREATE DATABASE {TEST_DB};"));
    psql(
        "postgres",
        &format!("GRANT ALL PRIVILEGES ON DATABASE {TEST_DB} TO {TEST_USER};"),
    );
    psql(
        TEST_DB,
        &format!(
            "CREATE TABLE public.safe_table (id int primary key, name text, payload text); \
             CREATE TABLE public.large_payload (id int primary key, payload text); \
             CREATE TABLE public.secret_table (id int primary key, secret text); \
             INSERT INTO public.safe_table VALUES (1, 'alpha', repeat('a', 20)), (2, 'beta', repeat('b', 20)), (3, 'gamma', repeat('c', 200)); \
             INSERT INTO public.large_payload VALUES (1, repeat('z', 2000)); \
             INSERT INTO public.secret_table VALUES (1, 'top-secret'); \
             GRANT USAGE ON SCHEMA public TO {TEST_USER}; \
             GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {TEST_USER};"
        ),
    );
}

fn cleanup_database() {
    psql("postgres", &format!("DROP DATABASE IF EXISTS {TEST_DB};"));
    psql("postgres", &format!("DROP ROLE IF EXISTS {TEST_USER};"));
}

fn write_config(root: &Path) {
    let safeselect_dir = root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir).unwrap();

    std::fs::write(
        safeselect_dir.join("project.toml"),
        r#"
version = 1
display_name = "SafeSelect Security Test"

[security]
allowed_schemas = ["public"]
denied_relations = ["public.secret_table"]
require_single_statement = true

[limits]
statement_timeout_ms = 1000
max_rows = 2
max_result_bytes = 1000

[audit]
enabled = false
"#,
    )
    .unwrap();

    std::fs::write(
        env_dir.join("testing.toml"),
        format!(
            r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://{}:{}/{TEST_DB}"
username = "{TEST_USER}"

[database.secret]
source = "env"
variable = "SAFESELECT_SECURITY_TEST_PASSWORD"
"#,
            pg_host(),
            pg_port()
        ),
    )
    .unwrap();
}

fn run_safeselect(root: &Path, config_dir: &Path, sql: &str) -> (String, String, bool) {
    let output = Command::new(safeselect_bin())
        .args([
            "query",
            "--project",
            root.to_str().unwrap(),
            "--environment",
            "testing",
            "--sql",
            sql,
        ])
        .env("SAFESELECT_CONFIG_DIR", config_dir)
        .env("SAFESELECT_SECURITY_TEST_PASSWORD", TEST_PASSWORD)
        .output()
        .expect("failed to run safeselect");

    (
        strip_ansi(&String::from_utf8_lossy(&output.stdout)),
        strip_ansi(&String::from_utf8_lossy(&output.stderr)),
        output.status.success(),
    )
}

fn download_driver(config_dir: &Path) {
    let output = Command::new(safeselect_bin())
        .args(["driver", "download", "--vendor", "postgresql"])
        .env("SAFESELECT_CONFIG_DIR", config_dir)
        .output()
        .expect("driver download failed");
    assert!(
        output.status.success(),
        "driver download failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_rejected(root: &Path, config_dir: &Path, name: &str, sql: &str) {
    let (stdout, stderr, success) = run_safeselect(root, config_dir, sql);
    assert!(
        !success,
        "{name} unexpectedly succeeded\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Query rejected")
            || stderr.contains("Read-only mode")
            || stderr.contains("RESULT_LIMIT_EXCEEDED"),
        "{name} failed for the wrong reason\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
