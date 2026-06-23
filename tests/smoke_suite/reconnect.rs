//! Real Docker-backed reconnect regression test.
//!
//! This test intentionally restarts the Docker container named by
//! `SAFESELECT_SECURITY_DOCKER_CONTAINER`, so it is gated separately from the
//! normal real PostgreSQL security suite.

use super::postgres;
use std::path::Path;
use std::process::Command;

pub fn run() {
    if std::env::var("SAFESELECT_RECONNECT_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_RECONNECT_TEST=1 to restart Docker and test reconnect");
        return;
    }

    let container = match std::env::var("SAFESELECT_SECURITY_DOCKER_CONTAINER") {
        Ok(container) => container,
        Err(_) => {
            eprintln!(
                "Skipping: set SAFESELECT_SECURITY_DOCKER_CONTAINER=<container> for reconnect test"
            );
            return;
        }
    };
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("reconnect_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!("safeselect-reconnect-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    postgres::setup_database();
    postgres::write_config(&repo_root);
    postgres::download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        assert_select_ok(&repo_root, &config_dir, "before restart");

        docker(&["restart", &container]);
        postgres::wait_for_postgres();

        assert_select_ok(&repo_root, &config_dir, "after restart");
    });

    postgres::cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn assert_select_ok(repo_root: &Path, config_dir: &Path, phase: &str) {
    let (stdout, stderr, success) =
        postgres::run_safeselect(repo_root, config_dir, "SELECT 1 AS ok");
    assert!(
        success,
        "SELECT failed {phase}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("| 1"),
        "unexpected output {phase}: {stdout}"
    );
}

fn docker(args: &[&str]) {
    let output = Command::new("docker")
        .args(args)
        .output()
        .expect("failed to run docker");
    assert!(
        output.status.success(),
        "docker {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
