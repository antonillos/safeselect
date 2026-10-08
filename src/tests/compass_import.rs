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
    assert_eq!(
        select_environment(&root, &[], "dev", true).unwrap(),
        Some("dev-2".into())
    );
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
    variables: std::collections::VecDeque<String>,
    names: std::collections::VecDeque<String>,
    fail_at: Option<&'static str>,
    allow_invalid_selection: bool,
}
impl CredentialInteraction for ScriptedInteraction {
    fn select(&mut self, _: &str, options: Vec<&str>) -> Result<String> {
        self.check_failure("selection")?;
        let selection = self.selections.pop_front().expect("unexpected selection");
        assert!(
            self.allow_invalid_selection || options.contains(&selection),
            "choice {selection} unavailable: {options:?}"
        );
        Ok(selection.into())
    }
    fn confirm(&mut self, _: &str) -> Result<bool> {
        self.check_failure("confirmation")?;
        Ok(self
            .confirmations
            .pop_front()
            .expect("unexpected confirmation"))
    }
    fn variable(&mut self, default: &str) -> Result<String> {
        self.check_failure("variable")?;
        self.defaults.push(default.into());
        Ok(self.variables.pop_front().unwrap_or_else(|| default.into()))
    }
    fn password(&mut self) -> Result<String> {
        self.check_failure("password")?;
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
            variables: Default::default(),
            names: Default::default(),
            fail_at: None,
            allow_invalid_selection: false,
        }
    }
    fn check_failure(&self, method: &str) -> Result<()> {
        if self.fail_at == Some(method) {
            return Err(SafeselectError::Other("Import cancelled".into()));
        }
        Ok(())
    }
}

impl EnvironmentInteraction for ScriptedInteraction {
    fn environment_name(&mut self, default: &str) -> Result<String> {
        self.check_failure("environment name")?;
        self.defaults.push(default.into());
        Ok(self
            .names
            .pop_front()
            .expect("unexpected environment name prompt"))
    }
}
#[derive(Default)]
struct RecordingStorage {
    macos: bool,
    present: bool,
    writes: Vec<(String, String)>,
    fail_store: bool,
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
        if self.fail_store {
            return Err(SafeselectError::Secret("Store failed".into()));
        }
        self.writes.push((name.into(), value.into()));
        Ok(())
    }
    fn session(&mut self, name: &str, value: &str) -> Result<()> {
        if self.fail_store {
            return Err(SafeselectError::Secret("Store failed".into()));
        }
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

struct TestDirectory(std::path::PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("compass-decisions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn environment_selection_updates_only_after_confirmation_and_can_skip() {
    let dir = TestDirectory::new();
    std::fs::write(dir.0.join("dev.toml"), "untouched").unwrap();
    let candidates = vec!["dev".into()];
    for consent in [false, true] {
        let mut ui = ScriptedInteraction::new(&["Update existing environment"], &[consent]);
        assert_eq!(
            select_environment_with(&dir.0, &candidates, "dev", &mut ui).unwrap(),
            consent.then(|| "dev".into())
        );
        assert!(ui.selections.is_empty());
        assert!(ui.confirmations.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.0.join("dev.toml")).unwrap(),
            "untouched"
        );
    }
    let mut ui = ScriptedInteraction::new(&["Skip this connection"], &[]);
    assert!(select_environment_with(&dir.0, &candidates, "dev", &mut ui)
        .unwrap()
        .is_none());
}

#[test]
fn environment_selection_asks_which_duplicate_to_update() {
    let dir = TestDirectory::new();
    let mut ui = ScriptedInteraction::new(&["Update existing environment", "copy"], &[true]);
    assert_eq!(
        select_environment_with(&dir.0, &["dev".into(), "copy".into()], "dev", &mut ui).unwrap(),
        Some("copy".into())
    );
    assert!(ui.selections.is_empty());
    assert!(ui.confirmations.is_empty());
}

#[test]
fn creating_an_environment_retries_empty_and_existing_names_without_overwriting() {
    let dir = TestDirectory::new();
    std::fs::write(dir.0.join("dev.toml"), "untouched").unwrap();
    let mut ui = ScriptedInteraction::new(&["Create a new environment"], &[]);
    ui.names = ["!!!", "dev", "Stage Copy"].map(str::to_string).into();
    let selected = select_environment_with(&dir.0, &["dev".into()], "dev", &mut ui).unwrap();
    assert_eq!(selected, Some("stage-copy".into()));
    assert_eq!(ui.defaults, vec!["dev-2", "dev-2", "dev-2"]);
    assert!(ui.names.is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.0.join("dev.toml")).unwrap(),
        "untouched"
    );
    assert!(!dir.0.join("stage-copy.toml").exists()); // Choosing alone never writes.
}

