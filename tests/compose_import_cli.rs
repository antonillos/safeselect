//! Exercise unattended Compose output without a database, network, or Keychain.
use sha2::{Digest, Sha256};
use std::process::Command;

#[test]
fn unattended_compose_preserves_completion_and_deferred_verification_guidance() {
    let root = std::env::temp_dir().join(format!("compose-output-{}", uuid::Uuid::new_v4()));
    let project = root.join("project");
    let global = root.join("global");
    let drivers = global.join("drivers");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&drivers).unwrap();
    std::fs::write(
        project.join("compose.yaml"),
        r#"
services:
  db:
    image: postgres:17
    environment:
      POSTGRES_USER: reader
      POSTGRES_DB: app
      POSTGRES_PASSWORD: ${SAFESELECT_COMPOSE_OFFLINE_PASSWORD}
"#,
    )
    .unwrap();
    // Listing a registered driver avoids downloads; missing secrets defer its use.
    let jar = drivers.join("fixture.jar");
    let bytes = b"synthetic driver fixture";
    std::fs::write(&jar, bytes).unwrap();
    let driver = serde_json::json!({
        "version": 1, "vendor": "postgresql", "path": jar.to_str().unwrap(),
        "class": "org.postgresql.Driver", "sha256": hex::encode(Sha256::digest(bytes)),
    });
    std::fs::write(
        drivers.join("postgresql.toml"),
        toml::to_string(&driver).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args(["import-compose", "--non-interactive", "--path"])
        .arg(&project)
        .env("SAFESELECT_CONFIG_DIR", &global)
        .env("NO_COLOR", "1")
        .env_remove("SAFESELECT_COMPOSE_OFFLINE_PASSWORD")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Import Complete"), "{stdout}");
    assert!(stdout.contains("Imported 1 connection(s): db"), "{stdout}");
    assert!(
        stdout.contains("safeselect check --environment db"),
        "{stdout}"
    );
    assert!(
        stdout.contains("safeselect agent install opencode --environment db"),
        "{stdout}"
    );
    assert!(stdout.contains("Verification deferred:"), "{stdout}");
    assert!(!stdout.contains("All checks passed"));
    let saved = std::fs::read_to_string(project.join(".safeselect/environments/db.toml")).unwrap();
    assert!(saved.contains("SAFESELECT_COMPOSE_OFFLINE_PASSWORD"));
    std::fs::remove_dir_all(root).unwrap();
}
