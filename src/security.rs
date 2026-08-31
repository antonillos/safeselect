use crate::backend::{
    DocumentAggregateRequest, DocumentCollectionRequest, DocumentCountRequest,
    DocumentDistinctRequest, DocumentExplainRequest, DocumentFieldProfileRequest,
    DocumentFindRequest, DocumentFixtureRequest, DocumentSchemaRequest,
};
use crate::config::{LimitsConfig, SecurityPolicy};
use crate::error::{Result, SafeselectError};
use sqlparser::ast::{
    ArrayElemTypeDef, BinaryOperator, DataType, Expr, ObjectName, ObjectNamePart, Query, SetExpr,
    Table, TableFactor, Visit, Visitor,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::collections::HashSet;
use std::ops::ControlFlow;

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

fn contains_nested_sql_block_comment(sql: &str) -> bool {
    let Some((_, after_open)) = sql.split_once("/*") else {
        return false;
    };
    let Some((comment_body, _)) = after_open.split_once("*/") else {
        return false;
    };
    comment_body.contains("/*")
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
        if is_system_schema(schema) {
            return Err(SafeselectError::QueryRejected(format!(
                "Schema '{schema}' is reserved for PostgreSQL system catalogs"
            )));
        }
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
        self.validate_optional_document_mql(request.projection.as_ref(), request.sort.as_ref())?;

        Ok(())
    }

    pub fn validate_document_find_arguments(&self, args: &serde_json::Value) -> Result<()> {
        self.validate_document_max_time(args.get("maxTimeMS"))?;
        if let Some(batch_size) = args.get("batchSize") {
            let batch_size = batch_size.as_u64().ok_or_else(|| {
                SafeselectError::QueryRejected("Document batchSize must be an integer".into())
            })?;
            if batch_size == 0 || batch_size > self.limits.max_rows {
                return Err(SafeselectError::QueryRejected(format!(
                    "Document batchSize must be between 1 and {}",
                    self.limits.max_rows
                )));
            }
        }
        Ok(())
    }

    pub fn validate_document_aggregate_arguments(&self, args: &serde_json::Value) -> Result<()> {
        if args
            .get("allowDiskUse")
            .is_some_and(|allow| allow.as_bool() != Some(false))
        {
            return Err(SafeselectError::QueryRejected(
                "MongoDB allowDiskUse must be false for read-only aggregation".into(),
            ));
        }
        Ok(())
    }

    fn validate_document_max_time(&self, max_time: Option<&serde_json::Value>) -> Result<()> {
        let Some(max_time) = max_time else {
            return Ok(());
        };
        let max_time = max_time.as_u64().ok_or_else(|| {
            SafeselectError::QueryRejected("Document maxTimeMS must be an integer".into())
        })?;
        if max_time == 0 {
            return Err(SafeselectError::QueryRejected(
                "Document maxTimeMS must be greater than zero".into(),
            ));
        }
        Ok(())
    }

    fn validate_optional_document_mql(
        &self,
        projection: Option<&serde_json::Value>,
        sort: Option<&serde_json::Value>,
    ) -> Result<()> {
        for value in [projection, sort].into_iter().flatten() {
            self.validate_document_mql(value)?;
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
        self.validate_aggregate_pipeline(&request.database, request.pipeline.as_array().unwrap())?;
        Ok(())
    }

    fn validate_aggregate_pipeline(
        &self,
        database: &str,
        stages: &[serde_json::Value],
    ) -> Result<()> {
        for stage in stages {
            self.validate_aggregate_stage(database, stage)?;
        }
        Ok(())
    }

    fn validate_aggregate_stage(&self, database: &str, stage: &serde_json::Value) -> Result<()> {
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
        if let Some(name) = stage_object.keys().find(|name| {
            matches!(
                name.as_str(),
                "$out"
                    | "$merge"
                    | "$currentOp"
                    | "$listSessions"
                    | "$collStats"
                    | "$planCacheStats"
            )
        }) {
            return Err(SafeselectError::QueryRejected(format!(
                "Aggregation stage '{name}' is not read-only"
            )));
        }
        self.validate_nested_aggregate_namespaces(database, stage_object)?;
        self.validate_document_mql(stage)
    }

    fn validate_nested_aggregate_namespaces(
        &self,
        database: &str,
        stage: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let Some((operator, value)) = stage.iter().next() else {
            return Ok(());
        };
        match operator.as_str() {
            "$lookup" => self.validate_lookup_stage(database, value),
            "$unionWith" => self.validate_union_with_stage(database, value),
            "$graphLookup" => self.validate_graph_lookup_stage(database, value),
            "$facet" => self.validate_facet_stage(database, value),
            _ => Ok(()),
        }
    }

    fn validate_lookup_stage(&self, database: &str, value: &serde_json::Value) -> Result<()> {
        let lookup = value.as_object().ok_or_else(|| {
            SafeselectError::QueryRejected("MongoDB $lookup stage must be an object".into())
        })?;
        self.validate_nested_collection(database, lookup.get("from"))?;
        self.validate_nested_pipeline(database, lookup.get("pipeline"))
    }

    fn validate_union_with_stage(&self, database: &str, value: &serde_json::Value) -> Result<()> {
        let (collection, pipeline) = match value {
            serde_json::Value::String(collection) => (Some(collection.as_str()), None),
            serde_json::Value::Object(union) => (
                union.get("coll").and_then(|value| value.as_str()),
                union.get("pipeline"),
            ),
            _ => (None, None),
        };
        self.validate_nested_collection_value(database, collection)?;
        self.validate_nested_pipeline(database, pipeline)
    }

    fn validate_graph_lookup_stage(&self, database: &str, value: &serde_json::Value) -> Result<()> {
        let graph_lookup = value.as_object().ok_or_else(|| {
            SafeselectError::QueryRejected("MongoDB $graphLookup stage must be an object".into())
        })?;
        self.validate_nested_collection(database, graph_lookup.get("from"))
    }

    fn validate_facet_stage(&self, database: &str, value: &serde_json::Value) -> Result<()> {
        let facets = value.as_object().ok_or_else(|| {
            SafeselectError::QueryRejected("MongoDB $facet stage must be an object".into())
        })?;
        for pipeline in facets.values() {
            self.validate_nested_pipeline(database, Some(pipeline))?;
        }
        Ok(())
    }

    fn validate_nested_collection(
        &self,
        database: &str,
        collection: Option<&serde_json::Value>,
    ) -> Result<()> {
        self.validate_nested_collection_value(database, collection.and_then(|value| value.as_str()))
    }

    fn validate_nested_collection_value(
        &self,
        database: &str,
        collection: Option<&str>,
    ) -> Result<()> {
        if let Some(collection) = collection {
            self.validate_document_collection(&DocumentCollectionRequest {
                database: database.into(),
                collection: collection.into(),
            })?;
        }
        Ok(())
    }

    fn validate_nested_pipeline(
        &self,
        database: &str,
        pipeline: Option<&serde_json::Value>,
    ) -> Result<()> {
        let Some(pipeline) = pipeline else {
            return Ok(());
        };
        let Some(stages) = pipeline.as_array() else {
            return Err(SafeselectError::QueryRejected(
                "Nested MongoDB aggregation pipelines must be JSON arrays".into(),
            ));
        };
        self.validate_aggregate_pipeline(database, stages)
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

        if contains_nested_sql_block_comment(trimmed) {
            return Err(SafeselectError::QueryRejected(
                "Nested SQL block comments are not supported".into(),
            ));
        }

        self.validate_policy_constraints(trimmed)?;
        self.check_read_only(trimmed)
    }

    fn validate_policy_constraints(&self, query: &str) -> Result<()> {
        self.policy
            .require_single_statement
            .then(|| self.check_single_statement(query))
            .transpose()?;
        self.check_system_schema_references(query)?;
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

        self.validate_system_policy(trimmed)
    }

    fn validate_system_policy(&self, sql: &str) -> Result<()> {
        if self.policy.require_single_statement {
            self.check_single_statement(sql)?;
        }

        self.check_read_only(sql)?;

        if !self.policy.denied_relations.is_empty() {
            self.check_denied_relations(sql)?;
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
        let mut in_single = false;
        let mut in_double = false;
        let mut dollar_delimiter: Option<String> = None;
        while let Some(ch) = chars.next() {
            if consume_dollar_quote(&mut chars, ch, &mut result, &mut dollar_delimiter)
                || consume_quoted_char(&mut chars, ch, &mut result, &mut in_single, &mut in_double)
            {
                continue;
            }
            match (ch, chars.peek().copied()) {
                ('-', Some('-')) => Self::skip_line_comment(&mut chars, &mut result),
                ('/', Some('*')) => Self::skip_block_comment(&mut chars),
                _ => result.push(ch),
            }
        }
        result
    }

    fn skip_line_comment(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        result: &mut String,
    ) {
        for ch in chars.by_ref() {
            if ch == '\n' {
                result.push('\n');
                break;
            }
        }
    }

    fn skip_block_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        chars.next();
        let mut previous = ' ';
        for ch in chars.by_ref() {
            if previous == '*' && ch == '/' {
                break;
            }
            previous = ch;
        }
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

        if upper.starts_with("SELECT") || upper.starts_with("TABLE") {
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

        if upper.starts_with("SELECT") || upper.starts_with("TABLE") {
            return self.check_select_like_body(trimmed);
        }

        if upper.starts_with("WITH") {
            let body = extract_with_query_body(trimmed).ok_or_else(|| {
                SafeselectError::QueryRejected(
                    "Read-only mode: could not validate WITH query".into(),
                )
            })?;

            self.validate_with_query_body(body)?;

            self.check_forbidden_tokens(trimmed, true)?;
            return Ok(());
        }

        Err(SafeselectError::QueryRejected(
            "Read-only mode: unrecognized statement type".into(),
        ))
    }

    fn check_select_like_body(&self, sql: &str) -> Result<()> {
        self.check_forbidden_tokens(sql, false)
    }

    fn validate_with_query_body(&self, body: &str) -> Result<()> {
        let upper = body.trim_start().to_uppercase();
        if !upper.starts_with("SELECT") && !upper.starts_with("TABLE") {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: WITH queries must end in SELECT or TABLE".into(),
            ));
        }
        Ok(())
    }

    fn check_forbidden_tokens(&self, sql: &str, allow_with_keyword: bool) -> Result<()> {
        let compact = sanitize_for_keyword_scan(sql);
        if let Some(keyword) = Self::first_forbidden_keyword(&compact, allow_with_keyword) {
            return Err(SafeselectError::QueryRejected(format!(
                "Read-only mode: {keyword} not allowed"
            )));
        }
        if contains_transaction_alias(sql) {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: transaction control not allowed".into(),
            ));
        }
        if let Some(function) = Self::first_forbidden_function(&compact) {
            return Err(SafeselectError::QueryRejected(format!(
                "Read-only mode: function {function} not allowed"
            )));
        }
        if contains_keyword(&compact, "SET") || compact.contains("SETROLE") {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: session changes are not allowed".into(),
            ));
        }

        Ok(())
    }

    fn first_forbidden_keyword(compact: &str, allow_with_keyword: bool) -> Option<&'static str> {
        const FORBIDDEN: &[&str] = &[
            "INSERT",
            "UPDATE",
            "DELETE",
            "DROP",
            "CREATE",
            "ALTER",
            "TRUNCATE",
            "COPY",
            "PREPARE",
            "EXECUTE",
            "CALL",
            "MERGE",
            "REPLACE",
            "GRANT",
            "REVOKE",
            "WITH",
            "DO",
            "DECLARE",
            "LOCK",
            "VACUUM",
            "REINDEX",
            "BEGIN",
            "START",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT",
            "RELEASE",
        ];
        FORBIDDEN.iter().copied().find(|keyword| {
            !(*keyword == "WITH" && (allow_with_keyword || contains_with_ordinality_only(compact)))
                && contains_keyword(compact, keyword)
        })
    }

    fn first_forbidden_function(compact: &str) -> Option<&'static str> {
        const FORBIDDEN: &[&str] = &[
            "SET_CONFIG",
            "PG_SLEEP",
            "SETVAL",
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
        FORBIDDEN
            .iter()
            .copied()
            .find(|function| compact.contains(function))
    }

    fn check_allowed_schemas(&self, sql: &str) -> Result<()> {
        validate_relation_policy(
            sql,
            &self.policy.allowed_schemas,
            &self.policy.denied_relations,
            self.policy.require_single_statement,
        )
        .map_err(SafeselectError::QueryRejected)
    }

    fn check_system_schema_references(&self, sql: &str) -> Result<()> {
        let sql_lower = sql.to_lowercase();
        if ["pg_catalog.", "information_schema.", "pg_toast."]
            .iter()
            .any(|schema| sql_lower.contains(schema))
        {
            return Err(SafeselectError::QueryRejected(
                "Query references a PostgreSQL system catalog schema".into(),
            ));
        }
        Ok(())
    }

    fn check_denied_relations(&self, sql: &str) -> Result<()> {
        if !self.policy.allowed_schemas.is_empty() {
            return Ok(());
        }
        validate_relation_policy(
            sql,
            &[],
            &self.policy.denied_relations,
            self.policy.require_single_statement,
        )
        .map_err(SafeselectError::QueryRejected)
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

fn consume_dollar_quote(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ch: char,
    result: &mut String,
    delimiter: &mut Option<String>,
) -> bool {
    if let Some(active) = delimiter {
        result.push(ch);
        if result.ends_with(active.as_str()) {
            *delimiter = None;
        }
        return true;
    }
    if ch == '$' {
        if let Some(found) = take_dollar_delimiter(chars) {
            result.push('$');
            result.push_str(&found[1..]);
            *delimiter = Some(found);
            return true;
        }
    }
    false
}

fn consume_quoted_char(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ch: char,
    result: &mut String,
    in_single: &mut bool,
    in_double: &mut bool,
) -> bool {
    if ch == '\'' && !*in_double {
        *in_single = !*in_single;
        result.push(ch);
        return true;
    }
    if consume_escaped_char(chars, ch, result, *in_single) {
        return true;
    }
    if ch == '"' && !*in_single {
        *in_double = !*in_double;
        result.push(ch);
        return true;
    }
    if *in_single || *in_double {
        result.push(ch);
        return true;
    }
    false
}

fn consume_escaped_char(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ch: char,
    result: &mut String,
    in_single: bool,
) -> bool {
    if ch != '\\' || !in_single {
        return false;
    }
    result.push(ch);
    if let Some(escaped) = chars.next() {
        result.push(escaped);
    }
    true
}

fn take_dollar_delimiter(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut lookahead = chars.clone();
    let mut tag = String::from("$");
    while let Some(ch) = lookahead.next() {
        if ch == '$' {
            tag.push('$');
            for _ in 0..tag.len() - 1 {
                chars.next();
            }
            chars.next();
            return Some(tag);
        }
        if !(ch.is_ascii_alphanumeric() || ch == '_') || tag.len() > 64 {
            return None;
        }
        tag.push(ch);
    }
    None
}

struct RelationPolicyVisitor<'a> {
    allowed_schemas: &'a [String],
    denied_relations: &'a [String],
    cte_scopes: Vec<HashSet<String>>,
    violation: Option<String>,
}

