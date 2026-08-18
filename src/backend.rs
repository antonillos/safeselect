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
    TableIndexes,
    DatabaseStats,
    TableStats,
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
    DocumentIndexes,
    DocumentDatabaseStats,
    DocumentCollectionStats,
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
        let vendor = vendor.into();
        let mut capabilities = vec![
            BackendCapability::SqlQuery,
            BackendCapability::SqlExplain,
            BackendCapability::TableDiscovery,
        ];
        if vendor.eq_ignore_ascii_case("postgresql") || vendor.eq_ignore_ascii_case("postgres") {
            capabilities.extend([
                BackendCapability::TableIndexes,
                BackendCapability::DatabaseStats,
                BackendCapability::TableStats,
            ]);
        }
        Self {
            kind: BackendKind::Jdbc,
            vendor,
            capabilities,
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
                BackendCapability::DocumentIndexes,
                BackendCapability::DocumentDatabaseStats,
                BackendCapability::DocumentCollectionStats,
            ],
        }
    }

    pub fn has(&self, capability: BackendCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_backend_advertises_indexes_and_bounded_stats() {
        let backend = BackendDescriptor::document("mongodb");

        assert!(backend.has(BackendCapability::DocumentIndexes));
        assert!(backend.has(BackendCapability::DocumentDatabaseStats));
        assert!(backend.has(BackendCapability::DocumentCollectionStats));
    }

    #[test]
    fn only_postgres_advertises_postgres_catalog_tools() {
        let postgres = BackendDescriptor::jdbc("postgresql");
        let mysql = BackendDescriptor::jdbc("mysql");

        assert!(postgres.has(BackendCapability::TableIndexes));
        assert!(postgres.has(BackendCapability::DatabaseStats));
        assert!(postgres.has(BackendCapability::TableStats));
        assert!(!mysql.has(BackendCapability::TableIndexes));
        assert!(!mysql.has(BackendCapability::DatabaseStats));
        assert!(!mysql.has(BackendCapability::TableStats));
    }
}
