/// Integration tests against a real PostgreSQL database.
///
/// Requires:
///   - PostgreSQL on localhost:25432 (user=postgres, pass=testpass, db=testdb)
///   - SAFESELECT_INTEGRATION_TEST=1
///   - PostgreSQL JDBC driver registered
///
use std::path::PathBuf;
use std::process::Command;

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

fn run_with_config(args: &[&str], config_dir: &str) -> (String, String, bool) {
    let output = Command::new(safeselect_bin())
        .args(args)
        .env("SAFESELECT_CONFIG_DIR", config_dir)
        .env("SAFESELECT_INT_TEST_PASSWORD", "testpass")
        .env("SAFESELECT_PASSWORD_DB", "testpass")
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run safeselect");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let success = output.status.success();

    (strip_ansi(&stdout), strip_ansi(&stderr), success)
}

fn cleanup_keychain_account(account: &str) {
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", account, "-s", "safeselect"])
        .output();
}

fn setup_test_config() -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("safeselect-int-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    // Create .safeselect/ local project structure
    let repo_root = tmp.join("inttest-repo");
    let safeselect_dir = repo_root.join(".safeselect");
    let env_dir = safeselect_dir.join("environments");
    let drivers_dir = tmp.join("drivers");
    std::fs::create_dir_all(&env_dir).unwrap();
    std::fs::create_dir_all(&drivers_dir).unwrap();

    // Create project.toml
    std::fs::write(
        safeselect_dir.join("project.toml"),
        r#"
version = 1
display_name = "Integration Test"

[security]
read_only = true
allowed_schemas = ["public"]
denied_relations = []

[limits]
statement_timeout_ms = 5000
max_rows = 200
max_result_bytes = 1_000_000

[audit]
enabled = false
"#,
    )
    .unwrap();

    // Create environment
    std::fs::write(
        env_dir.join("testing.toml"),
        r#"
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://localhost:25432/testdb"
username = "postgres"

[database.secret]
source = "env"
variable = "SAFESELECT_INT_TEST_PASSWORD"
"#,
    )
    .unwrap();

    // Driver needs to be pre-registered — download it
    let dl = Command::new(safeselect_bin())
        .args(["driver", "download", "--vendor", "postgresql"])
        .env("SAFESELECT_CONFIG_DIR", &tmp)
        .output()
        .expect("driver download failed");
    assert!(
        dl.status.success(),
        "driver download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );

    repo_root
}

#[test]
fn test_integration_check() {
    if std::env::var("SAFESELECT_INTEGRATION_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_INTEGRATION_TEST=1 to run");
        return;
    }

    let repo_root = setup_test_config();
    let (stdout, stderr, success) = run_with_config(
        &[
            "check",
            "--project",
            repo_root.to_str().unwrap(),
            "--environment",
            "testing",
        ],
        repo_root.parent().unwrap().to_str().unwrap(),
    );

    if !success {
        eprintln!("stdout: {stdout}");
        eprintln!("stderr: {stderr}");
    }

    assert!(
        success,
        "check failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("All checks passed"),
        "expected success message, got: {stdout}"
    );

    let _ = std::fs::remove_dir_all(repo_root.parent().unwrap());
}

#[test]
fn test_integration_import_compose_non_interactive() {
    if std::env::var("SAFESELECT_INTEGRATION_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_INTEGRATION_TEST=1 to run");
        return;
    }

    let tmp = std::env::temp_dir().join(format!(
        "safeselect-import-compose-int-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let repo_root = tmp.join("compose-int-repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::write(
        repo_root.join(".env"),
        "DB_USER=postgres\nDB_PASSWORD=testpass\n",
    )
    .unwrap();
    std::fs::write(
        repo_root.join("compose.yaml"),
        r#"
services:
  db:
    image: postgres:17
    environment:
      POSTGRES_DB: testdb
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASSWORD}
    ports:
      - target: 5432
        published: 25432
"#,
    )
    .unwrap();

    let account = "compose-int-repo/db";
    cleanup_keychain_account(account);

    let dl = Command::new(safeselect_bin())
        .args(["driver", "download", "--vendor", "postgresql"])
        .env("SAFESELECT_CONFIG_DIR", &tmp)
        .env("NO_COLOR", "1")
        .output()
        .expect("driver download failed");
    assert!(
        dl.status.success(),
        "driver download failed: {}",
        String::from_utf8_lossy(&dl.stderr)
    );

    let (stdout, stderr, success) = run_with_config(
        &[
            "import-compose",
            "--path",
            repo_root.to_str().unwrap(),
            "--non-interactive",
        ],
        tmp.to_str().unwrap(),
    );

    if !success {
        eprintln!("stdout: {stdout}");
        eprintln!("stderr: {stderr}");
    }

    assert!(
        success,
        "import-compose failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Import Complete"));
    assert!(stdout.contains("Imported 1 connection(s): db"));
    assert!(stdout.contains("Passwords were imported or are already configured."));
    assert!(stdout.contains("safeselect check --environment db"));
    assert!(stdout.contains("safeselect agent install opencode --environment db"));
    assert!(stdout.contains("All checks passed"));

    let env_toml = std::fs::read_to_string(repo_root.join(".safeselect/environments/db.toml"))
        .expect("expected imported environment config");
    assert!(env_toml.contains("jdbc:postgresql://localhost:25432/testdb"));
    assert!(env_toml.contains("username = \"postgres\""));

    cleanup_keychain_account(account);
    let _ = std::fs::remove_dir_all(&tmp);
}
