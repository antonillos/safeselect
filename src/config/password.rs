use crate::error::{Result, SafeselectError};

/// Configuration input only: never expand an imported credential or a TOML string.
pub enum PasswordInput {
    Environment(String),
    Literal(String),
}

impl PasswordInput {
    pub fn parse(value: String) -> Result<Self> {
        if let Some(reference) = value.strip_prefix("{env:") {
            let variable = reference.strip_suffix('}').ok_or_else(|| {
                SafeselectError::Secret("Expected a complete {env:NAME} password reference".into())
            })?;
            validate_variable(variable)?;
            return Ok(Self::Environment(variable.to_string()));
        }
        if value.starts_with("{file:") {
            return Err(SafeselectError::Secret(
                "File password references are not supported; use {env:NAME}".into(),
            ));
        }
        Self::literal(value)
    }

    pub fn literal(value: String) -> Result<Self> {
        if value.is_empty() {
            return Err(SafeselectError::Secret("Password must not be empty".into()));
        }
        Ok(Self::Literal(value))
    }
}

pub fn validate_variable(variable: &str) -> Result<()> {
    let mut chars = variable.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(SafeselectError::Secret(
            "Password environment variable must be a non-empty shell variable name".into(),
        ));
    }
    Ok(())
}

pub fn variable_input(value: &str) -> Result<String> {
    let value = value.trim();
    let variable = if value.starts_with("{env:") {
        match PasswordInput::parse(value.to_string())? {
            PasswordInput::Environment(variable) => variable,
            PasswordInput::Literal(_) => unreachable!(),
        }
    } else {
        value.to_string()
    };
    validate_variable(&variable)?;
    Ok(variable)
}

#[cfg(test)]
mod tests {
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
}
