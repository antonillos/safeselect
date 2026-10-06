use super::*;

#[test]
fn explicit_references_and_literal_escape() {
    assert!(
        matches!(PasswordInput::parse("{env:DB_PASSWORD}".into()).unwrap(), PasswordInput::Environment(v) if v == "DB_PASSWORD")
    );
    for value in [
        "ordinary",
        "$PASSWORD",
        "${PASSWORD}",
        "prefix{env:NAME}",
        " spaced ",
    ] {
        assert!(
            matches!(PasswordInput::parse(value.into()).unwrap(), PasswordInput::Literal(v) if v == value)
        );
    }
    assert!(
        matches!(PasswordInput::literal("{env:NAME}".into()).unwrap(), PasswordInput::Literal(v) if v == "{env:NAME}")
    );
    assert_eq!(
        variable_input(" {env:SSH_PASSWORD} ").unwrap(),
        "SSH_PASSWORD"
    );
    assert_eq!(variable_input(" SSH_PASSWORD ").unwrap(), "SSH_PASSWORD");
}

#[test]
fn rejects_invalid_references_without_echoing_input() {
    for value in [
        "{env:}",
        "{env:1NAME}",
        "{env:A-B}",
        "{env:NAME",
        "{env:NAME}suffix",
        "{env:NAME}}",
        "{env:BAD;private-value}",
        "{file:/private-value}",
        "",
    ] {
        let error = PasswordInput::parse(value.into())
            .err()
            .expect("must fail")
            .to_string();
        assert!(!error.contains("private-value"));
    }
    for value in ["", "1NAME", "A B", "NAME;echo", "é", "A\nB"] {
        assert!(validate_variable(value).is_err());
    }
}

#[test]
fn mongodb_placeholder_adds_missing_userinfo_and_preserves_authority_and_suffix() {
    for scheme in ["mongodb", "mongodb+srv"] {
        for authority in ["host", "host:27017,other:27018", "[::1]:27017"] {
            let suffix = "/db?authSource=admin&appName=demo@example";
            for userinfo in ["", "demo@", "demo:old-placeholder@"] {
                assert_eq!(
                    inject_mongodb_password_placeholder(
                        &format!("{scheme}://{userinfo}{authority}{suffix}"),
                        "demo"
                    ),
                    format!("{scheme}://demo:__SAFESELECT_PASSWORD__@{authority}{suffix}")
                );
            }
        }
        assert_eq!(
            inject_mongodb_password_placeholder(&format!("{scheme}://host"), "demo"),
            format!("{scheme}://demo:__SAFESELECT_PASSWORD__@host")
        );
    }
    assert_eq!(
        inject_mongodb_password_placeholder("invalid", "demo"),
        "invalid"
    );
}
