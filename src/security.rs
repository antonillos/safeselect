use crate::backend::{
    DocumentAggregateRequest, DocumentCollectionRequest, DocumentCountRequest,
    DocumentDistinctRequest, DocumentExplainRequest, DocumentFieldProfileRequest,
    DocumentFindRequest, DocumentFixtureRequest, DocumentSchemaRequest,
};
use crate::config::{LimitsConfig, SecurityPolicy};
use crate::error::{Result, SafeselectError};

const MAX_SQL_BYTES: usize = 102_400;
const FORBIDDEN_MQL_JAVASCRIPT_OPERATORS: &[&str] = &["$where", "$function", "$accumulator"];

pub struct SecurityEngine {
    policy: SecurityPolicy,
    limits: LimitsConfig,
}

fn forbidden_mql_javascript_operator(value: &serde_json::Value) -> Option<&'static str> {
    match value {
        serde_json::Value::Object(object) => object.iter().find_map(|(key, value)| {
            FORBIDDEN_MQL_JAVASCRIPT_OPERATORS
                .iter()
                .copied()
                .find(|operator| key == operator)
                .or_else(|| forbidden_mql_javascript_operator(value))
        }),
        serde_json::Value::Array(values) => {
            values.iter().find_map(forbidden_mql_javascript_operator)
        }
        _ => None,
    }
}

impl SecurityEngine {
    pub fn new(policy: SecurityPolicy, limits: LimitsConfig) -> Self {
        Self { policy, limits }
    }

    pub fn limits(&self) -> &LimitsConfig {
        &self.limits
    }

    pub fn allowed_schemas(&self) -> &[String] {
        &self.policy.allowed_schemas
    }

    pub fn validate_relation_access(&self, schema: &str, relation: &str) -> Result<()> {
        if !self.policy.allowed_schemas.is_empty()
            && !self
                .policy
                .allowed_schemas
                .iter()
                .any(|allowed| allowed == schema)
        {
            return Err(SafeselectError::QueryRejected(format!(
                "Schema '{schema}' is not in the allowed schemas list ({})",
                self.policy.allowed_schemas.join(", ")
            )));
        }

        let qualified = format!("{schema}.{relation}");
        if self.policy.denied_relations.iter().any(|denied| {
            denied.eq_ignore_ascii_case(relation) || denied.eq_ignore_ascii_case(&qualified)
        }) {
            return Err(SafeselectError::QueryRejected(format!(
                "Relation '{qualified}' is denied"
            )));
        }

        Ok(())
    }

    pub fn filter_document_databases(&self, databases: Vec<String>) -> Vec<String> {
        if self.policy.allowed_databases.is_empty() {
            return databases;
        }
        databases
            .into_iter()
            .filter(|database| self.policy.allowed_databases.iter().any(|d| d == database))
            .collect()
    }

    pub fn filter_document_collections(
        &self,
        database: &str,
        collections: Vec<String>,
    ) -> Vec<String> {
        let namespace = |collection: &str| format!("{database}.{collection}");
        collections
            .into_iter()
            .filter(|collection| {
                let ns = namespace(collection);
                let allowed = self.policy.allowed_collections.is_empty()
                    || self
                        .policy
                        .allowed_collections
                        .iter()
                        .any(|allowed| allowed == collection || allowed == &ns);
                let denied = self
                    .policy
                    .denied_collections
                    .iter()
                    .any(|denied| denied == collection || denied == &ns);
                allowed && !denied
            })
            .collect()
    }

    pub fn validate_document_find(&self, request: &DocumentFindRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;

        self.validate_document_find_filter(&request.filter)?;
        self.validate_document_find_options(request)?;
        self.validate_document_mql(&request.filter)?;
        if let Some(projection) = &request.projection {
            self.validate_document_mql(projection)?;
        }
        if let Some(sort) = &request.sort {
            self.validate_document_mql(sort)?;
        }

        Ok(())
    }

