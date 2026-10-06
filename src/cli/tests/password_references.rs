use super::*;

#[test]
fn password_reference_flags_and_literal_escape() {
    for action in ["set-password", "set-ssh-password"] {
        assert!(Cli::try_parse_from([
            "safeselect",
            "config",
            action,
            "--password",
            "{env:DEMO_PASSWORD}"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "safeselect",
            "config",
            action,
            "--password",
            "{env:LITERAL}",
            "--literal-password"
        ])
        .is_ok());
        assert!(
            Cli::try_parse_from(["safeselect", "config", action, "--literal-password"]).is_ok()
        );
    }
}
