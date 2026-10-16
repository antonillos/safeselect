use crate::error::{Result, SafeselectError};
use serde::Deserialize;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Default, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub mcp: McpConfig,
}

#[derive(Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval_seconds: u64,
}

fn default_health_check_interval() -> u64 {
    120
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            health_check_interval_seconds: default_health_check_interval(),
        }
    }
}

impl GlobalConfig {
    pub fn load(directory: &Path) -> Result<Self> {
        let content = match std::fs::read_to_string(directory.join("config.toml")) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => {
                return Err(SafeselectError::Config(
                    "Cannot read global config.toml".into(),
                ))
            }
        };
        // Do not echo TOML input: global settings may contain private values.
        let config: Self = toml::from_str(&content).map_err(|_| {
            SafeselectError::Config("Invalid global config.toml; mcp.health_check_interval_seconds must be a positive integer in seconds".into())
        })?;
        config.health_check_interval()?;
        Ok(config)
    }

    pub fn health_check_interval(&self) -> Result<Duration> {
        let seconds = self.mcp.health_check_interval_seconds;
        let interval = Duration::from_secs(seconds);
        if seconds == 0 || Instant::now().checked_add(interval).is_none() {
            return Err(SafeselectError::Config(
                "Invalid global mcp.health_check_interval_seconds: expected positive seconds that fit the timer".into(),
            ));
        }
        Ok(interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_global_interval_and_rejects_invalid_values_without_echoing_input() {
        let root = std::env::temp_dir().join(format!("safeselect-global-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            GlobalConfig::load(&root)
                .unwrap()
                .health_check_interval()
                .unwrap(),
            Duration::from_secs(120)
        );
        for content in ["", "[mcp]", "[mcp]\nhealth_check_interval_seconds = 17"] {
            std::fs::write(root.join("config.toml"), content).unwrap();
            let expected = if content.contains("17") { 17 } else { 120 };
            assert_eq!(
                GlobalConfig::load(&root)
                    .unwrap()
                    .health_check_interval()
                    .unwrap(),
                Duration::from_secs(expected)
            );
        }
        for value in [
            "0",
            "-1",
            "1.5",
            "'private-value'",
            "18446744073709551615",
            "[",
        ] {
            std::fs::write(
                root.join("config.toml"),
                format!("[mcp]\nhealth_check_interval_seconds = {value}"),
            )
            .unwrap();
            let error = GlobalConfig::load(&root).err().unwrap().to_string();
            assert!(error.contains("health_check_interval_seconds"));
            assert!(!error.contains("private-value"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