impl Visitor for RelationPolicyVisitor<'_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if let SetExpr::Table(table) = query.body.as_ref() {
            if let Err(message) = self.validate_table_command(table) {
                self.violation = Some(message);
            }
        }
        let names = query
            .with
            .as_ref()
            .map(|with| {
                with.cte_tables
                    .iter()
                    .map(|cte| canonical_ident(&cte.alias.name))
                    .collect()
            })
            .unwrap_or_default();
        self.cte_scopes.push(names);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.cte_scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Self::Break> {
        if self.violation.is_some() {
            return ControlFlow::Continue(());
        }
        let result = validate_table_factor(self, factor);
        if let Err(message) = result {
            self.violation = Some(message);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if self.violation.is_some() {
            return ControlFlow::Continue(());
        }
        let result = match expr {
            Expr::Function(function) => self.validate_scalar_function(&function.name),
            Expr::BinaryOp {
                op: BinaryOperator::PGCustomBinaryOperator(parts),
                ..
            } => self.validate_custom_operator(parts),
            Expr::Cast { data_type, .. } => self.validate_data_type(data_type),
            Expr::TypedString(typed) => self.validate_data_type(&typed.data_type),
            _ => Ok(()),
        };
        if let Err(message) = result {
            self.violation = Some(message);
        }
        ControlFlow::Continue(())
    }
}

