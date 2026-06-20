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
        let upper = sql.trim().to_uppercase();

        if upper.starts_with("SELECT") || upper.starts_with("EXPLAIN") || upper.starts_with("WITH")
        {
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
    fn test_read_only_with_cte() {
        let engine = SecurityEngine::new(SecurityPolicy::default(), LimitsConfig::default());
        assert!(engine
            .check_read_only("WITH x AS (SELECT 1) SELECT * FROM x")
            .is_ok());
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
