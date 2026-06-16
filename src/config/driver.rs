use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverConfig {
    pub version: u32,
    pub vendor: String,
    pub path: String,
    pub class: String,
    pub sha256: String,
}
