//! Real unattended imports must not prompt, contact databases, or store credentials.
use std::{io::Write, path::Path, process::Command};

fn export(path: &Path, json: &str) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    zip.start_file(
        "workspace/data-sources.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(json.as_bytes()).unwrap();
    zip.finish().unwrap();
}

fn run(root: &Path, archive: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args(["import-dbeaver", "--non-interactive"])
        .arg(archive)
        .current_dir(root)
        .env("SAFESELECT_CONFIG_DIR", root.join("global"))
        .output()
        .unwrap()
}

#[test]
fn reimport_preserves_files_and_never_persists_exported_passwords() {
    let root = std::env::temp_dir().join(format!("dbeaver-cli-{}", uuid::Uuid::new_v4()));
    let project = root.join("my-app");
    std::fs::create_dir_all(&project).unwrap();
    let archive = root.join("export.zip");
    export(
        &archive,
        r##"{"connections":[{"name":"staging","driver":"postgres","username":"demo","password":"synthetic-db-password","configuration":{"url":"jdbc:postgresql://db.example:5432/app?sslmode=require","handlers":{"ssh_tunnel":{"enabled":true,"properties":{"#host":"bastion.example","#port":22,"#user":"demo","#authType":"PASSWORD"}}}}}]}"##,
    );
    let first = run(&project, &archive);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let config = project.join(".safeselect");
    let env_path = config.join("environments/staging.toml");
    let original = std::fs::read_to_string(&env_path).unwrap();
    assert!(original.contains("MY_APP_STAGING_DB_PASSWORD"));
    assert!(original.contains("sslmode=require"));
    let bastions = std::fs::read_to_string(config.join("project.toml")).unwrap();
    assert!(bastions.contains("STAGING_SSH_PASSWORD"));
    assert!(!bastions.contains("MY_APP_STAGING_SSH_PASSWORD"));
    // Renamed environments are recognized by endpoint identity without an index.
    std::fs::rename(&env_path, config.join("environments/custom.toml")).unwrap();
    std::fs::remove_file(config.join("dbeaver-imports.toml")).unwrap();
    let second = run(&project, &archive);
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("Skipping existing"));
    assert_eq!(
        std::fs::read_to_string(config.join("environments/custom.toml")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(config.join("project.toml")).unwrap(),
        bastions
    );
    assert_eq!(
        std::fs::read_dir(config.join("environments"))
            .unwrap()
            .count(),
        1
    );
    for output in [&first.stdout, &first.stderr, &second.stdout, &second.stderr] {
        assert!(!String::from_utf8_lossy(output).contains("synthetic-db-password"));
    }
    for path in [
        config.join("environments/custom.toml"),
        config.join("project.toml"),
    ] {
        assert!(!std::fs::read_to_string(path)
            .unwrap()
            .contains("synthetic-db-password"));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn colliding_names_and_password_changes_do_not_overwrite() {
    let root = std::env::temp_dir().join(format!("dbeaver-collision-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let archive = root.join("export.zip");
    let sources = r#"{"connections":[{"name":"staging","driver":"postgres","host":"one.example","database":"app","username":"demo","password":"first-fixture"},{"name":"staging","driver":"postgres","host":"two.example","database":"app","username":"demo"}]}"#;
    export(&archive, sources);
    assert!(run(&root, &archive).status.success());
    let envs = root.join(".safeselect/environments");
    let first = std::fs::read_to_string(envs.join("staging.toml")).unwrap();
    let second = std::fs::read_to_string(envs.join("staging-2.toml")).unwrap();
    assert!(first.contains("one.example"));
    assert!(second.contains("two.example"));
    let index = std::fs::read_to_string(root.join(".safeselect/dbeaver-imports.toml")).unwrap();
    assert!(!index.contains("first-fixture"));
    export(
        &archive,
        &sources.replace("first-fixture", "second-fixture"),
    );
    assert!(run(&root, &archive).status.success());
    assert_eq!(std::fs::read_dir(&envs).unwrap().count(), 2);
    assert_eq!(
        std::fs::read_to_string(envs.join("staging.toml")).unwrap(),
        first
    );
    assert_eq!(
        std::fs::read_to_string(envs.join("staging-2.toml")).unwrap(),
        second
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_index_is_rejected_without_overwriting_environments() {
    let root = std::env::temp_dir().join(format!("dbeaver-index-{}", uuid::Uuid::new_v4()));
    let config = root.join(".safeselect");
    std::fs::create_dir_all(config.join("environments")).unwrap();
    let archive = root.join("export.zip");
    export(
        &archive,
        r#"{"connections":[{"name":"staging","host":"db.example","database":"app"}]}"#,
    );
    std::fs::write(
        config.join("dbeaver-imports.toml"),
        "[connections]\nid = ['../private']",
    )
    .unwrap();
    std::fs::write(config.join("environments/staging.toml"), "untouched").unwrap();
    let output = run(&root, &archive);
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read_to_string(config.join("environments/staging.toml")).unwrap(),
        "untouched"
    );
    std::fs::remove_dir_all(root).unwrap();
}
