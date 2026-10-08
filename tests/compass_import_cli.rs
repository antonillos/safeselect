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
    assert!(bastions.contains("STAGING_SSH_PASSWORD"));
    assert!(!bastions.contains("MY_APP_STAGING_SSH_PASSWORD"));
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

#[test]
fn distinct_same_named_connections_are_imported_without_overwriting() {
    let root = std::env::temp_dir().join(format!("compass-collision-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let export = root.join("connections.json");
    std::fs::write(&export, r#"{"connections":[{"name":"staging","connectionString":"mongodb://one.example/app"},{"name":"staging","connectionString":"mongodb://two.example/app"}]}"#).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_safeselect"))
            .args(["import-compass", "--non-interactive", "--path"])
            .arg(&export)
            .current_dir(&root)
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
    let dir = root.join(".safeselect/environments");
    let original = std::fs::read_to_string(dir.join("staging.toml")).unwrap();
    assert!(original.contains("one.example"));
    assert!(std::fs::read_to_string(dir.join("staging-2.toml"))
        .unwrap()
        .contains("two.example"));
    assert!(run().status.success());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    assert_eq!(
        std::fs::read_to_string(dir.join("staging.toml")).unwrap(),
        original
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_database_query_is_rejected_before_import_writes() {
    let root = std::env::temp_dir().join(format!("compass-validation-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let export = root.join("connections.json");
    std::fs::write(&export, r#"{"connections":[{"name":"valid","connectionString":"mongodb://db.example/app"},{"name":"invalid","connectionOptions":{"connectionString":"mongodb://db.example/app?proxyPassword=synthetic-query-value","sshTunnel":{"host":"bastion.example","username":"demo","authenticationMethod":"password","password":"synthetic-ssh-value"}}}]}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args(["import-compass", "--non-interactive", "--path"])
        .arg(&export)
        .current_dir(&root)
        .env("SAFESELECT_CONFIG_DIR", root.join("global"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.join(".safeselect").exists());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("unsupported credential-bearing query options"));
    assert!(!error.contains("synthetic-query-value"));
    assert!(!error.contains("synthetic-ssh-value"));
    std::fs::remove_dir_all(root).unwrap();
}
