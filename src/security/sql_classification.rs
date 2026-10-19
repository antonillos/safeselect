use super::canonical_ident;
use crate::error::{Result, SafeselectError};
use sqlparser::ast::{
    Expr, ObjectName, ObjectNamePart, Query, SetExpr, Statement, TableFactor, Visit, Visitor,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};
use std::ops::ControlFlow;

// Command names are checked only at statement boundaries, never inside identifiers.
const STATE_CHANGING_COMMANDS: &[&str] = &[
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "REPLACE",
    "CREATE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "COMMENT",
    "SECURITY",
    "GRANT",
    "REVOKE",
    "REASSIGN",
    "IMPORT",
    "COPY",
    "DO",
    "CALL",
    "PREPARE",
    "EXECUTE",
    "DEALLOCATE",
    "SET",
    "RESET",
    "DISCARD",
    "BEGIN",
    "START",
    "COMMIT",
    "END",
    "ROLLBACK",
    "ABORT",
    "SAVEPOINT",
    "RELEASE",
    "LOCK",
    "VACUUM",
    "ANALYZE",
    "ANALYSE",
    "REINDEX",
    "CLUSTER",
    "REFRESH",
    "CHECKPOINT",
    "LOAD",
    "LISTEN",
    "UNLISTEN",
    "NOTIFY",
    "DECLARE",
    "CLOSE",
    "MOVE",
    "FETCH",
];
const STATE_CHANGING_FUNCTIONS: &[&str] = &[
    "set_config",
    "setseed",
    "pg_notify",
    "pg_logical_emit_message",
    "pg_switch_wal",
    "pg_promote",
    "pg_wal_replay_pause",
    "pg_wal_replay_resume",
    "pg_replication_slot_advance",
    "setval",
    "nextval",
    "pg_advisory_lock",
    "pg_advisory_xact_lock",
    "pg_advisory_lock_shared",
    "pg_advisory_xact_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_xact_lock",
    "pg_try_advisory_lock_shared",
    "pg_try_advisory_xact_lock_shared",
    "pg_advisory_unlock",
    "pg_advisory_unlock_shared",
    "pg_advisory_unlock_all",
    "pg_create_physical_replication_slot",
    "pg_create_logical_replication_slot",
    "pg_drop_replication_slot",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_start_backup",
    "pg_stop_backup",
    "pg_backup_start",
    "pg_backup_stop",
    "lo_import",
    "lo_export",
    "lo_unlink",
    "lo_create",
    "lo_from_bytea",
    "lo_put",
    "lowrite",
    "lo_truncate",
    "lo_truncate64",
];
const SLEEP_FUNCTIONS: &[&str] = &["pg_sleep", "pg_sleep_for", "pg_sleep_until"];

fn rejected() -> SafeselectError {
    SafeselectError::QueryRejected("Read-only mode: unsupported or invalid SQL".into())
}
fn serious(operation: &str) -> SafeselectError {
    SafeselectError::Security(format!("Read-only mode: {operation} not allowed"))
}

pub(super) fn tokens(sql: &str) -> Result<Vec<Token>> {
    Tokenizer::new(&PostgreSqlDialect {}, sql)
        .tokenize()
        .map(|tokens| {
            tokens
                .into_iter()
                .filter(|token| !matches!(token, Token::Whitespace(_)))
                .collect()
        })
        .map_err(|_| rejected())
}

pub(super) fn validate(sql: &str) -> Result<()> {
    let tokens = tokens(sql)?;
    if tokens.iter().all(|token| *token == Token::SemiColon) {
        return Err(rejected());
    }
    let mut recovery = None;
    for statement in tokens
        .split(|token| *token == Token::SemiColon)
        .filter(|s| !s.is_empty())
    {
        match validate_statement(statement) {
            Err(error @ SafeselectError::Security(_)) => return Err(error),
            Err(error) => recovery = Some(error),
            Ok(()) => {}
        }
    }
    recovery.map_or(Ok(()), Err)
}

fn validate_statement(tokens: &[Token]) -> Result<()> {
    check_command(tokens)?;
    let statements = match parse_statement(tokens) {
        Ok(statements) => statements,
        Err(_) => {
            check_explain_command(tokens)?;
            return Err(rejected());
        }
    };
    let mut visitor = ReadOnlyVisitor { recovery: None };
    if let ControlFlow::Break(error) = statements.visit(&mut visitor) {
        return Err(error);
    }
    visitor.recovery.map_or(Ok(()), Err)
}

fn parse_statement(tokens: &[Token]) -> Result<Vec<Statement>> {
    let mut parser = Parser::new(&PostgreSqlDialect {}).with_tokens(parser_tokens(tokens));
    // sqlparser accepts TABLE as a query body but not as a top-level statement.
    if matches!(tokens.first(), Some(Token::Word(word)) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("TABLE"))
    {
        let query = parser.parse_query().map_err(|_| rejected())?;
        if parser.next_token().token != Token::EOF {
            return Err(rejected());
        }
        return Ok(vec![Statement::Query(query)]);
    }
    parser.parse_statements().map_err(|_| rejected())
}

fn parser_tokens(tokens: &[Token]) -> Vec<Token> {
    let mut normalized = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while index < tokens.len() {
        // sqlparser only models UPDATE and SHARE locks. Preserve their severity
        // for PostgreSQL's weaker forms without changing the SQL sent to JDBC.
        let width = if keyword_sequence(&tokens[index..], &["FOR", "KEY", "SHARE"]) {
            3
        } else if keyword_sequence(&tokens[index..], &["FOR", "NO", "KEY", "UPDATE"]) {
            4
        } else {
            1
        };
        normalized.push(tokens[index].clone());
        if width > 1 {
            normalized.push(tokens[index + width - 1].clone());
        }
        index += width;
    }
    normalized
}

