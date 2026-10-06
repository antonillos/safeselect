use super::*;

#[test]
fn mcp_password_reference_configuration_does_not_read_or_store_secrets() {
    let root = std::env::temp_dir().join(format!(
        "safeselect-mcp-password-ref-{}",
        uuid::Uuid::new_v4()
    ));
    let env_dir = root.join(".safeselect/environments");
    std::fs::create_dir_all(&env_dir).unwrap();
    let file = env_dir.join("dev.toml");
    std::fs::write(
        &file,
        "version = 1\n[database]\nurl = 'mongodb://localhost/demo'\n",
    )
    .unwrap();
    let mut server = test_server(&root);
    let variable = format!("SAFESELECT_TEST_{}", uuid::Uuid::new_v4().simple());
    server
        .handle_config_set_password(
            Some(serde_json::json!(1)),
            &serde_json::json!({"environment":"dev","password":format!("{{env:{variable}}}")}),
        )
        .unwrap();
    let saved = std::fs::read_to_string(&file).unwrap();
    let config: EnvironmentConfig = toml::from_str(&saved).unwrap();
    let secret = config.database.secret.unwrap();
    assert_eq!(secret.source, "env");
    assert_eq!(secret.variable.as_deref(), Some(variable.as_str()));
    for password in ["{env:BAD;private-value}", "{file:/private-value}"] {
        server
            .handle_config_set_password(
                Some(serde_json::json!(2)),
                &serde_json::json!({"environment":"dev","password":password}),
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
    }
    std::fs::remove_dir_all(root).unwrap();
}
