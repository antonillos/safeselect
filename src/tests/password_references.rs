use super::*;

#[test]
fn password_references_save_only_sources_for_database_and_ssh() {
    let root = std::env::temp_dir().join(format!("safeselect-reference-{}", uuid::Uuid::new_v4()));
    let environments = root.join(".safeselect/environments");
    std::fs::create_dir_all(&environments).unwrap();
    let file = environments.join("dev.toml");
    std::fs::write(&file, "version = 1\n[database]\nkind = 'document'\nurl = 'mongodb://demo@localhost/demo'\nusername = 'demo'\n[ssh]\nenabled = true\nidentity_file = '/tmp/old-key'\nsecret_account = 'old-account'\n").unwrap();
    let loader = ConfigLoader::new();
    for ssh in [false, true] {
        let variable = format!("SAFESELECT_TEST_{}", uuid::Uuid::new_v4().simple());
        // Configure while absent: configuration must never read the secret.
        save_password_input(
            &root,
            "dev",
            config::password::PasswordInput::parse(format!("{{env:{variable}}}")).unwrap(),
            ssh,
            |_, _| panic!("References must not access Keychain"),
        )
        .unwrap();
        let saved = std::fs::read_to_string(&file).unwrap();
        let config: config::EnvironmentConfig = toml::from_str(&saved).unwrap();
        if ssh {
            let ssh = config.ssh.unwrap();
            assert_eq!(ssh.secret_variable.as_deref(), Some(variable.as_str()));
            assert!(ssh.secret_account.is_none());
            assert!(ssh.identity_file.is_none());
            assert_eq!(ssh.auth_type.as_deref(), Some("PASSWORD"));
        } else {
            let secret = config.database.secret.unwrap();
            assert_eq!(secret.source, "env");
            assert_eq!(secret.variable.as_deref(), Some(variable.as_str()));
            assert!(secret.account.is_none());
            assert!(config.database.url.contains("__SAFESELECT_PASSWORD__"));
            // Existing env sources survive import setup on every platform.
            setup_passwords_for_missing(&root, &["dev".into()]).unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
        }
        configure_password_input(
            &loader,
            Some("dev".into()),
            Some(format!("{{env:{variable}}}")),
            Some(root.clone()),
            false,
            ssh,
        )
        .unwrap();
        let previous = std::fs::read_to_string(&file).unwrap();
        let error = configure_password_input(
            &loader,
            Some("dev".into()),
            Some("{env:BAD;private-value}".into()),
            Some(root.clone()),
            false,
            ssh,
        )
        .unwrap_err();
        assert!(!error.to_string().contains("private-value"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), previous);
    }
    // An explicitly literal value must not be interpreted as a reference.
    save_password_input(
        &root,
        "dev",
        config::password::PasswordInput::literal("{env:LITERAL}".into()).unwrap(),
        false,
        |_, password| {
            assert_eq!(password, "{env:LITERAL}");
            Ok(())
        },
    )
    .unwrap();
    let saved = std::fs::read_to_string(&file).unwrap();
    assert!(saved.contains("macos-keychain"));
    assert!(!saved.contains("{env:LITERAL}"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn interactive_password_sources_accept_references_without_keychain() {
    for is_macos in [false, true] {
        let result = prompt_ssh_password_source_with(
            Path::new("."),
            "demo/dev/ssh",
            is_macos,
            || Ok("{env:BASTION_PASSWORD}".into()),
            |_| Ok("{env:BASTION_PASSWORD}".into()),
            |_, _| panic!("References must not be stored as passwords"),
        )
        .unwrap();
        assert_eq!(result, (None, Some("BASTION_PASSWORD".into())));
    }
    let secret = database_password_input_with_store("{env:DB_PASSWORD}", "demo/dev", |_, _| {
        panic!("must not store")
    })
    .unwrap();
    assert_eq!(secret.variable.as_deref(), Some("DB_PASSWORD"));
    assert!(database_password_input_with_store(
        "{env:BAD;private-value}",
        "unused",
        |_, _| panic!("invalid input must not store")
    )
    .is_err());
    // Credentials imported from external exports remain opaque literal values.
    let secret = import_database_password_with_store(
        Path::new("."),
        "demo",
        "dev",
        "{env:LITERAL}",
        true,
        |_, password| {
            assert_eq!(password, "{env:LITERAL}");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(secret.source, "macos-keychain");
}

#[test]
fn password_input_selection_covers_platforms_and_preserves_literals() {
    use config::password::PasswordInput;
    for macos in [false, true] {
        let input = read_configured_password_with(
            Some("{env:DB_PASSWORD}".into()),
            false,
            macos,
            || panic!("explicit input must not prompt"),
            || panic!("explicit input must not prompt"),
        )
        .unwrap();
        assert!(matches!(input, PasswordInput::Environment(v) if v == "DB_PASSWORD"));
    }
    assert!(matches!(read_configured_password_with(None, false, false,
        || Ok("{env:DB_PASSWORD}".into()), || panic!("Linux must request a reference")).unwrap(),
        PasswordInput::Environment(v) if v == "DB_PASSWORD"));
    assert!(read_configured_password_with(
        None,
        false,
        false,
        || Ok("BAD;private-value".into()),
        || panic!("must not prompt password")
    )
    .is_err());
    assert!(matches!(read_configured_password_with(None, true, true,
        || panic!("macOS requests a password"), || Ok("{env:LITERAL}".into())).unwrap(),
        PasswordInput::Literal(v) if v == "{env:LITERAL}"));
    assert!(read_configured_password_with(
        None,
        false,
        true,
        || panic!("must not prompt reference"),
        || Err(SafeselectError::Other("cancelled".into()))
    )
    .is_err());
    assert!(validate_literal_password_platform(false).is_ok());
    assert_eq!(
        validate_literal_password_platform(true).is_ok(),
        cfg!(target_os = "macos")
    );
    assert!(
        validate_password_storage_platform(&PasswordInput::Environment("DB_PASSWORD".into()))
            .is_ok()
    );
    assert_eq!(
        validate_password_storage_platform(&PasswordInput::Literal("synthetic".into())).is_ok(),
        cfg!(target_os = "macos")
    );
}

#[test]
fn default_password_names_and_keychain_selection_preserve_existing_sources() {
    let root = std::env::temp_dir().join(format!("safeselect-source-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg: config::EnvironmentConfig =
        toml::from_str("version=1\n[database]\nurl='mongodb://localhost/demo'\n").unwrap();
    assert!(validate_password_target(&cfg, false).is_ok());
    assert!(validate_password_target(&cfg, true).is_err());
    assert_eq!(
        default_password_variable(&root, "dev", &cfg, false).unwrap(),
        compose::database_env_reference(&root, "dev").unwrap()
    );
    cfg.database.secret = Some(environment_password_secret("CUSTOM_DB".into()));
    assert_eq!(
        default_password_variable(&root, "dev", &cfg, false).unwrap(),
        "CUSTOM_DB"
    );
    cfg.ssh = Some(toml::from_str("enabled=true\nsecret_variable='CUSTOM_SSH'").unwrap());
    assert!(validate_password_target(&cfg, true).is_ok());
    assert_eq!(
        default_password_variable(&root, "dev", &cfg, true).unwrap(),
        "CUSTOM_SSH"
    );
    assert_eq!(
        select_keychain_ssh_password_source("demo/dev/ssh", String::new(), |_, _| panic!(
            "empty input must not store"
        ))
        .unwrap(),
        (Some("demo/dev/ssh".into()), None)
    );
    let literal = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        select_keychain_ssh_password_source("demo/dev/ssh", literal.clone(), |account, value| {
            assert_eq!(account, "demo/dev/ssh");
            assert_eq!(value, literal);
            Ok(())
        })
        .unwrap(),
        (Some("demo/dev/ssh".into()), None)
    );
    assert!(
        select_keychain_ssh_password_source("demo/dev/ssh", literal, |_, _| Err(
            SafeselectError::Other("store failed".into())
        ))
        .is_err()
    );
    assert!(select_keychain_ssh_password_source(
        "demo/dev/ssh",
        "{env:BAD;private-value}".into(),
        |_, _| panic!("invalid input must not store")
    )
    .is_err());
    std::fs::remove_dir_all(root).unwrap();
}