fn validate_table_factor(
    visitor: &RelationPolicyVisitor<'_>,
    factor: &TableFactor,
) -> std::result::Result<(), String> {
    match factor {
        TableFactor::Table { name, args, .. } if args.is_some() => {
            visitor.validate_callable_name(name)
        }
        TableFactor::Table { name, .. } => visitor.validate_relation(name),
        TableFactor::Function { name, .. } => visitor.validate_callable_name(name),
        TableFactor::TableFunction { expr, .. } => visitor.validate_table_function(expr),
        _ => Ok(()),
    }
}

impl RelationPolicyVisitor<'_> {
    fn validate_table_command(&self, table: &Table) -> std::result::Result<(), String> {
        let parts = table_relation_parts(table);
        if parts.is_empty() {
            return Err("TABLE command is missing a relation name".into());
        }
        if parts.len() == 1 && self.is_cte(&parts[0]) {
            return Ok(());
        }
        if !self.allowed_schemas.is_empty() {
            self.validate_allowed_relation(&parts)?;
        }
        self.validate_denied_relation(&parts)
    }

    fn validate_data_type(&self, data_type: &DataType) -> std::result::Result<(), String> {
        match data_type {
            DataType::Custom(name, _) | DataType::NamedTable { name, .. } => {
                let parts = relation_parts(name)?;
                if !self.allowed_schemas.is_empty() {
                    self.validate_allowed_relation(&parts)?;
                }
                self.validate_denied_relation(&parts)
            }
            DataType::Array(ArrayElemTypeDef::AngleBracket(inner))
            | DataType::Array(ArrayElemTypeDef::SquareBracket(inner, _))
            | DataType::Array(ArrayElemTypeDef::Parenthesis(inner)) => {
                self.validate_data_type(inner)
            }
            _ => Ok(()),
        }
    }

    fn validate_custom_operator(&self, parts: &[String]) -> std::result::Result<(), String> {
        if parts.len() < 2 {
            return Err("Unqualified custom operators are not allowed by SQL policy".into());
        }
        if !self.allowed_schemas.is_empty() && !self.allowed_schemas.iter().any(|s| s == &parts[0])
        {
            return Err(format!(
                "Query references schema '{}' outside allowed list ({})",
                parts[0],
                self.allowed_schemas.join(", ")
            ));
        }
        self.validate_denied_relation(parts)
    }

    fn validate_scalar_function(&self, name: &ObjectName) -> std::result::Result<(), String> {
        let parts = relation_parts(name)?;
        if parts.len() == 1 {
            if !self.allowed_schemas.is_empty() {
                return Err(format!(
                    "Unqualified function '{}' is not allowed with a schema policy",
                    parts[0]
                ));
            }
            return self.validate_denied_relation(&parts);
        }
        self.validate_relation(name)
    }

    fn validate_relation(&self, name: &ObjectName) -> std::result::Result<(), String> {
        let parts = relation_parts(name)?;
        if parts.len() == 1 && self.is_cte(&parts[0]) {
            return Ok(());
        }
        if !self.allowed_schemas.is_empty() {
            self.validate_allowed_relation(&parts)?;
        }
        self.validate_denied_relation(&parts)
    }

    fn is_cte(&self, name: &str) -> bool {
        self.cte_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    fn validate_table_function(&self, expr: &Expr) -> std::result::Result<(), String> {
        match expr {
            Expr::Function(function) => self.validate_callable_name(&function.name),
            _ => Err("SQL policy cannot validate this table-function expression".into()),
        }
    }

    fn validate_callable_name(&self, name: &ObjectName) -> std::result::Result<(), String> {
        let parts = relation_parts(name)?;
        if parts.len() == 1 {
            return self.validate_unqualified_callable(&parts[0]);
        }
        self.validate_allowed_relation(&parts).or_else(|error| {
            if self.allowed_schemas.is_empty() {
                self.validate_denied_relation(&parts)
            } else {
                Err(error)
            }
        })
    }

    fn validate_unqualified_callable(&self, name: &str) -> std::result::Result<(), String> {
        if self
            .denied_relations
            .iter()
            .any(|denied| denied.eq_ignore_ascii_case(name))
        {
            return Err(format!("Query references denied relation: {name}"));
        }
        if !self.allowed_schemas.is_empty() && !matches!(name, "unnest" | "generate_series") {
            return Err(format!(
                "Unqualified function '{}' is not allowed with a schema policy",
                name
            ));
        }
        Ok(())
    }

    fn validate_allowed_relation(&self, parts: &[String]) -> std::result::Result<(), String> {
        if parts.len() != 2 {
            return Err(format!(
                "Schema allowlist requires every relation to use schema.table ({})",
                self.allowed_schemas.join(", ")
            ));
        }
        if self.allowed_schemas.iter().any(|schema| {
            (parts[0].to_ascii_lowercase() == parts[0] && schema.eq_ignore_ascii_case(&parts[0]))
                || schema == &parts[0]
        }) {
            Ok(())
        } else {
            Err(format!(
                "Query references schema '{}' outside allowed list ({})",
                parts[0],
                self.allowed_schemas.join(", ")
            ))
        }
    }

    fn validate_denied_relation(&self, parts: &[String]) -> std::result::Result<(), String> {
        let qualified = parts.join(".").to_ascii_lowercase();
        let table = parts
            .last()
            .map(|part| part.to_ascii_lowercase())
            .unwrap_or_default();
        if let Some(denied) = self.denied_relations.iter().find(|denied| {
            let denied = denied.to_ascii_lowercase();
            denied == qualified || (!denied.contains('.') && denied == table)
        }) {
            return Err(format!("Query references denied relation: {denied}"));
        }
        Ok(())
    }
}

