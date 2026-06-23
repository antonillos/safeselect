use std::path::PathBuf;
use std::process::Command;

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
    let output = Command::new(safeselect_bin())
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run safeselect");

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
    assert!(stdout.contains("MCP SQL Fail-Closed"));
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
    let (stdout, _stderr, success) = &run(&["config", "validate"]);
    assert!(success);
    assert!(stdout.contains("No .safeselect/"));
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
    assert!(formula.contains("openjdk@17"));
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