#[test]
fn first_import_asks_for_a_name_without_an_overwrite_selection() {
    let dir = TestDirectory::new();
    let mut ui = ScriptedInteraction::new(&[], &[]);
    ui.names.push_back("custom".into());
    assert_eq!(
        select_environment_with(&dir.0, &[], "dev", &mut ui).unwrap(),
        Some("custom".into())
    );
    assert_eq!(ui.defaults, vec!["dev"]);
}

#[test]
fn environment_selection_cancellation_and_invalid_choices_leave_files_untouched() {
    let dir = TestDirectory::new();
    let candidates = vec!["dev".into()];
    std::fs::write(dir.0.join("dev.toml"), "untouched").unwrap();
    for failure in ["selection", "confirmation", "environment name"] {
        let selection = match failure {
            "confirmation" => "Update existing environment",
            _ => "Create a new environment",
        };
        let mut ui = ScriptedInteraction::new(&[selection], &[]);
        ui.fail_at = Some(failure);
        assert!(select_environment_with(&dir.0, &candidates, "dev", &mut ui).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.0.join("dev.toml")).unwrap(),
            "untouched"
        );
    }
    let mut ui = ScriptedInteraction::new(&["private-invalid-choice"], &[]);
    ui.allow_invalid_selection = true;
    let error = select_environment_with(&dir.0, &candidates, "dev", &mut ui)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-invalid-choice"));
    let mut ui = ScriptedInteraction::new(&["private-unknown-environment"], &[]);
    ui.allow_invalid_selection = true;
    assert!(environment_to_update(&["dev".into(), "copy".into()], &mut ui).is_err());
}

#[test]
fn variable_prompt_retries_invalid_names_and_preserves_reference_syntax() {
    let mut values = ["bad;private", "", "1NAME", " {env:ATLAS_DEV_DB_PASSWORD} "].into_iter();
    let variable = prompt_variable_with("DEFAULT", |default| {
        assert_eq!(default, "DEFAULT");
        Ok(values.next().expect("unexpected prompt").into())
    })
    .unwrap();
    assert_eq!(variable, "ATLAS_DEV_DB_PASSWORD");
    assert!(values.next().is_none());
    assert_eq!(
        prompt_variable_with("DEFAULT", |_| Ok(" EXPORTED_PASSWORD ".into())).unwrap(),
        "EXPORTED_PASSWORD"
    );
    assert!(
        prompt_variable_with("DEFAULT", |_| Err(SafeselectError::Other(
            "Import cancelled".into()
        )))
        .is_err()
    );
}

#[test]
fn source_menu_only_offers_credentials_that_exist() {
    let existing = env_secret("LEGACY".into());
    let mut prompt = CredentialPrompt {
        project: "atlas",
        environment: "staging",
        ssh: false,
        imported: None,
        existing: None,
    };
    assert_eq!(
        prompt.source_choices(),
        vec![
            "Enter password (hidden)",
            "Use an exported environment variable",
            "Configure later"
        ]
    );
    prompt.imported = Some("");
    assert!(!prompt
        .source_choices()
        .contains(&"Use password from Compass export"));
    prompt.imported = Some("literal");
    prompt.existing = Some(&existing);
    assert_eq!(
        &prompt.source_choices()[..2],
        &[
            "Keep existing password source",
            "Use password from Compass export"
        ]
    );
}