fn table_relation_parts(table: &Table) -> Vec<String> {
    [table.schema_name.as_ref(), table.table_name.as_ref()]
        .into_iter()
        .flatten()
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn validate_relation_policy(
    sql: &str,
    allowed_schemas: &[String],
    denied_relations: &[String],
    require_single_statement: bool,
) -> std::result::Result<(), String> {
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, sql)
        .map_err(|error| format!("SQL policy parsing failed: {error}"))?;
    if require_single_statement && statements.len() != 1 {
        return Err("SQL policy requires exactly one parsed statement".into());
    }
    let mut shadowing = CteVisibilityVisitor {
        scopes: Vec::new(),
        allowed_schemas,
        denied_relations,
        violation: None,
    };
    let _ = statements.visit(&mut shadowing);
    if let Some(violation) = shadowing.violation {
        return Err(violation);
    }
    let mut visitor = RelationPolicyVisitor {
        allowed_schemas,
        denied_relations,
        cte_scopes: Vec::new(),
        violation: None,
    };
    let _ = statements.visit(&mut visitor);
    visitor.violation.map_or(Ok(()), Err)
}

struct CteVisibilityVisitor<'a> {
    scopes: Vec<QueryScopeFrame>,
    allowed_schemas: &'a [String],
    denied_relations: &'a [String],
    violation: Option<String>,
}

