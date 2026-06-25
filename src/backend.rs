use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    #[default]
    Jdbc,
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendCapability {
    SqlQuery,
    SqlExplain,
    TableDiscovery,
    DatabaseDiscovery,
    CollectionDiscovery,
    DocumentFind,
}

#[derive(Debug, Clone)]
pub struct BackendDescriptor {
    pub kind: BackendKind,
    pub vendor: String,
    pub capabilities: Vec<BackendCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentFindRequest {
    pub database: String,
    pub collection: String,
    pub filter: serde_json::Value,
    #[serde(default)]
    pub projection: Option<serde_json::Value>,
    #[serde(default)]
    pub sort: Option<serde_json::Value>,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentResult {
    pub documents: Vec<serde_json::Value>,
    pub document_count: u64,
    pub byte_count: u64,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub elapsed: String,
}

impl BackendDescriptor {
    pub fn jdbc(vendor: impl Into<String>) -> Self {
        Self {
            kind: BackendKind::Jdbc,
            vendor: vendor.into(),
            capabilities: vec![
                BackendCapability::SqlQuery,
                BackendCapability::SqlExplain,
                BackendCapability::TableDiscovery,
            ],
        }
    }

    pub fn document(vendor: impl Into<String>) -> Self {
        Self {
            kind: BackendKind::Document,
            vendor: vendor.into(),
            capabilities: vec![
                BackendCapability::DatabaseDiscovery,
                BackendCapability::CollectionDiscovery,
                BackendCapability::DocumentFind,
            ],
        }
    }

    pub fn has(&self, capability: BackendCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}