    fn validate_document_find_filter(&self, filter: &serde_json::Value) -> Result<()> {
        if !filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Document filter must be a JSON object".into(),
            ));
        }
        Ok(())
    }

    fn validate_document_find_options(&self, request: &DocumentFindRequest) -> Result<()> {
        if request
            .projection
            .as_ref()
            .is_some_and(|projection| !projection.is_object())
        {
            return Err(SafeselectError::QueryRejected(
                "Document projection must be a JSON object".into(),
            ));
        }
        if request.sort.as_ref().is_some_and(|sort| !sort.is_object()) {
            return Err(SafeselectError::QueryRejected(
                "Document sort must be a JSON object".into(),
            ));
        }
        if request.limit == 0 || request.limit > self.limits.max_rows {
            return Err(SafeselectError::QueryRejected(format!(
                "Document limit must be between 1 and {}",
                self.limits.max_rows
            )));
        }
        Ok(())
    }

    pub fn validate_document_collection(&self, request: &DocumentCollectionRequest) -> Result<()> {
        self.validate_document_database(&request.database)?;
        self.check_document_name("collection", &request.collection)?;

        let namespace = format!("{}.{}", request.database, request.collection);
        if !self.policy.allowed_collections.is_empty()
            && !self
                .policy
                .allowed_collections
                .iter()
                .any(|collection| collection == &request.collection || collection == &namespace)
        {
            return Err(SafeselectError::QueryRejected(format!(
                "Collection '{}' is not in the allowed collections list ({})",
                namespace,
                self.policy.allowed_collections.join(", ")
            )));
        }

        if self
            .policy
            .denied_collections
            .iter()
            .any(|collection| collection == &request.collection || collection == &namespace)
        {
            return Err(SafeselectError::QueryRejected(format!(
                "Collection '{namespace}' is denied"
            )));
        }

        Ok(())
    }

    pub fn validate_document_aggregate(&self, request: &DocumentAggregateRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        if !request.pipeline.is_array() {
            return Err(SafeselectError::QueryRejected(
                "Aggregation pipeline must be a JSON array of stage objects. Correct the pipeline argument before retrying; do not repeat the same call. Example: [{\"$match\":{\"active\":true}}]".into(),
            ));
        }
        if request.limit == 0 || request.limit > self.limits.max_rows {
            return Err(SafeselectError::QueryRejected(format!(
                "Aggregation limit must be between 1 and {}",
                self.limits.max_rows
            )));
        }
        for stage in request.pipeline.as_array().into_iter().flatten() {
            self.validate_aggregate_stage(stage)?;
        }
        Ok(())
    }

    fn validate_aggregate_stage(&self, stage: &serde_json::Value) -> Result<()> {
        let Some(stage_object) = stage.as_object() else {
            return Err(SafeselectError::QueryRejected(
                "Aggregation stages must be JSON objects. Correct the pipeline argument before retrying; do not repeat the same call. Example: [{\"$match\":{\"active\":true}}]".into(),
            ));
        };
        if stage_object.len() != 1 {
            return Err(SafeselectError::QueryRejected(
                "Aggregation stages must contain exactly one operator. Correct the pipeline argument before retrying; do not repeat the same call. Example: [{\"$match\":{\"active\":true}}]".into(),
            ));
        }
        if let Some(name) = stage_object
            .keys()
            .find(|name| matches!(name.as_str(), "$out" | "$merge" | "$currentOp"))
        {
            return Err(SafeselectError::QueryRejected(format!(
                "Aggregation stage '{name}' is not read-only"
            )));
        }
        self.validate_document_mql(stage)
    }

    pub fn validate_document_distinct(&self, request: &DocumentDistinctRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        self.check_document_field(&request.field)?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Distinct filter must be a JSON object".into(),
            ));
        }
        if request.limit == 0 || request.limit > self.limits.max_rows {
            return Err(SafeselectError::QueryRejected(format!(
                "Distinct limit must be between 1 and {}",
                self.limits.max_rows
            )));
        }
        self.validate_document_mql(&request.filter)?;
        Ok(())
    }

    pub fn validate_document_count(&self, request: &DocumentCountRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Count filter must be a JSON object".into(),
            ));
        }
        if request
            .filter
            .as_object()
            .is_some_and(|filter| filter.is_empty())
        {
            return Err(SafeselectError::QueryRejected(
                "Count filter must not be empty; full collection counts are rejected".into(),
            ));
        }
        self.validate_document_mql(&request.filter)?;
        Ok(())
    }

    pub fn validate_document_explain(&self, request: &DocumentExplainRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Explain filter must be a JSON object".into(),
            ));
        }
        if request
            .projection
            .as_ref()
            .is_some_and(|projection| !projection.is_object())
        {
            return Err(SafeselectError::QueryRejected(
                "Explain projection must be a JSON object".into(),
            ));
        }
        if request.sort.as_ref().is_some_and(|sort| !sort.is_object()) {
            return Err(SafeselectError::QueryRejected(
                "Explain sort must be a JSON object".into(),
            ));
        }
        self.validate_document_mql(&request.filter)?;
        if let Some(projection) = &request.projection {
            self.validate_document_mql(projection)?;
        }
        if let Some(sort) = &request.sort {
            self.validate_document_mql(sort)?;
        }
        Ok(())
    }

    pub fn validate_document_field_profile(
        &self,
        request: &DocumentFieldProfileRequest,
    ) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        self.check_document_field(&request.field)?;
        self.check_document_sample_size(request.sample_size, "Profile sample size")?;
        self.check_document_sample_size(request.examples, "Profile examples")?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Profile filter must be a JSON object".into(),
            ));
        }
        self.validate_document_mql(&request.filter)?;
        Ok(())
    }

    pub fn validate_document_schema(&self, request: &DocumentSchemaRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        self.check_document_sample_size(request.sample_size, "Schema sample size")?;
        self.check_document_sample_size(request.examples, "Schema examples")?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Schema filter must be a JSON object".into(),
            ));
        }
        self.validate_document_mql(&request.filter)?;
        Ok(())
    }

    pub fn validate_document_fixture(&self, request: &DocumentFixtureRequest) -> Result<()> {
        self.validate_document_collection(&DocumentCollectionRequest {
            database: request.database.clone(),
            collection: request.collection.clone(),
        })?;
        if !request.filter.is_object() {
            return Err(SafeselectError::QueryRejected(
                "Fixture filter must be a JSON object".into(),
            ));
        }
        if request
            .projection
            .as_ref()
            .is_some_and(|projection| !projection.is_object())
        {
            return Err(SafeselectError::QueryRejected(
                "Fixture projection must be a JSON object".into(),
            ));
        }
        if request.limit == 0 || request.limit > self.limits.max_rows {
            return Err(SafeselectError::QueryRejected(format!(
                "Fixture limit must be between 1 and {}",
                self.limits.max_rows
            )));
        }
        self.validate_document_mql(&request.filter)?;
        if let Some(projection) = &request.projection {
            self.validate_document_mql(projection)?;
        }
        for field in &request.redact_fields {
            self.check_document_field(field)?;
        }
        Ok(())
    }

    fn validate_document_mql(&self, value: &serde_json::Value) -> Result<()> {
        if let Some(operator) = forbidden_mql_javascript_operator(value) {
            return Err(SafeselectError::QueryRejected(format!(
                "MongoDB server-side JavaScript operator '{operator}' is not allowed; rebuild the query using declarative MQL operators"
            )));
        }
        Ok(())
    }

    pub fn validate_document_database(&self, database: &str) -> Result<()> {
        self.check_document_name("database", database)?;

        if !self.policy.allowed_databases.is_empty()
            && !self
                .policy
                .allowed_databases
                .iter()
                .any(|allowed| allowed == database)
        {
            return Err(SafeselectError::QueryRejected(format!(
                "Database '{}' is not in the allowed databases list ({})",
                database,
                self.policy.allowed_databases.join(", ")
            )));
        }

        Ok(())
    }

    pub fn validate(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim();

        if trimmed.is_empty() {
            return Err(SafeselectError::QueryRejected("Empty query".into()));
        }

        if trimmed.len() > MAX_SQL_BYTES {
            return Err(SafeselectError::QueryRejected(format!(
                "Query exceeds maximum size ({} bytes)",
                MAX_SQL_BYTES
            )));
        }

        self.validate_policy_constraints(trimmed)?;
        self.check_read_only(trimmed)
    }

    fn validate_policy_constraints(&self, query: &str) -> Result<()> {
        if self.policy.require_single_statement {
            self.check_single_statement(query)?;
        }
        if !self.policy.allowed_schemas.is_empty() {
            self.check_allowed_schemas(query)?;
        }
        if !self.policy.denied_relations.is_empty() {
            self.check_denied_relations(query)?;
        }
        Ok(())
    }

    /// Like `validate` but skips schema allowlist checking.
    /// Use for tool-generated queries (e.g. `list_tables`) that
    /// reference system catalogs like `information_schema`.
    pub fn validate_system(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim();

        if trimmed.is_empty() {
            return Err(SafeselectError::QueryRejected("Empty query".into()));
        }

        if trimmed.len() > MAX_SQL_BYTES {
            return Err(SafeselectError::QueryRejected(format!(
                "Query exceeds maximum size ({} bytes)",
                MAX_SQL_BYTES
            )));
        }

        if self.policy.require_single_statement {
            self.check_single_statement(trimmed)?;
        }

        self.check_read_only(trimmed)?;

        if !self.policy.denied_relations.is_empty() {
            self.check_denied_relations(trimmed)?;
        }

        Ok(())
    }

    fn check_single_statement(&self, sql: &str) -> Result<()> {
        let clean = strip_trailing_semicolons(sql);
        let count = count_statements(clean);
        if count != 1 {
            return Err(SafeselectError::QueryRejected(format!(
                "Single statement required, detected {count} statements"
            )));
        }
        Ok(())
    }

    fn strip_sql_comments(sql: &str) -> String {
        let mut result = String::with_capacity(sql.len());
        let mut chars = sql.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '-' && chars.peek() == Some(&'-') {
                // Line comment: skip until newline
                for c in chars.by_ref() {
                    if c == '\n' {
                        result.push('\n');
                        break;
                    }
                }
            } else if ch == '/' && chars.peek() == Some(&'*') {
                // Block comment: skip until */
                chars.next(); // consume '*'
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    fn check_read_only(&self, sql: &str) -> Result<()> {
        let stripped = Self::strip_sql_comments(sql);
        let trimmed = stripped.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("WITH") {
            return self.check_with_read_only(trimmed);
        }

        if upper.starts_with("EXPLAIN") {
            return self.check_explain_read_only(trimmed);
        }

        if upper.starts_with("SELECT") {
            self.check_forbidden_tokens(trimmed, false)?;
            return Ok(());
        }

        let disallowed = [
            "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "COPY", "SET ",
            "PREPARE", "EXECUTE", "CALL", "MERGE", "REPLACE", "GRANT", "REVOKE",
        ];

        for kw in &disallowed {
            if upper.starts_with(kw) {
                return Err(SafeselectError::QueryRejected(format!(
                    "Read-only mode: {} not allowed",
                    kw.trim()
                )));
            }
        }

        Err(SafeselectError::QueryRejected(
            "Read-only mode: unrecognized statement type".into(),
        ))
    }

    fn check_explain_read_only(&self, sql: &str) -> Result<()> {
        let explained_sql = extract_explain_target(sql).ok_or_else(|| {
            SafeselectError::QueryRejected(
                "Read-only mode: could not validate EXPLAIN target statement".into(),
            )
        })?;

        self.check_select_like_read_only(explained_sql)
            .map_err(|_| {
                SafeselectError::QueryRejected(
                    "Read-only mode: EXPLAIN is only allowed for SELECT statements".into(),
                )
            })
    }

    fn check_with_read_only(&self, sql: &str) -> Result<()> {
        self.check_select_like_read_only(sql)
    }

    fn check_select_like_read_only(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim_start();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("SELECT") {
            self.check_forbidden_tokens(trimmed, false)?;
            return Ok(());
        }

        if upper.starts_with("WITH") {
            let body = extract_with_query_body(trimmed).ok_or_else(|| {
                SafeselectError::QueryRejected(
                    "Read-only mode: could not validate WITH query".into(),
                )
            })?;

            if !body.trim_start().to_uppercase().starts_with("SELECT") {
                return Err(SafeselectError::QueryRejected(
                    "Read-only mode: WITH queries must end in SELECT".into(),
                ));
            }

            self.check_forbidden_tokens(trimmed, true)?;
            return Ok(());
        }

        Err(SafeselectError::QueryRejected(
            "Read-only mode: unrecognized statement type".into(),
        ))
    }

    fn check_forbidden_tokens(&self, sql: &str, allow_with_keyword: bool) -> Result<()> {
        let compact = sanitize_for_keyword_scan(sql);
        let forbidden = [
            "INSERT", "UPDATE", "DELETE", "DROP", "CREATE", "ALTER", "TRUNCATE", "COPY", "PREPARE",
            "EXECUTE", "CALL", "MERGE", "REPLACE", "GRANT", "REVOKE", "WITH", "DO", "DECLARE",
            "LOCK", "VACUUM", "REINDEX",
        ];

        for keyword in forbidden {
            if keyword == "WITH" && allow_with_keyword {
                continue;
            }
            if keyword == "WITH" && contains_with_ordinality_only(&compact) {
                continue;
            }
            if contains_keyword(&compact, keyword) {
                return Err(SafeselectError::QueryRejected(format!(
                    "Read-only mode: {keyword} not allowed"
                )));
            }
        }

        let forbidden_functions = [
            "SET_CONFIG",
            "PG_SLEEP",
            "PG_ADVISORY_LOCK",
            "PG_ADVISORY_XACT_LOCK",
            "PG_CREATE_PHYSICAL_REPLICATION_SLOT",
            "PG_CREATE_LOGICAL_REPLICATION_SLOT",
            "PG_DROP_REPLICATION_SLOT",
            "PG_TERMINATE_BACKEND",
            "PG_CANCEL_BACKEND",
            "PG_RELOAD_CONF",
            "PG_ROTATE_LOGFILE",
            "PG_START_BACKUP",
            "PG_STOP_BACKUP",
            "LO_IMPORT",
            "LO_EXPORT",
            "LO_UNLINK",
            "NEXTVAL",
        ];

        for function in forbidden_functions {
            if compact.contains(function) {
                return Err(SafeselectError::QueryRejected(format!(
                    "Read-only mode: function {function} not allowed"
                )));
            }
        }

        if contains_keyword(&compact, "SET") || compact.contains("SETROLE") {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: session changes are not allowed".into(),
            ));
        }

        Ok(())
    }

    fn check_allowed_schemas(&self, sql: &str) -> Result<()> {
        let sql_lower = sql.to_lowercase();
        let schema_patterns: Vec<String> = self
            .policy
            .allowed_schemas
            .iter()
            .map(|s| format!("{}.", s.to_lowercase()))
            .collect();

        let has_allowed = schema_patterns
            .iter()
            .any(|p| sql_lower.contains(p.as_str()));

        if has_allowed {
            return Ok(());
        }

        let has_unknown = has_schema_reference(&sql_lower, &schema_patterns);
        if has_unknown {
            return Err(SafeselectError::QueryRejected(format!(
                "Query references a schema not in allowed list ({})",
                self.policy.allowed_schemas.join(", ")
            )));
        }

        Ok(())
    }

    fn check_denied_relations(&self, sql: &str) -> Result<()> {
        let sql_lower = sql.to_lowercase();
        for relation in &self.policy.denied_relations {
            let rel_lower = relation.to_lowercase();
            if sql_lower.contains(&rel_lower) {
                return Err(SafeselectError::QueryRejected(format!(
                    "Query references denied relation: {relation}"
                )));
            }
        }
        Ok(())
    }

    fn check_document_name(&self, kind: &str, name: &str) -> Result<()> {
        let invalid = [
            name.is_empty(),
            name.len() > 255,
            name.starts_with("system."),
            name.contains('\0'),
            name.contains('$'),
            name.contains('/'),
            name.contains('\\'),
            name.contains(' '),
        ]
        .into_iter()
        .any(std::convert::identity);
        if invalid {
            return Err(SafeselectError::QueryRejected(format!(
                "Invalid document {kind} name: {name}"
            )));
        }
        Ok(())
    }

    fn check_document_field(&self, field: &str) -> Result<()> {
        let invalid = [
            field.is_empty(),
            field.len() > 512,
            field.contains('\0'),
            field.contains('$'),
            field.contains(' '),
            field.starts_with('.'),
            field.ends_with('.'),
            field.split('.').any(str::is_empty),
        ]
        .into_iter()
        .any(std::convert::identity);
        if invalid {
            return Err(SafeselectError::QueryRejected(format!(
                "Invalid document field path: {field}"
            )));
        }
        Ok(())
    }

    fn check_document_sample_size(&self, value: u64, label: &str) -> Result<()> {
        if value == 0 || value > self.limits.max_rows {
            return Err(SafeselectError::QueryRejected(format!(
                "{label} must be between 1 and {}",
                self.limits.max_rows
            )));
        }
        Ok(())
    }

    pub fn check_result_size(&self, row_count: u64, byte_count: u64) -> Result<()> {
        if row_count > self.limits.max_rows {
            return Err(SafeselectError::LimitExceeded(format!(
                "Result has {row_count} rows, limit is {}",
                self.limits.max_rows
            )));
        }
        if byte_count > self.limits.max_result_bytes {
            return Err(SafeselectError::LimitExceeded(format!(
                "Result is {byte_count} bytes, limit is {}",
                self.limits.max_result_bytes
            )));
        }
        Ok(())
    }
}