impl Visitor for CteVisibilityVisitor<'_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        let inherited = self
            .scopes
            .last()
            .and_then(|parent| {
                let pointer = query as *const Query;
                parent
                    .cte_scopes
                    .iter()
                    .find(|(cte, _)| *cte == pointer)
                    .map(|(_, scope)| scope.clone())
                    .or_else(|| Some(parent.body_scope.clone()))
            })
            .unwrap_or_default();
        let aliases: Vec<String> = query
            .with
            .as_ref()
            .map(|with| {
                with.cte_tables
                    .iter()
                    .map(|cte| canonical_ident(&cte.alias.name))
                    .collect()
            })
            .unwrap_or_default();
        if query.with.as_ref().is_some_and(|with| !with.recursive) {
            for (index, cte) in query
                .with
                .as_ref()
                .into_iter()
                .flat_map(|with| with.cte_tables.iter())
                .enumerate()
            {
                let targets: HashSet<String> = aliases[index..]
                    .iter()
                    .filter(|alias| {
                        !inherited.contains(*alias) && self.relation_violates_policy(alias)
                    })
                    .cloned()
                    .collect();
                let mut references = UnqualifiedRelationVisitor {
                    targets: &targets,
                    local_scopes: Vec::new(),
                    found: None,
                };
                let _ = cte.query.visit(&mut references);
                if let Some(target) = references.found {
                    self.violation = Some(format!(
                        "Non-recursive CTE '{}' is referenced before it is visible",
                        target
                    ));
                    return ControlFlow::Break(());
                }
            }
        }
        let body_scope: HashSet<String> = inherited
            .iter()
            .cloned()
            .chain(aliases.iter().cloned())
            .collect();
        let cte_scopes = query
            .with
            .as_ref()
            .map(|with| {
                with.cte_tables
                    .iter()
                    .enumerate()
                    .map(|(index, cte)| {
                        let visible = if with.recursive {
                            aliases.clone()
                        } else {
                            aliases[..index].to_vec()
                        };
                        let scope = inherited.iter().cloned().chain(visible).collect();
                        (cte.query.as_ref() as *const Query, scope)
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.scopes.push(QueryScopeFrame {
            body_scope,
            cte_scopes,
        });
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.scopes.pop();
        ControlFlow::Continue(())
    }
}

impl CteVisibilityVisitor<'_> {
    fn relation_violates_policy(&self, relation: &str) -> bool {
        !self.allowed_schemas.is_empty()
            || self
                .denied_relations
                .iter()
                .any(|denied| !denied.contains('.') && denied.eq_ignore_ascii_case(relation))
    }
}

struct QueryScopeFrame {
    body_scope: HashSet<String>,
    cte_scopes: Vec<(*const Query, HashSet<String>)>,
}

struct UnqualifiedRelationVisitor<'a> {
    targets: &'a HashSet<String>,
    local_scopes: Vec<HashSet<String>>,
    found: Option<String>,
}

impl Visitor for UnqualifiedRelationVisitor<'_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        let aliases = query
            .with
            .as_ref()
            .map(|with| {
                with.cte_tables
                    .iter()
                    .map(|cte| canonical_ident(&cte.alias.name))
                    .collect()
            })
            .unwrap_or_default();
        self.local_scopes.push(aliases);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.local_scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Self::Break> {
        if let TableFactor::Table {
            name, args: None, ..
        } = factor
        {
            if let Ok(parts) = relation_parts(name) {
                if parts.len() == 1
                    && self.targets.contains(&parts[0])
                    && !self
                        .local_scopes
                        .iter()
                        .rev()
                        .any(|scope| scope.contains(&parts[0]))
                {
                    self.found = Some(parts[0].clone());
                }
            }
        }
        ControlFlow::Continue(())
    }
}

fn relation_parts(name: &ObjectName) -> std::result::Result<Vec<String>, String> {
    name.0
        .iter()
        .map(|part| match part {
            ObjectNamePart::Identifier(ident) => Ok(canonical_ident(ident)),
            ObjectNamePart::Function(_) => {
                Err("Computed relation names are not supported by SQL policy".into())
            }
        })
        .collect()
}

