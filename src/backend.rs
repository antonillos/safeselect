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
    DocumentAggregate,
    DocumentDistinct,
    DocumentCount,
    DocumentExplain,
    DocumentProfile,
    DocumentSchema,
    DocumentFixture,
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
pub struct DocumentCollectionRequest {
    pub database: String,
    pub collection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentAggregateRequest {
    pub database: String,
    pub collection: String,
    pub pipeline: serde_json::Value,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentDistinctRequest {
    pub database: String,
    pub collection: String,
    pub field: String,
    #[serde(default)]
    pub filter: serde_json::Value,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentCountRequest {
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentExplainRequest {
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: serde_json::Value,
    #[serde(default)]
    pub projection: Option<serde_json::Value>,
    #[serde(default)]
    pub sort: Option<serde_json::Value>,
    #[serde(default)]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentFieldProfileRequest {
    pub database: String,
    pub collection: String,
    pub field: String,
    #[serde(default)]
    pub filter: serde_json::Value,
    pub sample_size: u64,
    pub examples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSchemaRequest {
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: serde_json::Value,
    pub sample_size: u64,
    pub examples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentFixtureRequest {
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: serde_json::Value,
    #[serde(default)]
    pub projection: Option<serde_json::Value>,
    pub limit: u64,
    #[serde(default)]
    pub redact_fields: Vec<String>,
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
                BackendCapability::DocumentAggregate,
                BackendCapability::DocumentDistinct,
                BackendCapability::DocumentCount,
                BackendCapability::DocumentExplain,
                BackendCapability::DocumentProfile,
                BackendCapability::DocumentSchema,
                BackendCapability::DocumentFixture,
            ],
        }
    }

    pub fn has(&self, capability: BackendCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}
