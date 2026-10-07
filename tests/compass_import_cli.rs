//! Exercise the real non-interactive import without contacting databases or Keychain.
use std::process::Command;

#[test]
fn compass_reimport_is_idempotent_and_never_persists_exported_passwords() {
    let root = std::env::temp_dir().join(format!("compass-cli-{}", uuid::Uuid::new_v4()));
    let project = root.join("my-app");
    std::fs::create_dir_all(&project).unwrap();
    let export = root.join("connections.json");
    std::fs::write(&export, r#"{"connections":[{"name":"staging","connectionOptions":{"connectionString":"mongodb://demo:synthetic-db-password@db.example:27017/app","sshTunnel":{"host":"bastion.example","port":22,"username":"demo","authenticationMethod":"password","password":"synthetic-ssh-password"}}}]}"#).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_safeselect"))
            .args(["import-compass", "--non-interactive", "--path"])
            .arg(&export)
            .current_dir(&project)
            .env("SAFESELECT_CONFIG_DIR", root.join("global"))
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let env = project.join(".safeselect/environments/staging.toml");
    let original = std::fs::read_to_string(&env).unwrap();
    assert!(original.contains("MY_APP_STAGING_DB_PASSWORD"));
    let project_file = project.join(".safeselect/project.toml");
    let bastions = std::fs::read_to_string(&project_file).unwrap();
    assert!(bastions.contains("MY_APP_STAGING_SSH_PASSWORD"));
    let second = run();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("Skipping existing"));
    assert_eq!(std::fs::read_to_string(&env).unwrap(), original);
    assert_eq!(std::fs::read_to_string(project_file).unwrap(), bastions);
    assert_eq!(
        std::fs::read_dir(project.join(".safeselect/environments"))
            .unwrap()
            .count(),
        1
    );
    for output in [&first.stdout, &first.stderr, &second.stdout, &second.stderr] {
        let text = String::from_utf8_lossy(output);
        assert!(!text.contains("synthetic-db-password"));
        assert!(!text.contains("synthetic-ssh-password"));
    }
    for filename in [
        "environments/staging.toml",
        "project.toml",
        "compass-imports.toml",
    ] {
        let text = std::fs::read_to_string(project.join(".safeselect").join(filename)).unwrap();
        assert!(!text.contains("synthetic-db-password"));
        assert!(!text.contains("synthetic-ssh-password"));
    }
    std::fs::remove_dir_all(root).unwrap();
}