fn has_schema_reference(sql_lower: &str, allowed_patterns: &[String]) -> bool {
    let bytes = sql_lower.as_bytes();
    for i in 0..bytes.len().saturating_sub(2) {
        if bytes[i + 1] == b'.' && bytes[i].is_ascii_alphabetic() {
            let start = i;
            let mut end = i + 2;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            let schema = &sql_lower[start..end];
            let schemaname = schema.trim_end_matches('.');
            if !schemaname.is_empty()
                && schemaname
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !is_sql_keyword(schemaname)
                && !allowed_patterns.iter().any(|p| p.starts_with(schemaname))
            {
                return true;
            }
        }
    }
    false
}

fn is_sql_keyword(word: &str) -> bool {
    matches!(
        word,
        "select"
            | "from"
            | "where"
            | "and"
            | "or"
            | "not"
            | "in"
            | "on"
            | "as"
            | "join"
            | "left"
            | "right"
            | "inner"
            | "outer"
            | "cross"
            | "full"
            | "order"
            | "group"
            | "by"
            | "having"
            | "limit"
            | "offset"
            | "insert"
            | "update"
            | "delete"
            | "into"
            | "values"
            | "set"
            | "create"
            | "alter"
            | "drop"
            | "table"
            | "index"
            | "view"
            | "distinct"
            | "count"
            | "sum"
            | "avg"
            | "min"
            | "max"
            | "exists"
            | "true"
            | "false"
            | "null"
            | "is"
            | "like"
            | "between"
            | "union"
            | "all"
            | "any"
            | "some"
            | "case"
            | "when"
            | "then"
            | "else"
            | "end"
            | "cast"
            | "coalesce"
            | "nullif"
            | "begin"
            | "commit"
            | "rollback"
            | "grant"
            | "revoke"
    )
}

