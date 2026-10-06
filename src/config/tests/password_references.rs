use super::*;

#[test]
fn required_environment_passwords_fail_closed_without_disclosing_values() {
    let variable = format!("SAFESELECT_TEST_{}", uuid::Uuid::new_v4().simple());
    let loader = ConfigLoader::new();
    let mut secret = SecretConfig {
        source: "env".into(),
        service: None,
        account: None,
        variable: Some(variable.clone()),
    };
    assert!(matches!(
        loader.resolve_secret(&secret),
        Err(SafeselectError::EnvVarNotSet(_))
    ));
    std::env::set_var(&variable, "");
    assert!(loader.resolve_secret(&secret).is_err());
    std::env::set_var(&variable, " synthetic password ");
    assert_eq!(
        loader.resolve_secret(&secret).unwrap(),
        " synthetic password "
    );
    std::env::remove_var(&variable);
    secret.variable = Some("INVALID;private-value".into());
    assert!(!loader
        .resolve_secret(&secret)
        .unwrap_err()
        .to_string()
        .contains("private-value"));
    secret.variable = None;
    assert!(loader.resolve_secret(&secret).is_err());
}