#[test]
fn confirmed_replacement_reads_the_hidden_password_only_after_consent() {
    for ssh in [false, true] {
        let mut ui = ScriptedInteraction::new(
            &[
                "Enter password (hidden)",
                "Environment variable (this import session only)",
            ],
            &[true, true],
        );
        ui.password = Some("  literal  ".into());
        let mut storage = RecordingStorage {
            present: true,
            ..Default::default()
        };
        let secret = CredentialPrompt {
            project: "atlas",
            environment: "staging",
            ssh,
            imported: None,
            existing: None,
        }
        .run_with(&mut ui, &mut storage)
        .unwrap()
        .unwrap();
        assert_eq!(
            secret.variable.as_deref(),
            Some(friendly_variable("atlas", "staging", ssh).as_str())
        );
        assert_eq!(
            storage.writes,
            vec![(
                friendly_variable("atlas", "staging", ssh),
                "  literal  ".into()
            )]
        );
        assert!(ui.password.is_none());
        assert!(ui.confirmations.is_empty());
    }
}

#[test]
fn keychain_destination_does_not_reuse_an_existing_account_for_db_or_ssh() {
    for ssh in [false, true] {
        let existing = SecretConfig {
            source: "macos-keychain".into(),
            account: Some("shared-existing".into()),
            service: Some("safeselect".into()),
            variable: None,
        };
        let mut ui = ScriptedInteraction::new(
            &[
                "Use password from Compass export",
                "macOS Keychain (recommended)",
            ],
            &[],
        );
        let mut storage = RecordingStorage {
            macos: true,
            ..Default::default()
        };
        let secret = CredentialPrompt {
            project: "atlas",
            environment: "staging",
            ssh,
            imported: Some("literal"),
            existing: Some(&existing),
        }
        .run_with(&mut ui, &mut storage)
        .unwrap()
        .unwrap();
        let account = secret.account.unwrap();
        assert_ne!(account, "shared-existing");
        assert_eq!(account.ends_with("/ssh"), ssh);
        assert_eq!(storage.writes, vec![(account, "literal".into())]);
    }
}

#[test]
fn cancelled_credential_steps_do_not_read_or_store_a_password() {
    let prompt = CredentialPrompt {
        project: "atlas",
        environment: "staging",
        ssh: false,
        imported: None,
        existing: None,
    };
    for step in ["selection", "confirmation", "variable", "password"] {
        let mut ui = ScriptedInteraction::new(
            &[
                "Enter password (hidden)",
                "Environment variable (this import session only)",
            ],
            &[true],
        );
        ui.password = Some("unread".into());
        ui.fail_at = Some(step);
        let mut storage = RecordingStorage::default();
        assert!(prompt.run_with(&mut ui, &mut storage).is_err());
        assert!(storage.writes.is_empty());
        assert_eq!(ui.password.as_deref(), Some("unread"));
    }
}

#[test]
fn storage_failures_propagate_through_the_real_prompt_orchestration() {
    for destination in [
        "macOS Keychain (recommended)",
        "Environment variable (this import session only)",
    ] {
        let mut ui =
            ScriptedInteraction::new(&["Use password from Compass export", destination], &[true]);
        let mut storage = RecordingStorage {
            macos: true,
            fail_store: true,
            ..Default::default()
        };
        let result = CredentialPrompt {
            project: "atlas",
            environment: "staging",
            ssh: false,
            imported: Some("literal"),
            existing: None,
        }
        .run_with(&mut ui, &mut storage);
        assert!(result.is_err());
        assert!(storage.writes.is_empty());
    }
}

#[test]
fn invalid_source_destination_and_variable_are_rejected_without_reading_secrets() {
    let prompt = CredentialPrompt {
        project: "atlas",
        environment: "staging",
        ssh: false,
        imported: None,
        existing: None,
    };
    let mut ui = ScriptedInteraction::new(&[], &[]);
    let mut storage = RecordingStorage::default();
    assert!(prompt
        .select_source("private-invalid-source", &mut ui, &mut storage)
        .is_err());
    let mut ui = ScriptedInteraction::new(&["private-invalid-destination"], &[]);
    ui.allow_invalid_selection = true;
    assert!(prompt.select_destination(&mut ui, &storage).is_err());
    let mut ui = ScriptedInteraction::new(&["Use an exported environment variable"], &[]);
    ui.variables.push_back("bad;private-value".into());
    let error = prompt
        .run_with(&mut ui, &mut storage)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-value"));
    assert!(storage.writes.is_empty());
}