fn canonical_ident(ident: &sqlparser::ast::Ident) -> String {
    if ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_ascii_lowercase()
    }
}

fn is_system_schema(schema: &str) -> bool {
    let lower = schema.to_ascii_lowercase();
    lower == "pg_catalog"
        || lower == "information_schema"
        || lower == "pg_toast"
        || lower.starts_with("pg_")
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

fn contains_transaction_alias(sql: &str) -> bool {
    let normalized = SecurityEngine::strip_sql_comments(sql).replace(';', " ; ");
    let mut tokens = normalized.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == ";"
            && matches!(
                tokens.next().map(str::to_ascii_uppercase).as_deref(),
                Some("END" | "ABORT")
            )
        {
            return true;
        }
    }
    false
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

    consume_cte_list(sql, i)
}

fn consume_cte_list(sql: &str, mut i: usize) -> Option<&str> {
    loop {
        let (next, done) = parse_cte(sql, i)?;
        if done {
            return sql.get(next..);
        }
        i = next;
    }
}

fn parse_cte(sql: &str, mut i: usize) -> Option<(usize, bool)> {
    let as_index = find_top_level_keyword(sql, i, "AS")?;
    i = skip_sql_whitespace(sql, as_index + 2);
    if starts_with_keyword_at(sql, i, "NOT") {
        i = skip_sql_whitespace(sql, i + 3);
    }
    if starts_with_keyword_at(sql, i, "MATERIALIZED") {
        i = skip_sql_whitespace(sql, i + "MATERIALIZED".len());
    }
    if sql.get(i..=i)? != "(" {
        return None;
    }
    i = skip_sql_whitespace(sql, skip_balanced_parentheses(sql, i)?);
    if sql.get(i..=i) == Some(",") {
        Some((skip_sql_whitespace(sql, i + 1), false))
    } else {
        Some((i, true))
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
        return extract_parenthesized_explain_target(after_explain);
    }

    Some(strip_explain_option_prefix(after_explain).unwrap_or(after_explain))
}

