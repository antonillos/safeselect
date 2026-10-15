use serde::Deserialize;
use std::fmt;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionCategory {
    Authentication,
    Driver,
    Tls,
    Timeout,
    Network,
    Database,
}

#[derive(Debug, Deserialize)]
pub struct ConnectionFailure {
    category: ConnectionCategory,
    sql_state: Option<String>,
}

impl ConnectionFailure {
    pub fn from_startup_ack(ack: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct StartupFailure {
            r#type: String,
            #[serde(flatten)]
            failure: ConnectionFailure,
        }
        let response: StartupFailure = serde_json::from_str(ack).ok()?;
        let valid_state = response.failure.sql_state.as_ref().is_none_or(|state| {
            state.len() == 5
                && state
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        });
        (response.r#type == "connection_error" && valid_state).then_some(response.failure)
    }
}

impl ConnectionCategory {
    fn summary(&self) -> &'static str {
        match self {
            ConnectionCategory::Authentication => {
                "Database authentication failed. Check the database credential source"
            }
            ConnectionCategory::Driver => {
                "Database driver could not be loaded or does not support the configured URL"
            }
            ConnectionCategory::Tls => {
                "Database TLS negotiation failed. Check TLS configuration and trust"
            }
            ConnectionCategory::Timeout => "Database connection timed out",
            ConnectionCategory::Network => {
                "Database network connection failed. Check the endpoint and SSH tunnel"
            }
            ConnectionCategory::Database => "Database connection failed",
        }
    }
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.category.summary())?;
        if let Some(state) = &self.sql_state {
            write!(f, " (SQLSTATE {state})")?;
        }
        write!(f, ".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_only_known_categories_and_valid_sqlstates() {
        for (category, expected) in [
            ("authentication", "authentication failed"),
            ("driver", "driver could not be loaded"),
            ("tls", "TLS negotiation failed"),
            ("timeout", "connection timed out"),
            ("network", "network connection failed"),
            ("database", "Database connection failed"),
        ] {
            let ack = serde_json::json!({
                "type": "connection_error", "category": category,
                "sql_state": "08001", "message": "private-detail"
            });
            let message = ConnectionFailure::from_startup_ack(&ack.to_string())
                .unwrap()
                .to_string();
            assert!(message.contains(expected));
            assert!(message.contains("SQLSTATE 08001"));
            assert!(!message.contains("private-detail"));
        }
    }
}
