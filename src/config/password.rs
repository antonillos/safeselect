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

pub fn inject_mongodb_password_placeholder(url: &str, username: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    let tail = &url[authority_start..];
    let authority_end = tail.find(['/', '?', '#']).unwrap_or(tail.len());
    let authority = &tail[..authority_end];
    let (encoded_username, host) = match authority.rsplit_once('@') {
        Some((userinfo, host)) => (
            userinfo
                .split_once(':')
                .map_or(userinfo, |(name, _)| name)
                .to_string(),
            host,
        ),
        None => (encode_mongodb_username(username), authority),
    };
    format!(
        "{}{}:__SAFESELECT_PASSWORD__@{}{}",
        &url[..authority_start],
        encoded_username,
        host,
        &tail[authority_end..]
    )
}

fn encode_mongodb_username(username: &str) -> String {
    username
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "password_input_tests.rs"]
mod tests;
