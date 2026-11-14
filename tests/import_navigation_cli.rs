//! Include real terminal interactions in the Rust coverage-producing suite.
#[cfg(unix)]
#[test]
fn interactive_import_navigation() {
    let output = std::process::Command::new("python3")
        .args([
            "-c",
            include_str!("import_navigation_pty.py"),
            env!("CARGO_BIN_EXE_safeselect"),
        ])
        .output()
        .expect("Python 3 is required for the pseudoterminal import test");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
