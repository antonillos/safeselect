//! Shared PostgreSQL smoke-test helpers.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const TEST_DB_PREFIX: &str = "safeselect_test";
const TEST_USER_PREFIX: &str = "safeselect_test_user";
pub const TEST_PASSWORD: &str = "testpass";

pub fn test_db() -> String {
    let suffix = std::env::var("SAFESELECT_TEST_SUFFIX")
        .unwrap_or_else(|_| std::process::id().to_string())
        .replace('-', "_");
    format!("{TEST_DB_PREFIX}_{suffix}")
}

pub fn test_user() -> String {
    let suffix = std::env::var("SAFESELECT_TEST_SUFFIX")
        .unwrap_or_else(|_| std::process::id().to_string())
        .replace('-', "_");
    format!("{TEST_USER_PREFIX}_{suffix}")
}

pub fn safeselect_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_safeselect"))
}

pub fn psql(database: &str, sql: &str) -> String {
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

pub fn setup_database() {
    wait_for_postgres();
    let test_db = test_db();
    let test_user = test_user();
    psql("postgres", &format!("DROP DATABASE IF EXISTS {test_db};"));
    psql("postgres", &format!("DROP ROLE IF EXISTS {test_user};"));
    psql(
        "postgres",
        &format!("CREATE ROLE {test_user} LOGIN PASSWORD '{TEST_PASSWORD}';"),
    );
    psql("postgres", &format!("CREATE DATABASE {test_db};"));
    psql(
        "postgres",
        &format!("GRANT ALL PRIVILEGES ON DATABASE {test_db} TO {test_user};"),
    );
    psql(
        &test_db,
        &format!(
            "CREATE TABLE public.safe_table (id int primary key, name text, payload text); \
             CREATE TABLE public.large_payload (id int primary key, payload text); \
             CREATE TABLE public.secret_table (id int primary key, secret text); \
             INSERT INTO public.safe_table VALUES (1, 'alpha', repeat('a', 20)), (2, 'beta', repeat('b', 20)), (3, 'gamma', repeat('c', 200)); \
             INSERT INTO public.large_payload VALUES (1, repeat('z', 2000)); \
             INSERT INTO public.secret_table VALUES (1, 'top-secret'); \
             GRANT USAGE ON SCHEMA public TO {test_user}; \
             GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {test_user};"
        ),
    );
}

pub fn wait_for_postgres() {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::panic::catch_unwind(|| psql("postgres", "SELECT 1;")).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("PostgreSQL did not become ready");
}

pub fn cleanup_database() {
    let test_db = test_db();
    let test_user = test_user();
    psql("postgres", &format!("DROP DATABASE IF EXISTS {test_db};"));
    psql("postgres", &format!("DROP ROLE IF EXISTS {test_user};"));
}

pub fn write_config(root: &Path) {
    let safeselect_dir = root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    std::fs::create_dir_all(&env_dir).unwrap();

    std::fs::write(
        safeselect_dir.join("project.toml"),
        r#"
version = 1
display_name = "SafeSelect Smoke Test"

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

    let test_db = test_db();
    let test_user = test_user();
    std::fs::write(
        env_dir.join("testing.toml"),
        format!(
            r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://{}:{}/{test_db}"
username = "{test_user}"

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

pub fn run_safeselect(root: &Path, config_dir: &Path, sql: &str) -> (String, String, bool) {
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

pub fn run_safeselect_args(
    root: &Path,
    config_dir: &Path,
    args: &[&str],
) -> (String, String, bool) {
    let output = Command::new(safeselect_bin())
        .args(args)
        .env("SAFESELECT_CONFIG_DIR", config_dir)
        .env("SAFESELECT_SECURITY_TEST_PASSWORD", TEST_PASSWORD)
        .env("NO_COLOR", "1")
        .current_dir(root)
        .output()
        .expect("failed to run safeselect");

    (
        strip_ansi(&String::from_utf8_lossy(&output.stdout)),
        strip_ansi(&String::from_utf8_lossy(&output.stderr)),
        output.status.success(),
    )
}

pub fn download_driver(config_dir: &Path) {
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