fn strip_trailing_semicolons(sql: &str) -> &str {
    let trimmed = sql.trim();
    if trimmed.ends_with(';') {
        let stripped = trimmed.trim_end_matches(';');
        let stripped = stripped.trim();
        if stripped.is_empty() {
            trimmed
        } else {
            stripped
        }
    } else {
        trimmed
    }
}

fn count_statements(sql: &str) -> usize {
    let sql = sql.trim();
    if sql.is_empty() {
        return 0;
    }

    let mut count = 0;
    let mut in_string = false;
    let mut string_char = ' ';
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut i = 0;
    let chars: Vec<char> = sql.chars().collect();

    while i < chars.len() {
        let c = chars[i];

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_block_comment {
            if c == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }

        if in_string {
            if c == '\\' && i + 1 < chars.len() {
                i += 2;
                continue;
            }
            if c == string_char {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '\'' || c == '"' || c == '$' {
            in_string = true;
            string_char = c;
            i += 1;
            continue;
        }

        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            in_line_comment = true;
            i += 2;
            continue;
        }

        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            in_block_comment = true;
            i += 2;
            continue;
        }

        if c == ';' {
            count += 1;
        }

        i += 1;
    }

    count + 1
}

fn sanitize_for_keyword_scan(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < chars.len() {
        let c = chars[i];

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(' ');
            }
            i += 1;
            continue;
        }

        if in_block_comment {
            if c == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                in_block_comment = false;
                i += 2;
                out.push(' ');
                continue;
            }
            i += 1;
            continue;
        }

        if in_single {
            if c == '\'' {
                if i + 1 < chars.len() && chars[i + 1] == '\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }

        if in_double {
            if c == '"' {
                in_double = false;
            }
            i += 1;
            continue;
        }

        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            in_line_comment = true;
            i += 2;
            continue;
        }

        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            in_block_comment = true;
            i += 2;
            continue;
        }

        if c == '\'' {
            in_single = true;
            out.push(' ');
            i += 1;
            continue;
        }

        if c == '"' {
            in_double = true;
            out.push(' ');
            i += 1;
            continue;
        }

        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(' ');
        }

        i += 1;
    }

    out
}