fn legacy_tunnel_fixture() -> (
    crate::compass::CompassConnection,
    crate::config::EnvironmentConfig,
) {
    let mut conn = connection("mongodb://demo:literal@db:27017/app?authSource=admin");
    conn.ssh_host = Some("bastion".into());
    conn.ssh_user = Some("demo".into());
    // A missing exported port has the same identity as the saved default port 22.
    let mut existing: crate::config::EnvironmentConfig = toml::from_str("version=1\n[database]\nkind='document'\nurl='placeholder'\nusername='demo'\n[ssh]\nenabled=true\nhost='bastion'\nport=22\nusername='demo'\nlocal_host='localhost'\nlocal_port=15432\nforward_host='db'\nforward_port=27017\n").unwrap();
    let url = crate::rewrite_mongodb_url_for_local_endpoint(&conn.url, "localhost", 15432).unwrap();
    existing.database.url = split_database_url(&url).unwrap().0;
    (conn, existing)
}

#[test]
fn legacy_tunnel_identity_matches_defaults_without_reading_credentials() {
    let (conn, existing) = legacy_tunnel_fixture();
    assert!(legacy_match(&conn, &existing));
    let mut without_local_endpoint = existing.clone();
    let ssh = without_local_endpoint.ssh.as_mut().unwrap();
    ssh.local_host = None;
    ssh.local_port = None;
    assert!(legacy_match(&conn, &without_local_endpoint));
    assert!(same_legacy_bastion(&conn, existing.ssh.as_ref().unwrap()));
    assert!(same_legacy_forward_target(
        &conn,
        existing.ssh.as_ref().unwrap()
    ));
}

#[test]
fn legacy_identity_rejects_mismatched_backend_username_and_tunnel_fields() {
    let (conn, existing) = legacy_tunnel_fixture();
    let mut wrong_backend = existing.clone();
    wrong_backend.database.kind = crate::backend::BackendKind::Jdbc;
    assert!(!legacy_match(&conn, &wrong_backend));
    let mut wrong_user = existing.clone();
    wrong_user.database.username = "other".into();
    assert!(!legacy_match(&conn, &wrong_user));
    let mut wrong_url = existing.clone();
    wrong_url.database.url = wrong_url.database.url.replace("/app?", "/other?");
    assert!(!legacy_match(&conn, &wrong_url));
    for field in ["host", "username", "port", "forward host", "forward port"] {
        let mut wrong = existing.clone();
        let ssh = wrong.ssh.as_mut().unwrap();
        match field {
            "host" => ssh.host = Some("other".into()),
            "username" => ssh.username = Some("other".into()),
            "port" => ssh.port = Some(2222),
            "forward host" => ssh.forward_host = Some("other".into()),
            "forward port" => ssh.forward_port = Some(27018),
            _ => unreachable!(),
        }
        assert!(
            !legacy_match(&conn, &wrong),
            "mismatched {field} must not match"
        );
    }
    let mut without_tunnel = existing.clone();
    without_tunnel.ssh = None;
    assert!(!legacy_match(&conn, &without_tunnel));
    without_tunnel.ssh = existing.ssh.clone();
    without_tunnel.ssh.as_mut().unwrap().enabled = false;
    assert!(!legacy_match(&conn, &without_tunnel));
}

#[test]
fn legacy_identity_never_matches_two_invalid_credentials_or_unparseable_targets() {
    let (mut conn, mut existing) = legacy_tunnel_fixture();
    conn.url = "mongodb://demo@db:invalid/app".into();
    assert!(!legacy_match(&conn, &existing));
    conn.ssh_host = None;
    existing.ssh = None;
    conn.url = "mongodb://demo:bad%ZZ@db/app".into();
    existing.database.url = conn.url.clone();
    assert!(!legacy_match(&conn, &existing));
    conn.url = "mongodb://demo:valid@db/app".into();
    assert!(!legacy_match(&conn, &existing));
}

