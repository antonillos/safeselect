use std::path::Path;
use std::process::{Command, Output};

fn configure(root: &Path, action: &str, password: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args(["config", action, "--environment", "dev", "--project"])
        .arg(root)
        .args(["--password", password])
        // A value must never leak even when available during configuration.
        .env("SAFESELECT_TEST_DB_REF", "synthetic-db-value")
        .env("SAFESELECT_TEST_SSH_REF", "synthetic-ssh-value")
        .output()
        .unwrap()
}

#[test]
fn cli_password_references_persist_names_not_values_and_reject_bad_inputs() {
    let root = std::env::temp_dir().join(format!("safeselect-cli-ref-{}", uuid::Uuid::new_v4()));
    let environments = root.join(".safeselect/environments");
    std::fs::create_dir_all(&environments).unwrap();
    std::fs::write(root.join(".safeselect/project.toml"), "version = 1\n").unwrap();
    let file = environments.join("dev.toml");
    std::fs::write(&file, "version = 1\n[database]\nkind = 'document'\nurl = 'mongodb://demo@localhost/demo'\nusername = 'demo'\n[ssh]\nenabled = true\nidentity_file = '/tmp/old-demo-key'\n").unwrap();

    for (action, variable) in [
        ("set-password", "SAFESELECT_TEST_DB_REF"),
        ("set-ssh-password", "SAFESELECT_TEST_SSH_REF"),
    ] {
        let output = configure(&root, action, &format!("{{env:{variable}}}"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let saved = std::fs::read_to_string(&file).unwrap();
        let config: toml::Value = toml::from_str(&saved).unwrap();
        if action == "set-password" {
            assert_eq!(config["database"]["secret"]["source"].as_str(), Some("env"));
            assert_eq!(
                config["database"]["secret"]["variable"].as_str(),
                Some(variable)
            );
        } else {
            assert_eq!(config["ssh"]["secret_variable"].as_str(), Some(variable));
            assert_eq!(config["ssh"]["auth_type"].as_str(), Some("PASSWORD"));
            assert!(config["ssh"].get("identity_file").is_none());
        }
        for text in [
            &saved,
            &String::from_utf8_lossy(&output.stdout).into_owned(),
            &String::from_utf8_lossy(&output.stderr).into_owned(),
        ] {
            assert!(!text.contains("synthetic-db-value"));
            assert!(!text.contains("synthetic-ssh-value"));
        }
        for input in [
            "{env:BAD;private-value}",
            "{env:NAME",
            "{file:/private-value}",
            "",
        ] {
            let output = configure(&root, action, input);
            assert!(!output.status.success());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-value"));
            assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let output = configure(&root, action, "synthetic-literal-value");
            assert!(!output.status.success());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-literal-value"));
            assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args([
            "config",
            "rename-environment",
            "--old",
            "dev",
            "--new",
            "renamed",
            "--project",
        ])
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: toml::Value =
        toml::from_str(&std::fs::read_to_string(environments.join("renamed.toml")).unwrap())
            .unwrap();
    assert_eq!(
        saved["database"]["secret"]["variable"].as_str(),
        Some("SAFESELECT_TEST_DB_REF")
    );
    assert_eq!(
        saved["ssh"]["secret_variable"].as_str(),
        Some("SAFESELECT_TEST_SSH_REF")
    );
    std::fs::remove_dir_all(root).unwrap();
}
