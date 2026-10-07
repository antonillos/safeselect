use super::*;

#[test]
fn readable_names_include_project_environment_and_purpose() {
    assert_eq!(
        friendly_variable("my-app", "staging", false),
        "MY_APP_STAGING_DB_PASSWORD"
    );
    assert_eq!(
        friendly_variable("my-app", "staging", true),
        "MY_APP_STAGING_SSH_PASSWORD"
    );
    assert_ne!(
        friendly_variable("one", "dev", false),
        friendly_variable("two", "dev", false)
    );
    password::validate_variable(&friendly_variable("123 app", "", false)).unwrap();
}

#[test]
fn connection_identity_ignores_passwords_but_not_targets() {
    let mut conn = connection("mongodb://demo:first@db:27017/app");
    let first = fingerprint(&conn);
    conn.url = "mongodb://demo:second@db:27017/app".into();
    assert_eq!(first, fingerprint(&conn));
    conn.url = "mongodb://demo@db:27017/app".into();
    assert_eq!(first, fingerprint(&conn));
    conn.url = "mongodb://demo@other:27017/app".into();
    assert_ne!(first, fingerprint(&conn));
    assert!(!display_url("mongodb://demo:private@db/app").contains("private"));
}

fn connection(url: &str) -> crate::compass::CompassConnection {
    let value = serde_json::json!({"connectionString": url});
    let dir = std::env::temp_dir().join(format!("compass-ux-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("connections.json");
    std::fs::write(&file, value.to_string()).unwrap();
    let conn = crate::compass::import_path(&file).unwrap().remove(0);
    std::fs::remove_dir_all(dir).unwrap();
    conn
}

#[test]
fn imported_passwords_are_decoded_literals_not_references() {
    let (_, user, secret) =
        split_database_url("mongodb://demo:%7Benv%3ALITERAL%7D@db/app").unwrap();
    assert_eq!(user, "demo");
    assert_eq!(secret.as_deref(), Some("{env:LITERAL}"));
    let (_, _, secret) = split_database_url("mongodb://demo:p%40ss%3Aword%25@db/app").unwrap();
    assert_eq!(secret.as_deref(), Some("p@ss:word%"));
    let (_, _, secret) = split_database_url("mongodb://demo@db/app").unwrap();
    assert!(secret.is_none());
    assert!(split_database_url("mongodb://demo:bad%ZZ@db/app").is_err());
    // An @ in the path is not authentication.
    assert!(split_database_url("mongodb://db/app@other")
        .unwrap()
        .1
        .is_empty());
}

#[test]
fn storage_never_interprets_literal_references_or_writes_them_to_config() {
    let secret = store_literal(
        "{env:LITERAL}".into(),
        Destination::Keychain("test".into()),
        |account, value| {
            assert_eq!(account, "test");
            assert_eq!(value, "{env:LITERAL}");
            Ok(())
        },
        |_, _| panic!("wrong destination"),
    )
    .unwrap();
    assert_eq!(secret.source, "macos-keychain");
    let secret = store_literal(
        "literal".into(),
        Destination::Session("APP_DEV_DB_PASSWORD".into()),
        |_, _| panic!("wrong destination"),
        |name, value| {
            assert_eq!(name, "APP_DEV_DB_PASSWORD");
            assert_eq!(value, "literal");
            Ok(())
        },
    )
    .unwrap();
    let toml = toml::to_string(&secret).unwrap();
    assert!(!toml.contains("literal"));
    assert_eq!(secret.variable.as_deref(), Some("APP_DEV_DB_PASSWORD"));
    assert!(store_literal(
        "".into(),
        Destination::Session("VALID".into()),
        |_, _| Ok(()),
        |_, _| Ok(())
    )
    .is_err());
    assert!(store_literal(
        "secret".into(),
        Destination::Session("BAD;private".into()),
        |_, _| panic!(),
        |_, _| panic!()
    )
    .is_err());
    assert!(store_literal(
        "secret".into(),
        Destination::Keychain("test".into()),
        |_, _| Err(SafeselectError::Secret("store failed".into())),
        |_, _| panic!()
    )
    .is_err());
}

#[test]
fn provenance_matches_renamed_environment_and_rejects_path_traversal() {
    let root = std::env::temp_dir().join(format!("compass-index-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("environments")).unwrap();
    std::fs::write(root.join("environments/custom.toml"), "version=1").unwrap();
    let mut index = ImportIndex::default();
    let conn = connection("mongodb://demo@db/app");
    index.record(&conn, "custom");
    index.save(&root).unwrap();
    let loaded = ImportIndex::load(&root).unwrap();
    assert_eq!(
        loaded.candidates(&root.join("environments"), &conn, "default"),
        vec!["custom"]
    );
    std::fs::write(
        root.join("compass-imports.toml"),
        "[connections]\nid = ['../private']",
    )
    .unwrap();
    assert!(ImportIndex::load(&root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn new_bastion_credentials_do_not_modify_a_shared_definition() {
    let mut project = crate::config::ProjectConfig::default();
    let mut ssh: crate::config::SshConfig = toml::from_str("enabled=true\nhost='bastion'\nport=22\nusername='demo'\nauth_type='PASSWORD'\nsecret_variable='OLD_SSH_PASSWORD'").unwrap();
    let old_name = register_bastion(&mut project, &ssh);
    assert_eq!(register_bastion(&mut project, &ssh), old_name);
    ssh.secret_variable = Some("NEW_SSH_PASSWORD".into());
    let new_name = register_bastion(&mut project, &ssh);
    assert_ne!(old_name, new_name);
    assert_eq!(
        project.ssh_bastions[&old_name].secret_variable.as_deref(),
        Some("OLD_SSH_PASSWORD")
    );
    assert_eq!(
        project.ssh_bastions[&new_name].secret_variable.as_deref(),
        Some("NEW_SSH_PASSWORD")
    );
}

#[test]
fn unattended_duplicate_import_never_selects_an_overwrite() {
    let root = std::env::temp_dir().join(format!("compass-duplicate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("dev.toml"), "original").unwrap();
    assert_eq!(
        select_environment(&root, &["dev".into()], "dev", true).unwrap(),
        None
    );
    assert_eq!(select_environment(&root, &[], "dev", true).unwrap(), None);
    assert_eq!(
        select_environment(&root, &[], "staging", true).unwrap(),
        Some("staging".into())
    );
    assert_eq!(
        std::fs::read_to_string(root.join("dev.toml")).unwrap(),
        "original"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn compass_nested_options_import_once_and_keep_separate_bastions() {
    let root = std::env::temp_dir().join(format!("compass-nested-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("connections.json");
    let json = serde_json::json!({"connections": [
        {"name":"one", "connectionOptions":{"connectionString":"mongodb://demo@db/app", "sshTunnel":{"host":"first", "password":"{env:LITERAL}", "authenticationMethod":"password"}}},
        {"name":"two", "connectionOptions":{"connectionString":"mongodb://demo@db/app", "sshTunnel":{"host":"second"}}}
    ]});
    std::fs::write(&file, json.to_string()).unwrap();
    let connections = crate::compass::import_path(&file).unwrap();
    assert_eq!(connections.len(), 2);
    assert_eq!(
        connections[0].ssh_password.as_deref(),
        Some("{env:LITERAL}")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_save_replaces_file_without_following_target_symlink() {
    let root = std::env::temp_dir().join(format!("compass-atomic-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("original");
    let path = root.join("env.toml");
    std::fs::write(&target, "untouched").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &path).unwrap();
    write_atomic(&path, "updated").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_custom_names_are_found_without_an_import_index() {
    let root = std::env::temp_dir().join(format!("compass-legacy-{}", uuid::Uuid::new_v4()));
    let envs = root.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    let conn = connection("mongodb://demo:new@db:27017/app?authSource=admin");
    std::fs::write(envs.join("custom.toml"), "version=1\n[database]\nkind='document'\nurl='mongodb://demo:__SAFESELECT_PASSWORD__@db:27017/app?authSource=admin'\nusername='demo'\n").unwrap();
    assert_eq!(
        ImportIndex::default().candidates(&envs, &conn, "staging"),
        vec!["custom"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn index_updates_do_not_leave_an_overwritten_environment_under_old_identity() {
    let mut index = ImportIndex::default();
    let first = connection("mongodb://demo@first/app");
    let second = connection("mongodb://demo@second/app");
    index.record(&first, "staging");
    index.record(&second, "staging");
    assert!(index.connections[&fingerprint(&first)].is_empty());
    assert_eq!(index.connections[&fingerprint(&second)], vec!["staging"]);
}

#[test]
fn session_storage_rejects_nul_without_echoing_or_setting_the_value() {
    let error = store_literal(
        "private\0value".into(),
        Destination::Session("VALID".into()),
        |_, _| panic!("must not store"),
        |_, _| panic!("must not set"),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(!error.contains("private"));
}

struct ScriptedInteraction {
    selections: std::collections::VecDeque<&'static str>,
    confirmations: std::collections::VecDeque<bool>,
    password: Option<String>,
    defaults: Vec<String>,
}
impl CredentialInteraction for ScriptedInteraction {
    fn select(&mut self, _: &str, options: Vec<&str>) -> Result<String> {
        let selection = self.selections.pop_front().expect("unexpected selection");
        assert!(
            options.contains(&selection),
            "choice {selection} unavailable: {options:?}"
        );
        Ok(selection.into())
    }
    fn confirm(&mut self, _: &str) -> Result<bool> {
        Ok(self
            .confirmations
            .pop_front()
            .expect("unexpected confirmation"))
    }
    fn variable(&mut self, default: &str) -> Result<String> {
        self.defaults.push(default.into());
        Ok(default.into())
    }
    fn password(&mut self) -> Result<String> {
        Ok(self
            .password
            .take()
            .expect("must not request a password twice"))
    }
}
impl ScriptedInteraction {
    fn new(selections: &[&'static str], confirmations: &[bool]) -> Self {
        Self {
            selections: selections.iter().copied().collect(),
            confirmations: confirmations.iter().copied().collect(),
            password: None,
            defaults: vec![],
        }
    }
}
#[derive(Default)]
struct RecordingStorage {
    macos: bool,
    present: bool,
    writes: Vec<(String, String)>,
}
impl CredentialStorage for RecordingStorage {
    fn is_macos(&self) -> bool {
        self.macos
    }
    fn variable_present(&self, _: &str) -> bool {
        self.present
    }
    fn keychain(&mut self, name: &str, value: &str) -> Result<()> {
        assert!(self.macos);
        self.writes.push((name.into(), value.into()));
        Ok(())
    }
    fn session(&mut self, name: &str, value: &str) -> Result<()> {
        self.writes.push((name.into(), value.into()));
        Ok(())
    }
}

#[test]
fn database_and_ssh_use_identical_explicit_import_and_destination_flow_on_both_platforms() {
    for ssh in [false, true] {
        for macos in [false, true] {
            let mut storage = RecordingStorage {
                macos,
                ..Default::default()
            };
            let mut ui = ScriptedInteraction::new(
                &[
                    "Use password from Compass export",
                    "Environment variable (this import session only)",
                ],
                &[true],
            );
            let secret = CredentialPrompt {
                project: "atlas",
                environment: "staging",
                ssh,
                imported: Some("{env:LITERAL}"),
                existing: None,
            }
            .run_with(&mut ui, &mut storage)
            .unwrap()
            .unwrap();
            assert_eq!(
                storage.writes,
                vec![(
                    friendly_variable("atlas", "staging", ssh),
                    "{env:LITERAL}".into()
                )]
            );
            assert_eq!(secret.source, "env");
            assert!(ui.password.is_none());
        }
    }
}

#[test]
fn hidden_direct_passwords_preserve_whitespace_and_choose_keychain_explicitly() {
    let mut storage = RecordingStorage {
        macos: true,
        ..Default::default()
    };
    let mut ui = ScriptedInteraction::new(
        &["Enter password (hidden)", "macOS Keychain (recommended)"],
        &[],
    );
    ui.password = Some("  literal password  ".into());
    let secret = CredentialPrompt {
        project: "atlas",
        environment: "staging",
        ssh: false,
        imported: None,
        existing: None,
    }
    .run_with(&mut ui, &mut storage)
    .unwrap()
    .unwrap();
    assert_eq!(secret.source, "macos-keychain");
    assert_eq!(storage.writes[0].1, "  literal password  ");
    assert!(storage.writes[0].0.starts_with("atlas/staging/compass-"));
}

#[test]
fn existing_sources_and_existing_exported_variables_do_not_write_any_secret() {
    let existing = env_secret("LEGACY_REFERENCE".into());
    for selection in [
        "Keep existing password source",
        "Use an exported environment variable",
        "Configure later",
    ] {
        let mut ui = ScriptedInteraction::new(&[selection], &[]);
        let mut storage = RecordingStorage::default();
        let secret = CredentialPrompt {
            project: "atlas",
            environment: "staging",
            ssh: true,
            imported: Some("private"),
            existing: Some(&existing),
        }
        .run_with(&mut ui, &mut storage)
        .unwrap()
        .unwrap();
        assert_eq!(secret.variable.as_deref(), Some("LEGACY_REFERENCE"));
        assert!(storage.writes.is_empty());
        assert!(ui.password.is_none());
    }
}

#[test]
fn declined_session_or_variable_replacement_never_consumes_or_writes_a_password() {
    for confirmations in [vec![false], vec![true, false]] {
        let mut ui = ScriptedInteraction::new(
            &[
                "Enter password (hidden)",
                "Environment variable (this import session only)",
                "Configure later",
            ],
            &confirmations,
        );
        ui.password = Some("must-not-read".into());
        let mut storage = RecordingStorage {
            present: true,
            ..Default::default()
        };
        CredentialPrompt {
            project: "atlas",
            environment: "staging",
            ssh: false,
            imported: None,
            existing: None,
        }
        .run_with(&mut ui, &mut storage)
        .unwrap();
        assert!(storage.writes.is_empty());
        assert_eq!(ui.password.as_deref(), Some("must-not-read"));
    }
}