#[test]
fn legacy_discovery_skips_malformed_unreadable_and_non_environment_entries() {
    let dir = TestDirectory::new();
    let conn = connection("mongodb://demo@db/app");
    assert!(legacy_candidates(&dir.0.join("missing"), &conn).is_empty());
    let envs = dir.0.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    std::fs::write(envs.join("notes.txt"), "not an environment").unwrap();
    std::fs::write(envs.join("broken.toml"), "invalid TOML [").unwrap();
    std::fs::write(envs.join("missing-bastion.toml"), "version=1\n[database]\nkind='document'\nurl='mongodb://demo@db/app'\nusername='demo'\n[ssh]\nenabled=true\nbastion='missing'\n").unwrap();
    std::fs::create_dir_all(envs.join("directory.toml")).unwrap();
    std::fs::write(envs.join("custom.toml"), "version=1\n[database]\nkind='document'\nurl='mongodb://demo:__SAFESELECT_PASSWORD__@db/app'\nusername='demo'\n").unwrap();
    std::fs::write(dir.0.join("project.toml"), "version=1\n").unwrap();
    assert_eq!(
        ImportIndex::default().candidates(&envs, &conn, "default"),
        vec!["custom"]
    );
    assert!(legacy_candidate(&envs.join("notes.txt"), &conn, None).is_none());
    assert!(legacy_candidate(&envs.join("extensionless"), &conn, None).is_none());
    assert!(load_legacy_environment(&envs.join("nonexistent.toml"), None).is_none());
    assert!(load_legacy_environment(&envs.join("broken.toml"), None).is_none());
}

#[test]
fn legacy_discovery_merges_shared_bastions_without_changing_the_project() {
    let dir = TestDirectory::new();
    let envs = dir.0.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    let (conn, mut existing) = legacy_tunnel_fixture();
    let mut project = crate::config::ProjectConfig::default();
    let shared = crate::project_ssh_bastion_from_env(existing.ssh.as_ref().unwrap());
    project.ssh_bastions.insert("shared".into(), shared);
    existing.ssh = Some(crate::environment_ssh_from_bastion(
        "shared".into(),
        existing.ssh.as_ref().unwrap(),
    ));
    let original = toml::to_string(&project).unwrap();
    std::fs::write(dir.0.join("project.toml"), &original).unwrap();
    std::fs::write(
        envs.join("renamed.toml"),
        toml::to_string(&existing).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ImportIndex::default().candidates(&envs, &conn, "staging"),
        vec!["renamed"]
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join("project.toml")).unwrap(),
        original
    );
}

#[test]
fn database_urls_reject_query_credentials_without_echoing_values() {
    for option in [
        "password",
        "tlsCertificateKeyFilePassword",
        "proxyPassword",
        "authMechanismProperties",
        "PASSWORD",
        "pass%77ord",
        "accessToken",
        "clientSecret",
        "username",
        "proxyUsername",
    ] {
        for authority in ["db", "demo:fixture@db"] {
            let url = format!("mongodb://{authority}/app?{option}=sensitive-fixture");
            let error = split_database_url(&url).unwrap_err().to_string();
            assert!(!error.contains("sensitive-fixture"));
            assert!(error.contains("unsupported credential-bearing query options"));
        }
    }
    assert!(split_database_url("mongodb://db/app?pass%ZZword=value").is_err());
}

#[test]
fn database_urls_preserve_noncredential_query_options() {
    for url in [
        "mongodb://db/app?authSource=admin&tls=true&replicaSet=rs0",
        "mongodb+srv://db/app?retryWrites=true&w=majority",
    ] {
        assert_eq!(split_database_url(url).unwrap().0, url);
    }
    let (url, _, _) =
        split_database_url("mongodb://demo:fixture@db/app?authSource=admin&tls=true").unwrap();
    assert!(!url.contains("fixture"));
    assert!(url.ends_with("?authSource=admin&tls=true"));
}

#[test]
fn long_selection_answers_start_below_the_question() {
    assert!(!selection_render_config("Environment:", 7).answer_from_new_line);
    assert!(selection_render_config("Connection:", 100).answer_from_new_line);
    assert!(
        selection_render_config("Where do you want to keep this password?", 45)
            .answer_from_new_line
    );
}

#[test]
fn imported_urls_reject_fragments_without_echoing_them() {
    for url in [
        "mongodb://db/app#password=synthetic-value",
        "mongodb://db/app?authSource=admin#token=synthetic-value",
        "mongodb://demo:fixture@db/app#synthetic-value",
        "mongodb+srv://db/app#",
    ] {
        let error = split_database_url(url).unwrap_err().to_string();
        assert!(error.contains("must not contain fragments"));
        assert!(!error.contains("synthetic-value"));
    }
    assert!(split_database_url("mongodb://demo:encoded%23password@db/app").is_ok());
}