fn extract_parenthesized_explain_target(sql: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (idx, ch) in sql.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return sql.get(idx + 1..).map(str::trim_start);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_explain_option_prefix(sql: &str) -> Option<&str> {
    let upper = sql.to_uppercase();
    for option in [
        "ANALYZE", "VERBOSE", "BUFFERS", "SETTINGS", "WAL", "TIMING", "SUMMARY",
    ] {
        if upper.starts_with(option) {
            return sql.get(option.len()..).map(str::trim_start);
        }
    }
    None
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
    fn rejects_transaction_control_in_multi_statement_selects() {
        let engine = SecurityEngine::new(
            SecurityPolicy {
                require_single_statement: false,
                ..SecurityPolicy::default()
            },
            LimitsConfig::default(),
        );
        for sql in [
            "SELECT 1; COMMIT",
            "SELECT 1; BEGIN READ WRITE",
            "SELECT 1; ROLLBACK",
            "SELECT 1; SAVEPOINT nested",
            "SELECT 1; END",
            "SELECT 1; ABORT",
            "SELECT 1;\n\tEND",
        ] {
            assert!(engine.check_read_only(sql).is_err(), "{sql}");
        }
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
    fn test_read_only_rejects_sequence_mutation_and_nested_comments() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("SELECT setval('public.safe_sequence', 99, true)")
            .is_err());
        assert!(engine
            .validate("SELECT /* outer /* nested */ 1 */ 1")
            .is_err());
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
    fn test_extract_with_query_body_variants() {
        assert_eq!(extract_with_query_body("SELECT 1"), None);
        assert_eq!(
            extract_with_query_body(
                "WITH RECURSIVE x AS NOT MATERIALIZED (SELECT 1) SELECT * FROM x"
            ),
            Some("SELECT * FROM x")
        );
        assert_eq!(
            extract_with_query_body("WITH a AS (SELECT 1), b AS (SELECT 2) SELECT * FROM b"),
            Some("SELECT * FROM b")
        );
        assert_eq!(extract_with_query_body("WITH broken AS SELECT 1"), None);
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
    fn schema_allowlist_rejects_unqualified_and_mixed_relations() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());

        assert!(engine.validate("SELECT * FROM users").is_err());
        assert!(engine.validate("SELECT * FROM private.secrets").is_err());
        assert!(engine
            .validate("SELECT * FROM public.users JOIN private.secrets ON true")
            .is_err());
        assert!(engine
            .validate("SELECT * FROM public.users WHERE id IN (SELECT id FROM private.secrets)")
            .is_err());
        assert!(engine.validate("SELECT * FROM private.expose()").is_err());
        assert!(engine
            .validate("WITH expose AS (SELECT 1) SELECT * FROM expose()")
            .is_err());
        assert!(engine
            .validate("SELECT * FROM ROWS FROM (private.expose()) AS exposed")
            .is_err());
        assert!(engine
            .validate("SELECT * FROM TABLE(private.expose()) AS exposed")
            .is_err());
        assert!(engine
            .validate("SELECT private.expose(id) FROM public.users")
            .is_err());
        assert!(engine
            .validate("SELECT expose(id) FROM public.users")
            .is_err());
        assert!(engine
            .validate("SELECT count(*) FROM public.users")
            .is_err());
        assert!(engine
            .validate("SELECT 1 OPERATOR(private.custom) 1")
            .is_err());
        assert!(engine
            .validate("SELECT 1 OPERATOR(public.custom) 1")
            .is_ok());
        assert!(engine
            .validate("WITH leaked AS (TABLE private.secrets) SELECT * FROM leaked")
            .is_err());
        assert!(engine
            .validate("WITH visible AS (TABLE public.users) SELECT * FROM visible")
            .is_ok());
        assert!(engine
            .validate("SELECT CAST('x' AS private.leaky_type) FROM public.users")
            .is_err());
    }

    #[test]
    fn schema_allowlist_allows_ctes_and_derived_tables_over_allowed_relations() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());

        assert!(engine
            .validate("WITH active AS (SELECT * FROM public.users) SELECT * FROM active")
            .is_ok());
        assert!(engine
            .validate("SELECT * FROM (SELECT * FROM public.users) AS active")
            .is_ok());
        assert!(engine
            .validate("SELECT * FROM public.users UNION SELECT * FROM public.archived_users")
            .is_ok());
    }

    #[test]
    fn schema_allowlist_keeps_cte_names_scoped_to_their_query() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());

        assert!(engine
            .validate(
                "SELECT * FROM users WHERE EXISTS (WITH users AS (SELECT * FROM public.users) SELECT * FROM users)"
            )
            .is_err());
    }

    #[test]
    fn schema_allowlist_rejects_non_recursive_cte_shadowing() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            denied_relations: vec!["users".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate("WITH users AS (SELECT * FROM users) SELECT * FROM users")
            .is_err());
    }

    #[test]
    fn cte_shadowing_check_ignores_unrestricted_relations() {
        let policy = SecurityPolicy {
            denied_relations: vec!["public.secrets".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate("WITH users AS (SELECT * FROM users) SELECT * FROM users")
            .is_ok());
    }

    #[test]
    fn schema_allowlist_rejects_forward_cte_references() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            denied_relations: vec!["later".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate(
                "WITH first AS (SELECT * FROM later), later AS (SELECT * FROM public.allowed) SELECT * FROM first"
            )
            .is_err());
    }

    #[test]
    fn schema_allowlist_allows_nested_cte_shadowing() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate(
                "WITH users AS (WITH users AS (SELECT * FROM public.users) SELECT * FROM users) SELECT * FROM users"
            )
            .is_ok());
        assert!(engine
            .validate(
                "WITH x AS (SELECT 1) SELECT * FROM (WITH x AS (SELECT * FROM x) SELECT * FROM x) AS nested"
            )
            .is_ok());
    }

    #[test]
    fn relation_policy_respects_multi_statement_toggle() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            require_single_statement: false,
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate("SELECT * FROM public.first; SELECT * FROM public.second")
            .is_ok());
    }

    #[test]
    fn covers_sql_policy_ratchet_branches() {
        let policy = SecurityPolicy {
            allowed_schemas: vec!["public".into()],
            denied_relations: vec!["private.secrets".into()],
            require_single_statement: false,
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        for sql in [
            "TABLE public.users",
            "WITH x AS (SELECT * FROM public.users) TABLE x",
            "SELECT DATE '2025-01-01'",
            "SELECT * FROM unnest(ARRAY[1, 2]) WITH ORDINALITY AS t(value, ord)",
            "SELECT * FROM public.expose()",
            "SELECT * FROM generate_series(1, 2)",
            "SELECT * FROM public.users",
            "WITH first AS (SELECT * FROM later), later AS (SELECT * FROM public.allowed) SELECT * FROM first",
            "SELECT 1 OPERATOR(public.+) 1",
        ] {
            let _ = engine.validate(sql);
        }
        assert!(engine.validate("SELECT * FROM private.expose()").is_err());
        assert!(engine.validate("SELECT 'x'::private.leaky_type").is_err());
    }

    #[test]
    fn covers_callable_and_wrapped_type_validation() {
        let allowed = vec!["public".to_string()];
        let denied = vec!["blocked".to_string()];
        let visitor = RelationPolicyVisitor {
            allowed_schemas: &allowed,
            denied_relations: &denied,
            cte_scopes: Vec::new(),
            violation: None,
        };
        let unnest = ObjectName(vec![ObjectNamePart::Identifier(
            sqlparser::ast::Ident::new("unnest"),
        )]);
        assert!(visitor.validate_callable_name(&unnest).is_ok());
        let blocked = ObjectName(vec![ObjectNamePart::Identifier(
            sqlparser::ast::Ident::new("blocked"),
        )]);
        assert!(visitor.validate_callable_name(&blocked).is_err());
        let private_type = DataType::Custom(
            ObjectName(vec![
                ObjectNamePart::Identifier(sqlparser::ast::Ident::new("private")),
                ObjectNamePart::Identifier(sqlparser::ast::Ident::new("secret")),
            ]),
            Vec::new(),
        );
        assert!(visitor
            .validate_data_type(&DataType::Array(ArrayElemTypeDef::SquareBracket(
                Box::new(private_type),
                None,
            )))
            .is_err());
        assert!(visitor.validate_data_type(&DataType::Boolean).is_ok());
        assert!(visitor
            .validate_data_type(&DataType::Custom(
                ObjectName(vec![
                    ObjectNamePart::Identifier(sqlparser::ast::Ident::new("public")),
                    ObjectNamePart::Identifier(sqlparser::ast::Ident::new("safe")),
                ]),
                Vec::new(),
            ))
            .is_ok());
    }

    #[test]
    fn preserves_dollar_quoted_literals_and_operator_denylists() {
        let engine = SecurityEngine::new(
            SecurityPolicy {
                allowed_schemas: vec!["public".into()],
                denied_relations: vec!["public.custom".into()],
                require_single_statement: false,
                ..SecurityPolicy::default()
            },
            LimitsConfig::default(),
        );
        assert!(engine.check_read_only("SELECT $$--$$; COMMIT").is_err());
        assert!(engine
            .validate("SELECT 1 OPERATOR(public.custom) 1")
            .is_err());
        assert!(engine.validate("SELECT * FROM \"PUBLIC\".secrets").is_err());
        assert!(
            SecurityEngine::strip_sql_comments(r#"SELECT E'abc\'--'; COMMIT"#).contains("COMMIT")
        );
        assert!(
            SecurityEngine::strip_sql_comments("SELECT $tag$/*$tag$; COMMIT").contains("COMMIT")
        );
        assert!(SecurityEngine::strip_sql_comments("SELECT $bad; COMMIT").contains("COMMIT"));
        assert_eq!(
            SecurityEngine::strip_sql_comments("SELECT 'literal'"),
            "SELECT 'literal'"
        );
        assert_eq!(
            SecurityEngine::strip_sql_comments("SELECT \"identifier\""),
            "SELECT \"identifier\""
        );
    }

    #[test]
    fn validates_all_policy_constraints_on_a_valid_query() {
        let policy = SecurityPolicy {
            require_single_statement: true,
            allowed_schemas: vec!["public".into()],
            denied_relations: vec!["public.secrets".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());

        assert!(engine.validate("SELECT * FROM public.orders").is_ok());
    }

    #[test]
    fn system_catalogs_are_denied_even_without_an_allowlist() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .validate_relation_access("pg_catalog", "pg_inherits")
            .is_err());
        assert!(engine
            .validate("SELECT * FROM pg_catalog.pg_inherits")
            .is_err());
        assert!(engine
            .validate("SELECT * FROM information_schema.tables")
            .is_err());
        assert!(engine
            .validate_system("SELECT * FROM information_schema.tables")
            .is_ok());
    }

    #[test]
    fn system_schema_prefixes_are_denied_for_exact_relations() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        for schema in ["pg_toast", "pg_temp_3", "PG_CATALOG"] {
            assert!(
                engine.validate_relation_access(schema, "relation").is_err(),
                "system schema {schema} must be denied"
            );
        }
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
    fn test_document_aggregate_rejects_nested_denied_namespaces() {
        let engine = SecurityEngine::new(
            SecurityPolicy {
                allowed_collections: vec!["app.users".into()],
                denied_collections: vec!["app.secrets".into()],
                ..SecurityPolicy::default()
            },
            LimitsConfig::default(),
        );
        for pipeline in [
            serde_json::json!([{"$lookup": {"from": "secrets", "pipeline": [], "as": "leak"}}]),
            serde_json::json!([{"$unionWith": {"coll": "secrets", "pipeline": []}}]),
            serde_json::json!([{"$graphLookup": {"from": "secrets", "startWith": "$id", "connectFromField": "id", "connectToField": "id", "as": "leak"}}]),
        ] {
            let request = DocumentAggregateRequest {
                database: "app".into(),
                collection: "users".into(),
                pipeline,
                limit: 10,
            };
            assert!(engine.validate_document_aggregate(&request).is_err());
        }
    }

    #[test]
    fn test_document_aggregate_rejects_recursive_write_stages() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        let request = DocumentAggregateRequest {
            database: "app".into(),
            collection: "users".into(),
            pipeline: serde_json::json!([{"$facet": {"writes": [{"$merge": "copy"}]}}]),
            limit: 10,
        };
        assert!(engine.validate_document_aggregate(&request).is_err());
    }

    #[test]
    fn test_document_options_reject_unbounded_resource_requests() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .validate_document_find_arguments(&serde_json::json!({"maxTimeMS": 0}))
            .is_err());
        assert!(engine
            .validate_document_find_arguments(&serde_json::json!({"batchSize": 100_000}))
            .is_err());
        assert!(engine
            .validate_document_aggregate_arguments(&serde_json::json!({"allowDiskUse": true}))
            .is_err());
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
            extract_explain_target("EXPLAIN () SELECT 1"),
            Some("SELECT 1")
        );
        assert_eq!(
            extract_explain_target("EXPLAIN ANALYZE SELECT 1"),
            Some("SELECT 1")
        );
        assert_eq!(
            extract_explain_target("EXPLAIN SUMMARY SELECT 1"),
            Some("SELECT 1")
        );
        for option in ["VERBOSE", "BUFFERS", "SETTINGS", "WAL", "TIMING"] {
            assert_eq!(
                extract_explain_target(&format!("EXPLAIN {option} SELECT 1")),
                Some("SELECT 1")
            );
        }
        assert_eq!(extract_explain_target("EXPLAIN select 1"), Some("select 1"));
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
        assert!(engine.validate_system("SELECT 1").is_ok());
        assert!(engine.validate_system("").is_err());
        assert!(engine.validate_system("DROP TABLE users").is_err());
    }

    #[test]
    fn validates_system_single_statement_and_denied_relation_policies() {
        let policy = SecurityPolicy {
            require_single_statement: true,
            denied_relations: vec!["public.secrets".into()],
            ..SecurityPolicy::default()
        };
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate_system("SELECT * FROM information_schema.tables")
            .is_ok());
        assert!(engine.validate_system("SELECT 1; SELECT 2").is_err());
        assert!(engine
            .validate_system("SELECT * FROM public.secrets")
            .is_err());
        let oversized = "x".repeat(MAX_SQL_BYTES + 1);
        assert!(engine.validate_system(&oversized).is_err());
    }
}
