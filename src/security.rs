use crate::config::{LimitsConfig, SecurityPolicy};
use crate::error::{Result, SafeselectError};

const MAX_SQL_BYTES: usize = 102_400;

pub struct SecurityEngine {
    policy: SecurityPolicy,
    limits: LimitsConfig,
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

        if self.policy.require_single_statement {
            self.check_single_statement(trimmed)?;
        }

        self.check_read_only(trimmed)?;

        if !self.policy.allowed_schemas.is_empty() {
            self.check_allowed_schemas(trimmed)?;
        }

        if !self.policy.denied_relations.is_empty() {
            self.check_denied_relations(trimmed)?;
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

    fn check_read_only(&self, sql: &str) -> Result<()> {
        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();

        if upper.starts_with("WITH") {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: WITH queries are not allowed".into(),
            ));
        }

        if upper.starts_with("EXPLAIN") {
            return self.check_explain_read_only(trimmed);
        }

        if upper.starts_with("SELECT") {
            self.check_forbidden_tokens(trimmed)?;
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

        let explained_upper = explained_sql.trim_start().to_uppercase();
        if !explained_upper.starts_with("SELECT") {
            return Err(SafeselectError::QueryRejected(
                "Read-only mode: EXPLAIN is only allowed for SELECT statements".into(),
            ));
        }

        self.check_forbidden_tokens(explained_sql)
    }

    fn check_forbidden_tokens(&self, sql: &str) -> Result<()> {
        let compact = sanitize_for_keyword_scan(sql);
        let forbidden = [
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
        ];

        for keyword in forbidden {
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
    for option in ["ANALYZE", "VERBOSE", "BUFFERS", "SETTINGS", "WAL", "TIMING", "SUMMARY"] {
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
            .is_err());
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
        assert!(engine
            .check_read_only("EXPLAIN DELETE FROM users")
            .is_err());
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
        let mut policy = SecurityPolicy::default();
        policy.allowed_schemas = vec!["public".into()];
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine.validate("SELECT * FROM public.users").is_ok());
    }

    #[test]
    fn test_denied_relation() {
        let mut policy = SecurityPolicy::default();
        policy.denied_relations = vec!["public.users_credentials".into()];
        let engine = SecurityEngine::new(policy, LimitsConfig::default());
        assert!(engine
            .validate("SELECT * FROM public.users_credentials")
            .is_err());
    }
}