fn contains_keyword(sql: &str, keyword: &str) -> bool {
    sql.split_whitespace().any(|token| token == keyword)
}

fn contains_with_ordinality_only(sql: &str) -> bool {
    let mut saw_with = false;
    let mut tokens = sql.split_whitespace().peekable();

    while let Some(token) = tokens.next() {
        if token != "WITH" {
            continue;
        }

        saw_with = true;
        if tokens.next() != Some("ORDINALITY") {
            return false;
        }
    }

    saw_with
}

fn extract_with_query_body(sql: &str) -> Option<&str> {
    let upper = sql.to_uppercase();
    if !upper.starts_with("WITH") {
        return None;
    }

    let mut i = 4;
    i = skip_sql_whitespace(sql, i);

    if starts_with_keyword_at(sql, i, "RECURSIVE") {
        i += "RECURSIVE".len();
        i = skip_sql_whitespace(sql, i);
    }

    loop {
        let as_index = find_top_level_keyword(sql, i, "AS")?;
        i = skip_sql_whitespace(sql, as_index + 2);

        if starts_with_keyword_at(sql, i, "NOT") {
            i += 3;
            i = skip_sql_whitespace(sql, i);
        }
        if starts_with_keyword_at(sql, i, "MATERIALIZED") {
            i += "MATERIALIZED".len();
            i = skip_sql_whitespace(sql, i);
        }

        if sql.get(i..=i)? != "(" {
            return None;
        }

        i = skip_balanced_parentheses(sql, i)?;
        i = skip_sql_whitespace(sql, i);

        if sql.get(i..=i) == Some(",") {
            i += 1;
            i = skip_sql_whitespace(sql, i);
            continue;
        }

        return sql.get(i..);
    }
}