fn keyword_sequence(tokens: &[Token], words: &[&str]) -> bool {
    tokens.len() >= words.len() && tokens.iter().zip(words).all(|(token, expected)| {
        matches!(token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case(expected))
    })
}

fn check_explain_command(tokens: &[Token]) -> Result<()> {
    if !matches!(tokens.first(), Some(Token::Word(word)) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("EXPLAIN"))
    {
        return Ok(());
    }
    let mut index = 1;
    if tokens.get(index) == Some(&Token::LParen) {
        let mut depth = 0;
        for token in &tokens[index..] {
            index += 1;
            match token {
                Token::LParen => depth += 1,
                Token::RParen => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
    } else {
        while matches!(tokens.get(index), Some(Token::Word(word)) if word.quote_style.is_none() && ["ANALYZE", "VERBOSE"].contains(&word.value.to_ascii_uppercase().as_str()))
        {
            index += 1;
        }
    }
    check_command(&tokens[index..])
}

fn check_command(tokens: &[Token]) -> Result<()> {
    if let Some(Token::Word(word)) = tokens.first() {
        let command = word.value.to_ascii_uppercase();
        if word.quote_style.is_none() && STATE_CHANGING_COMMANDS.contains(&command.as_str()) {
            return Err(serious(&command));
        }
    }
    Ok(())
}

struct ReadOnlyVisitor {
    recovery: Option<SafeselectError>,
}

impl ReadOnlyVisitor {
    fn function(&mut self, name: &ObjectName) -> ControlFlow<SafeselectError> {
        if let Some(ObjectNamePart::Identifier(ident)) = name.0.last() {
            let name = canonical_ident(ident);
            if STATE_CHANGING_FUNCTIONS.contains(&name.as_str()) {
                return ControlFlow::Break(serious(&format!(
                    "function {}",
                    name.to_ascii_uppercase()
                )));
            }
            if SLEEP_FUNCTIONS.contains(&name.as_str()) {
                self.recovery = Some(SafeselectError::QueryRejected(format!(
                    "Read-only mode: function {} not allowed",
                    name.to_ascii_uppercase()
                )));
            }
        }
        ControlFlow::Continue(())
    }
}

impl Visitor for ReadOnlyVisitor {
    type Break = SafeselectError;

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        match statement {
            Statement::Query(query) => {
                if matches!(query.body.as_ref(), SetExpr::Values(_)) {
                    self.recovery = Some(rejected());
                }
            }
            Statement::Explain { .. } => {}
            _ => {
                if let Err(error) =
                    check_command(&tokens(&statement.to_string()).unwrap_or_default())
                {
                    return ControlFlow::Break(error);
                }
                self.recovery = Some(rejected());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if !query.locks.is_empty() {
            return ControlFlow::Break(serious("row locking"));
        }
        check_set_expression(&query.body)
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(function) = expr {
            return self.function(&function.name);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Self::Break> {
        match factor {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } => self.function(name),
            _ => ControlFlow::Continue(()),
        }
    }
}

fn check_set_expression(body: &SetExpr) -> ControlFlow<SafeselectError> {
    match body {
        SetExpr::Select(select) if select.into.is_some() => {
            ControlFlow::Break(serious("SELECT INTO"))
        }
        SetExpr::SetOperation { left, right, .. } => {
            check_set_expression(left)?;
            check_set_expression(right)
        }
        _ => ControlFlow::Continue(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_classification_uses_exact_call_names_in_every_context() {
        for function in STATE_CHANGING_FUNCTIONS {
            for call in [
                format!("{function}()"),
                format!("\"pg_catalog\".\"{function}\"()"),
                format!("{}()", function.to_ascii_uppercase()),
            ] {
                for sql in [
                    format!("SELECT {call}"),
                    format!("SELECT * FROM {call}"),
                    format!("SELECT * FROM LATERAL {call}"),
                    format!("WITH q AS (SELECT {call}) SELECT * FROM q"),
                    format!("EXPLAIN (ANALYZE false) SELECT {call}"),
                    format!("SELECT pg_sleep(1), {call}"),
                ] {
                    assert!(
                        matches!(validate(&sql), Err(SafeselectError::Security(_))),
                        "{sql}"
                    );
                }
            }
            for sql in [
                format!("SELECT {function}_count FROM public.metrics"),
                format!("SELECT public.{function}_count()"),
                format!("SELECT '{function}()'"),
                format!("WITH {function}(value) AS (SELECT 1) SELECT * FROM {function}"),
            ] {
                assert!(validate(&sql).is_ok(), "{sql}");
            }
        }
    }

    #[test]
    fn syntax_failures_do_not_hide_later_serious_statements() {
        assert!(matches!(
            validate(";;;"),
            Err(SafeselectError::QueryRejected(_))
        ));
        for sql in [
            "SELECT (; COMMIT",
            "SHOW ALL; SELECT nextval('public.seq')",
            "SELECT pg_sleep(1); SELECT 1 INTO public.example",
            "EXPLAIN (ANALYZE false) DISCARD ALL",
            "EXPLAIN ANALYZE VERBOSE DISCARD ALL",
        ] {
            assert!(
                matches!(validate(sql), Err(SafeselectError::Security(_))),
                "{sql}"
            );
        }
        for sql in [
            ";;;",
            "EXPLAIN (",
            "SELECT 'unterminated",
            "EXPLAIN ANALYZE VERBOSE SELECT 1",
            "SELECT $tag$; COMMIT$tag$",
        ] {
            assert!(
                !matches!(validate(sql), Err(SafeselectError::Security(_))),
                "{sql}"
            );
        }
    }
}