fn skip_sql_whitespace(sql: &str, mut index: usize) -> usize {
    while let Some(ch) = sql[index..].chars().next() {
        if !ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn starts_with_keyword_at(sql: &str, index: usize, keyword: &str) -> bool {
    let end = index + keyword.len();
    let Some(candidate) = sql.get(index..end) else {
        return false;
    };
    if !candidate.eq_ignore_ascii_case(keyword) {
        return false;
    }

    let prev_ok = index == 0
        || sql[..index]
            .chars()
            .next_back()
            .is_none_or(|ch| !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'));
    let next_ok = sql[end..]
        .chars()
        .next()
        .is_none_or(|ch| !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'));

    prev_ok && next_ok
}

fn find_top_level_keyword(sql: &str, start: usize, keyword: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = sql.char_indices().collect();
    let mut pos = chars.iter().position(|(idx, _)| *idx >= start)?;
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    while pos < chars.len() {
        let (byte_idx, ch) = chars[pos];

        if in_single {
            if ch == '\'' {
                if pos + 1 < chars.len() && chars[pos + 1].1 == '\'' {
                    pos += 2;
                    continue;
                }
                in_single = false;
            }
            pos += 1;
            continue;
        }

        if in_double {
            if ch == '"' {
                in_double = false;
            }
            pos += 1;
            continue;
        }

        if ch == '\'' {
            in_single = true;
            pos += 1;
            continue;
        }

        if ch == '"' {
            in_double = true;
            pos += 1;
            continue;
        }

        if ch == '-' && pos + 1 < chars.len() && chars[pos + 1].1 == '-' {
            pos += 2;
            while pos < chars.len() && chars[pos].1 != '\n' {
                pos += 1;
            }
            continue;
        }

        if ch == '/' && pos + 1 < chars.len() && chars[pos + 1].1 == '*' {
            pos += 2;
            while pos + 1 < chars.len() && !(chars[pos].1 == '*' && chars[pos + 1].1 == '/') {
                pos += 1;
            }
            pos = (pos + 2).min(chars.len());
            continue;
        }

        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }

        if depth == 0 && starts_with_keyword_at(sql, byte_idx, keyword) {
            return Some(byte_idx);
        }

        pos += 1;
    }

    None
}

fn skip_balanced_parentheses(sql: &str, start: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = sql.char_indices().collect();
    let mut pos = chars.iter().position(|(idx, _)| *idx == start)?;
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    while pos < chars.len() {
        let (_, ch) = chars[pos];

        if in_single {
            if ch == '\'' {
                if pos + 1 < chars.len() && chars[pos + 1].1 == '\'' {
                    pos += 2;
                    continue;
                }
                in_single = false;
            }
            pos += 1;
            continue;
        }

        if in_double {
            if ch == '"' {
                in_double = false;
            }
            pos += 1;
            continue;
        }

        if ch == '\'' {
            in_single = true;
            pos += 1;
            continue;
        }

        if ch == '"' {
            in_double = true;
            pos += 1;
            continue;
        }

        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                let next = pos + 1;
                return Some(if next < chars.len() {
                    chars[next].0
                } else {
                    sql.len()
                });
            }
        }

        pos += 1;
    }

    None
}

fn extract_explain_target(sql: &str) -> Option<&str> {
    let trimmed = sql.trim_start();
    let upper = trimmed.to_uppercase();
    if !upper.starts_with("EXPLAIN") {
        return None;
    }

    let after_explain = trimmed.get(7..)?.trim_start();
    if after_explain.starts_with('(') {
        let mut depth = 0usize;
        for (idx, ch) in after_explain.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return after_explain.get(idx + 1..).map(str::trim_start);
                    }
                }
                _ => {}
            }
        }
        return None;
    }

    let upper_after_explain = after_explain.to_uppercase();
    for option in [
        "ANALYZE", "VERBOSE", "BUFFERS", "SETTINGS", "WAL", "TIMING", "SUMMARY",
    ] {
        if upper_after_explain.starts_with(option) {
            return after_explain.get(option.len()..).map(str::trim_start);
        }
    }

    Some(after_explain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_statement_simple() {
        assert_eq!(count_statements("SELECT * FROM users"), 1);
    }

    #[test]
    fn test_single_statement_with_semicolon() {
        let sql = strip_trailing_semicolons("SELECT * FROM users;");
        assert_eq!(count_statements(sql), 1);
    }

    #[test]
    fn test_multiple_statements() {
        assert_eq!(count_statements("SELECT 1; SELECT 2"), 2);
    }

    #[test]
    fn test_stacked_query_variants_are_rejected_before_execution() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());

        for sql in [
            "COMMIT; DROP TABLE public.users",
            "ROLLBACK; CREATE TABLE public.evil_copy (id int)",
            "SELECT 1; DELETE FROM public.users",
            "/* harmless prefix */\nCoMmIt ;\nDrOp TABLE public.users",
            "WITH x AS (SELECT 1) SELECT * FROM x; DELETE FROM public.users",
            "DO $$ BEGIN PERFORM 1; END $$; DELETE FROM public.users",
        ] {
            let error = engine
                .validate(sql)
                .expect_err("stacked query must be rejected before sidecar execution");
            assert!(
                error.to_string().contains("Single statement required"),
                "unexpected rejection for {sql:?}: {error}"
            );
        }
    }

    #[test]
    fn test_semicolon_in_string() {
        assert_eq!(count_statements("SELECT 'hello;world'"), 1);
    }

    #[test]
    fn test_empty() {
        assert_eq!(count_statements(""), 0);
        assert_eq!(count_statements("   "), 0);
    }

    #[test]
    fn test_read_only_select() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "SELECT * FROM users";
        assert!(engine.check_read_only(sql).is_ok());
    }

    #[test]
    fn test_read_only_with_select_allowed() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "WITH x AS (SELECT 1 AS id) SELECT * FROM x";
        assert!(engine.check_read_only(sql).is_ok());
    }

    #[test]
    fn test_read_only_explain() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "EXPLAIN SELECT * FROM users";
        assert!(engine.check_read_only(sql).is_ok());
    }

    #[test]
    fn test_read_only_with_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("WITH x AS (SELECT 1) SELECT * FROM x")
            .is_ok());
    }

    #[test]
    fn test_read_only_select_with_ordinality_allowed() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("SELECT * FROM unnest(ARRAY[10, 20]) WITH ORDINALITY AS t(value, ord)")
            .is_ok());
    }

    #[test]
    fn test_read_only_explain_analyze_select_allowed() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("EXPLAIN (ANALYZE, FORMAT JSON) SELECT * FROM users")
            .is_ok());
        assert!(engine
            .check_read_only("EXPLAIN ANALYZE SELECT * FROM users")
            .is_ok());
        assert!(engine
            .check_read_only("EXPLAIN WITH x AS (SELECT 1) SELECT * FROM x")
            .is_ok());
    }

    #[test]
    fn test_read_only_with_delete_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only(
                "WITH deleted AS (DELETE FROM users RETURNING id) SELECT * FROM deleted"
            )
            .is_err());
    }

    #[test]
    fn test_read_only_with_non_select_tail_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("WITH x AS (SELECT 1) DELETE FROM users")
            .is_err());
    }

    #[test]
    fn test_read_only_explain_analyze_delete_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("EXPLAIN ANALYZE DELETE FROM users")
            .is_err());
    }

    #[test]
    fn test_read_only_explain_delete_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine.check_read_only("EXPLAIN DELETE FROM users").is_err());
    }

    #[test]
    fn test_read_only_select_with_delete_in_string_allowed() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("SELECT 'DELETE FROM users' AS sample")
            .is_ok());
    }

    #[test]
    fn test_read_only_rejects_session_change_function() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("SELECT set_config('role', 'postgres', false)")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_sleep_function() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine.check_read_only("SELECT pg_sleep(5)").is_err());
    }

    #[test]
    fn test_read_only_rejects_copy_to_program() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("COPY (SELECT * FROM users) TO PROGRAM 'cat'")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_do_block() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("DO $$ BEGIN DELETE FROM users; END $$")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_lock_table() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("LOCK TABLE users IN ACCESS EXCLUSIVE MODE")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_prepare_delete() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("PREPARE doomed AS DELETE FROM users")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_call() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("CALL pg_catalog.rotate_logfile()")
            .is_err());
    }

    #[test]
    fn test_read_only_rejects_declare_cursor() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("DECLARE c CURSOR FOR SELECT * FROM users")
            .is_err());
    }

    #[test]
    fn test_read_only_delete_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "DELETE FROM users";
        assert!(engine.check_read_only(sql).is_err());
    }

    #[test]
    fn test_read_only_drop_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "DROP TABLE users";
        assert!(engine.check_read_only(sql).is_err());
    }

    #[test]
    fn test_read_only_select_with_leading_line_comment() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "-- Valid sampleId for timing tests\nSELECT id FROM users LIMIT 1";
        assert!(engine.check_read_only(sql).is_ok());
    }

    #[test]
    fn test_read_only_select_with_block_comment() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "/* get sample */ SELECT id FROM users";
        assert!(engine.check_read_only(sql).is_ok());
    }

    #[test]
    fn test_read_only_dml_hidden_behind_comment_rejected() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let sql = "-- comment\nDELETE FROM users";
        assert!(engine.check_read_only(sql).is_err());
    }

    #[test]
    fn test_with_trailing_semicolon() {
        let sql = strip_trailing_semicolons("WITH x AS (SELECT 1) SELECT * FROM x;");
        assert_eq!(count_statements(sql), 1);
    }

    #[test]
    fn test_with_cte() {
        let sql = "WITH x AS (SELECT 1) SELECT * FROM x";
        assert_eq!(count_statements(sql), 1);
    }

    #[test]
    fn test_max_sql_bytes() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let big_sql = "SELECT ".to_string() + &"a".repeat(MAX_SQL_BYTES);
        assert!(engine.validate(&big_sql).is_err());
    }

    #[test]
    fn test_allowed_schema_pass() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine.validate("SELECT * FROM public.users").is_ok());
    }

    #[test]
    fn test_denied_relation() {
        let policy = SecurityPolicy {
            denied_relations: vec!["public.users_credentials".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate("SELECT * FROM public.users_credentials")
            .is_err());
    }

    #[test]
    fn test_document_find_valid() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentFindRequest {
            database: "app".into(),
            collection: "users".into(),
            filter: serde_json::json!({"active": true}),
            projection: None,
            sort: None,
            limit: 10,
        };
        assert!(engine.validate_document_find(&request).is_ok());
    }

    #[test]
    fn test_document_find_denied_collection() {
        let policy = SecurityPolicy {
            denied_collections: vec!["app.secrets".into()],
            ..Default::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        let request = DocumentFindRequest {
            database: "app".into(),
            collection: "secrets".into(),
            filter: serde_json::json!({}),
            projection: None,
            sort: None,
            limit: 10,
        };
        assert!(engine.validate_document_find(&request).is_err());
    }

    #[test]
    fn test_document_find_rejects_invalid_filter() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentFindRequest {
            database: "app".into(),
            collection: "users".into(),
            filter: serde_json::json!("not an object"),
            projection: None,
            sort: None,
            limit: 10,
        };
        assert!(engine.validate_document_find(&request).is_err());
    }

    #[test]
    fn test_document_count_rejects_empty_filter() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentCountRequest {
            database: "app".into(),
            collection: "users".into(),
            filter: serde_json::json!({}),
        };
        let err = engine.validate_document_count(&request).unwrap_err();
        assert!(err.to_string().contains("Count filter must not be empty"));
    }

    #[test]
    fn test_document_count_allows_non_empty_filter() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentCountRequest {
            database: "app".into(),
            collection: "users".into(),
            filter: serde_json::json!({"owners.30": {"$exists": true}}),
        };
        assert!(engine.validate_document_count(&request).is_ok());
    }

    #[test]
    fn test_document_aggregate_rejects_write_stage() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([{"$match": {"active": true}}, {"$out": "copy"}]),
            limit: 10,
        };
        assert!(engine.validate_document_aggregate(&request).is_err());
    }

    #[test]
    fn test_document_validation_rejects_javascript_operators_at_any_depth() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        for operator in FORBIDDEN_MQL_JAVASCRIPT_OPERATORS {
            let value = serde_json::from_str(&format!(
                r#"{{"$and":[{{"nested":{{"again":{{"{operator}":{{"body":"never execute"}}}}}}}}]}}"#
            ))
            .unwrap();
            let err = engine.validate_document_mql(&value).unwrap_err();
            assert!(err.to_string().contains(operator));
            assert!(!err.to_string().contains("never execute"));
        }
    }

    #[test]
    fn test_document_find_rejects_javascript_in_filter_projection_and_sort() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        for (filter, projection, sort) in [
            (serde_json::json!({"$where": "never execute"}), None, None),
            (
                serde_json::json!({"active": true}),
                Some(serde_json::json!({"nested": {"$function": {}}})),
                None,
            ),
            (
                serde_json::json!({"active": true}),
                None,
                Some(serde_json::json!({"nested": {"$accumulator": {}}})),
            ),
        ] {
            let request = DocumentFindRequest {
                database: "app".into(),
                collection: "users".into(),
                filter,
                projection,
                sort,
                limit: 10,
            };
            assert!(engine.validate_document_find(&request).is_err());
        }
    }

    #[test]
    fn test_all_other_document_operations_reject_javascript_operators() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .validate_document_aggregate(&DocumentAggregateRequest {
                database: "app".into(),
                collection: "users".into(),
                pipeline: serde_json::json!([{"$match": {"nested": {"$function": {}}}}]),
                limit: 10,
            })
            .is_err());
        assert!(engine
            .validate_document_distinct(&DocumentDistinctRequest {
                database: "app".into(),
                collection: "users".into(),
                field: "name".into(),
                filter: serde_json::json!({"$where": "never execute"}),
                limit: 10,
            })
            .is_err());
        assert!(engine
            .validate_document_count(&DocumentCountRequest {
                database: "app".into(),
                collection: "users".into(),
                filter: serde_json::json!({"nested": {"$accumulator": {}}}),
            })
            .is_err());
        assert!(engine
            .validate_document_explain(&DocumentExplainRequest {
                database: "app".into(),
                collection: "users".into(),
                filter: serde_json::json!({}),
                projection: Some(serde_json::json!({"nested": {"$function": {}}})),
                sort: None,
                limit: None,
            })
            .is_err());
        assert!(engine
            .validate_document_field_profile(&DocumentFieldProfileRequest {
                database: "app".into(),
                collection: "users".into(),
                field: "name".into(),
                filter: serde_json::json!({"nested": {"$where": "never execute"}}),
                sample_size: 10,
                examples: 1,
            })
            .is_err());
        assert!(engine
            .validate_document_schema(&DocumentSchemaRequest {
                database: "app".into(),
                collection: "users".into(),
                filter: serde_json::json!({"nested": {"$accumulator": {}}}),
                sample_size: 10,
                examples: 1,
            })
            .is_err());
        assert!(engine
            .validate_document_fixture(&DocumentFixtureRequest {
                database: "app".into(),
                collection: "users".into(),
                filter: serde_json::json!({}),
                projection: Some(serde_json::json!({"nested": {"$function": {}}})),
                limit: 1,
                redact_fields: vec![],
            })
            .is_err());
    }

    #[test]
    fn test_document_aggregate_rejects_non_object_stage_with_retry_guidance() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!(["$match"]),
            limit: 10,
        };
        let err = engine.validate_document_aggregate(&request).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Aggregation stages must be JSON objects"));
        assert!(message.contains("do not repeat the same call"));
        assert!(message.contains(r#"[{"$match":{"active":true}}]"#));
    }

    #[test]
    fn test_document_aggregate_rejects_empty_stage_with_retry_guidance() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([{}]),
            limit: 10,
        };
        let err = engine.validate_document_aggregate(&request).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Aggregation stages must contain exactly one operator"));
        assert!(message.contains("do not repeat the same call"));
        assert!(message.contains(r#"[{"$match":{"active":true}}]"#));
    }

    #[test]
    fn test_document_aggregate_rejects_multi_operator_stage() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([{"$match": {"active": true}, "$sort": {"_id": 1}}]),
            limit: 10,
        };
        assert!(engine.validate_document_aggregate(&request).is_err());
    }

    #[test]
    fn test_document_aggregate_allows_read_only_pipeline() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([
                {"$match": {"active": true}},
                {"$group": {"_id": "$owner", "count": {"$sum": 1}}},
                {"$sort": {"count": -1}}
            ]),
            limit: 10,
        };
        assert!(engine.validate_document_aggregate(&request).is_ok());
    }

    #[test]
    fn test_document_aggregate_rejects_write_stage_and_invalid_limit() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let write_stage = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([{"$out": "archive"}]),
            limit: 10,
        };
        assert!(engine.validate_document_aggregate(&write_stage).is_err());

        let invalid_limit = DocumentAggregateRequest {
            pipeline: serde_json::json!([]),
            limit: 0,
            ..write_stage
        };
        assert!(engine.validate_document_aggregate(&invalid_limit).is_err());
    }

    #[test]
    fn test_document_distinct_rejects_invalid_field_path() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentDistinctRequest {
            database: "app".into(),
            collection: "users".into(),
            field: "$owner".into(),
            filter: serde_json::json!({}),
            limit: 10,
        };
        assert!(engine.validate_document_distinct(&request).is_err());
    }

    #[test]
    fn test_document_distinct_accepts_valid_request() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentDistinctRequest {
            database: "app".into(),
            collection: "users".into(),
            field: "profile.name".into(),
            filter: serde_json::json!({"active": true}),
            limit: 10,
        };

        assert!(engine.validate_document_distinct(&request).is_ok());
    }

    #[test]
    fn test_document_distinct_rejects_invalid_filter_and_limit() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let invalid_filter = DocumentDistinctRequest {
            database: "app".into(),
            collection: "users".into(),
            field: "name".into(),
            filter: serde_json::json!("active"),
            limit: 10,
        };
        assert!(engine.validate_document_distinct(&invalid_filter).is_err());

        let invalid_limit = DocumentDistinctRequest {
            filter: serde_json::json!({}),
            limit: 0,
            ..invalid_filter
        };
        assert!(engine.validate_document_distinct(&invalid_limit).is_err());
    }

    #[test]
    fn test_document_collection_filter_hides_denied_collection() {
        let policy = SecurityPolicy {
            denied_collections: vec!["app.secrets".into()],
            ..Default::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        let collections =
            engine.filter_document_collections("app", vec!["users".into(), "secrets".into()]);
        assert_eq!(collections, vec!["users"]);
    }

    #[test]
    fn test_result_size_accepts_limits_and_rejects_overages() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine.check_result_size(1, 1).is_ok());
        assert!(engine.check_result_size(501, 1).is_err());
        assert!(engine.check_result_size(1, 2_000_001).is_err());
    }

    #[test]
    fn extracts_explain_targets_and_options() {
        assert_eq!(extract_explain_target("EXPLAIN SELECT 1"), Some("SELECT 1"));
        assert_eq!(
            extract_explain_target("EXPLAIN (ANALYZE, BUFFERS) SELECT 1"),
            Some("SELECT 1")
        );
        assert_eq!(
            extract_explain_target("EXPLAIN ANALYZE SELECT 1"),
            Some("SELECT 1")
        );
        assert_eq!(extract_explain_target("SELECT 1"), None);
        assert_eq!(extract_explain_target("EXPLAIN (ANALYZE SELECT 1"), None);
    }

    #[test]
    fn validates_document_find_projection_sort_and_limits() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let valid = DocumentFindRequest {
            database: "app".into(),
            collection: "users".into(),
            filter: serde_json::json!({"active": true}),
            projection: Some(serde_json::json!({"name": 1})),
            sort: Some(serde_json::json!({"name": 1})),
            limit: 10,
        };
        assert!(engine.validate_document_find(&valid).is_ok());

        for request in [
            DocumentFindRequest {
                projection: Some(serde_json::json!("name")),
                ..valid.clone()
            },
            DocumentFindRequest {
                sort: Some(serde_json::json!("name")),
                ..valid.clone()
            },
            DocumentFindRequest {
                limit: 0,
                ..valid.clone()
            },
            DocumentFindRequest {
                limit: 501,
                ..valid
            },
        ] {
            assert!(engine.validate_document_find(&request).is_err());
        }
    }

    #[test]
    fn strips_sql_comments_without_changing_literals() {
        assert_eq!(
            SecurityEngine::strip_sql_comments("SELECT 1 -- note\n"),
            "SELECT 1 \n"
        );
        assert_eq!(
            SecurityEngine::strip_sql_comments("SELECT 1 /* remove */"),
            "SELECT 1 "
        );
    }

    #[test]
    fn counts_sql_statements_and_ignores_trailing_semicolons() {
        assert_eq!(count_statements("SELECT 1"), 1);
        assert_eq!(count_statements("SELECT 1; SELECT 2"), 2);
        assert_eq!(strip_trailing_semicolons("SELECT 1;;;"), "SELECT 1");
    }

    #[test]
    fn rejects_unbalanced_sql_parentheses() {
        assert_eq!(count_statements("SELECT (1"), 1);
    }

    #[test]
    fn detects_schema_references_and_allowed_patterns() {
        assert!(!has_schema_reference("public.", &["public".into()]));
        assert!(!has_schema_reference("private.x.y.", &[]));
        assert!(!has_schema_reference("select a.b from users", &[]));
    }

    #[test]
    fn sanitizes_keyword_scan_comments_and_literals() {
        let sanitized =
            sanitize_for_keyword_scan("SELECT 'FROM x' /* hidden */ FROM users -- end\n");
        assert!(sanitized.contains("SELECT"));
        assert!(sanitized.contains("FROM USERS"));
        assert!(!sanitized.contains("hidden"));
        assert!(!sanitized.contains("end"));
    }

    #[test]
    fn finds_only_top_level_keywords() {
        let sql = "SELECT (SELECT 1 AS nested) AS value FROM users";
        assert_eq!(find_top_level_keyword(sql, 0, "AS"), Some(28));
        assert_eq!(find_top_level_keyword(sql, 0, "FROM"), Some(37));
        assert_eq!(find_top_level_keyword("SELECT 'FROM'", 0, "FROM"), None);
    }

    #[test]
    fn scans_keywords_across_comments_quotes_and_nested_parentheses() {
        let sql = "SELECT (1 /* FROM */), \"FROM\" -- FROM\nFROM users";
        assert_eq!(find_top_level_keyword(sql, 0, "FROM"), sql.rfind("FROM"));
        let sanitized = sanitize_for_keyword_scan("SELECT \"secret\" -- comment\nFROM users");
        assert!(sanitized.contains("SELECT"));
        assert!(sanitized.contains("FROM USERS"));
    }

    #[test]
    fn validates_system_queries_without_schema_allowlist() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .validate_system("SELECT * FROM information_schema.tables")
            .is_ok());
        assert!(engine.validate_system("").is_err());
        assert!(engine.validate_system("DROP TABLE users").is_err());
    }
}
