use crate::sidecar::QueryResult;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
pub struct MaintenanceMetric {
    pub status: &'static str,
    pub reason: &'static str,
    pub observed: Option<f64>,
    pub threshold: Option<f64>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MaintenanceDiagnostic {
    pub schema: String,
    pub table: String,
    pub relation_type: &'static str,
    pub reltuples: Option<f64>,
    pub live_rows: Option<f64>,
    pub dead_rows: Option<f64>,
    pub changes_since_analyze: Option<f64>,
    pub last_analyze: Option<String>,
    pub last_autoanalyze: Option<String>,
    pub last_vacuum: Option<String>,
    pub last_autovacuum: Option<String>,
    pub autovacuum_enabled: Option<bool>,
    pub analyze_scale_factor: Option<f64>,
    pub analyze_threshold_setting: Option<f64>,
    pub analyze_threshold: Option<f64>,
    pub vacuum_scale_factor: Option<f64>,
    pub vacuum_threshold_setting: Option<f64>,
    pub vacuum_threshold: Option<f64>,
    pub vacuum_max_threshold: Option<f64>,
    pub inserts_since_vacuum: Option<f64>,
    pub vacuum_insert_scale_factor: Option<f64>,
    pub vacuum_insert_threshold_setting: Option<f64>,
    pub vacuum_insert_threshold: Option<f64>,
    pub warnings: Vec<&'static str>,
    pub analyze: MaintenanceMetric,
    pub vacuum: MaintenanceMetric,
}

fn number(row: &[serde_json::Value], index: usize) -> Option<f64> {
    row.get(index)
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
}

fn text(row: &[serde_json::Value], index: usize) -> Option<String> {
    row.get(index)
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

fn boolean(row: &[serde_json::Value], index: usize) -> Option<bool> {
    row.get(index).and_then(|v| v.as_bool())
}

fn metric(
    observed: Option<f64>,
    threshold: Option<f64>,
    reason: &'static str,
) -> MaintenanceMetric {
    match (observed, threshold) {
        (Some(value), Some(limit)) if value.is_finite() && limit.is_finite() => MaintenanceMetric {
            status: if value > limit {
                "threshold_exceeded"
            } else {
                "below_threshold"
            },
            reason,
            observed: Some(value),
            threshold: Some(limit),
        },
        _ => MaintenanceMetric {
            status: "unknown",
            reason: "statistics_unavailable",
            observed,
            threshold,
        },
    }
}

fn metrics_for_relation(
    relkind: &str,
    changes: Option<f64>,
    dead_rows: Option<f64>,
    analyze_threshold: Option<f64>,
    vacuum_threshold: Option<f64>,
) -> (MaintenanceMetric, MaintenanceMetric) {
    if relkind == "p" {
        (
            MaintenanceMetric {
                status: "unknown",
                reason: "partitioned_parent_requires_manual_review",
                observed: changes,
                threshold: analyze_threshold,
            },
            MaintenanceMetric {
                status: "not_applicable",
                reason: "partitioned_parent_has_no_tuples_for_vacuum",
                observed: dead_rows,
                threshold: vacuum_threshold,
            },
        )
    } else {
        (
            metric(changes, analyze_threshold, "changes_since_analyze"),
            metric(dead_rows, vacuum_threshold, "dead_tuples"),
        )
    }
}

fn effective_vacuum_threshold(raw: Option<f64>, max: Option<f64>) -> Option<f64> {
    match (raw, max) {
        (Some(raw), Some(max)) if max >= 0.0 => Some(raw.min(max)),
        (Some(raw), _) => Some(raw),
        _ => None,
    }
}

fn effective_vacuum_insert_threshold(
    raw: Option<f64>,
    scale: Option<f64>,
    reltuples: Option<f64>,
    relpages: Option<f64>,
    relallfrozen: Option<f64>,
) -> Option<f64> {
    let (Some(raw), Some(scale), Some(reltuples), Some(relpages), Some(relallfrozen)) =
        (raw, scale, reltuples, relpages, relallfrozen)
    else {
        return None;
    };
    if !raw.is_finite()
        || !scale.is_finite()
        || !reltuples.is_finite()
        || !relpages.is_finite()
        || !relallfrozen.is_finite()
        || raw < 0.0
        || scale < 0.0
        || reltuples < 0.0
        || relpages <= 0.0
        || relallfrozen < 0.0
    {
        return None;
    }
    let not_frozen_fraction = (1.0 - relallfrozen / relpages).clamp(0.0, 1.0);
    Some(raw + scale * reltuples * not_frozen_fraction)
}

fn vacuum_metric(
    dead_rows: Option<f64>,
    vacuum_threshold: Option<f64>,
    inserts_since_vacuum: Option<f64>,
    vacuum_insert_threshold: Option<f64>,
    insert_vacuum_disabled: bool,
) -> MaintenanceMetric {
    if insert_vacuum_disabled {
        return metric(dead_rows, vacuum_threshold, "dead_tuples");
    }
    let dead = dead_rows
        .zip(vacuum_threshold)
        .filter(|(value, limit)| value.is_finite() && limit.is_finite());
    let inserts = inserts_since_vacuum
        .zip(vacuum_insert_threshold)
        .filter(|(value, limit)| value.is_finite() && limit.is_finite());

    if let Some((value, limit)) = dead.filter(|(value, limit)| value > limit) {
        return MaintenanceMetric {
            status: "threshold_exceeded",
            reason: "dead_tuples",
            observed: Some(value),
            threshold: Some(limit),
        };
    }
    if let Some((value, limit)) = inserts.filter(|(value, limit)| value > limit) {
        return MaintenanceMetric {
            status: "threshold_exceeded",
            reason: "inserts_since_vacuum",
            observed: Some(value),
            threshold: Some(limit),
        };
    }
    if let (Some((dead_value, dead_limit)), Some((_insert_value, _insert_limit))) = (dead, inserts)
    {
        return MaintenanceMetric {
            status: "below_threshold",
            reason: "dead_tuples_and_inserts_since_vacuum",
            observed: Some(dead_value),
            threshold: Some(dead_limit),
        };
    }
    MaintenanceMetric {
        status: "unknown",
        reason: "statistics_unavailable",
        observed: dead_rows.or(inserts_since_vacuum),
        threshold: vacuum_threshold.or(vacuum_insert_threshold),
    }
}

pub fn diagnostics_from_query(result: &QueryResult) -> (Vec<MaintenanceDiagnostic>, bool) {
    let mut diagnostics = Vec::new();
    let total_relations = result
        .rows
        .first()
        .and_then(|row| number(row, 24))
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64);
    for row in &result.rows {
        let server_version = row
            .first()
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)))
            .unwrap_or_default();
        let relkind = row.get(3).and_then(|v| v.as_str()).unwrap_or("");
        let relation_type = if relkind == "p" {
            "partitioned_table"
        } else {
            "table"
        };
        let reltuples = number(row, 4);
        let live_rows = number(row, 5);
        let dead_rows = number(row, 6);
        let changes = number(row, 7);
        let analyze_scale = number(row, 13);
        let analyze_base = number(row, 14);
        let vacuum_scale = number(row, 15);
        let vacuum_base = number(row, 16);
        let autovacuum_enabled = boolean(row, 12);
        let vacuum_max = (server_version >= 180000)
            .then(|| number(row, 17))
            .flatten();
        let inserts_since_vacuum = number(row, 18);
        let relpages = number(row, 19);
        let relallfrozen = number(row, 20);
        let vacuum_insert_scale = number(row, 21);
        let vacuum_insert_base = number(row, 22);
        let analyze_threshold = reltuples
            .zip(analyze_scale)
            .zip(analyze_base)
            .map(|((r, s), b)| b + s * r);
        let vacuum_raw = reltuples
            .zip(vacuum_scale)
            .zip(vacuum_base)
            .map(|((r, s), b)| b + s * r);
        let vacuum_threshold = effective_vacuum_threshold(vacuum_raw, vacuum_max);
        let vacuum_insert_threshold = effective_vacuum_insert_threshold(
            vacuum_insert_base,
            vacuum_insert_scale,
            reltuples,
            relpages,
            relallfrozen,
        );
        let insert_vacuum_disabled = vacuum_insert_base.is_some_and(|value| value < 0.0);
        let (analyze, vacuum) = if relkind == "p" {
            metrics_for_relation(
                relkind,
                changes,
                dead_rows,
                analyze_threshold,
                vacuum_threshold,
            )
        } else {
            (
                metric(changes, analyze_threshold, "changes_since_analyze"),
                vacuum_metric(
                    dead_rows,
                    vacuum_threshold,
                    inserts_since_vacuum,
                    vacuum_insert_threshold,
                    insert_vacuum_disabled,
                ),
            )
        };
        let warnings = if autovacuum_enabled == Some(false) {
            vec!["autovacuum_disabled"]
        } else {
            Vec::new()
        };
        diagnostics.push(MaintenanceDiagnostic {
            schema: text(row, 1).unwrap_or_default(),
            table: text(row, 2).unwrap_or_default(),
            relation_type,
            reltuples,
            live_rows,
            dead_rows,
            changes_since_analyze: changes,
            last_analyze: text(row, 8),
            last_autoanalyze: text(row, 9),
            last_vacuum: text(row, 10),
            last_autovacuum: text(row, 11),
            autovacuum_enabled,
            analyze_scale_factor: analyze_scale,
            analyze_threshold_setting: analyze_base,
            analyze_threshold,
            vacuum_scale_factor: vacuum_scale,
            vacuum_threshold_setting: vacuum_base,
            vacuum_threshold,
            vacuum_max_threshold: vacuum_max,
            inserts_since_vacuum,
            vacuum_insert_scale_factor: vacuum_insert_scale,
            vacuum_insert_threshold_setting: vacuum_insert_base,
            vacuum_insert_threshold,
            warnings,
            analyze,
            vacuum,
        });
    }
    diagnostics.sort_by(|a, b| (&a.schema, &a.table).cmp(&(&b.schema, &b.table)));
    let truncated = total_relations.is_some_and(|total| total > diagnostics.len() as u64);
    (diagnostics, truncated)
}

pub fn empty_payload(server_version_num: i64) -> Option<serde_json::Value> {
    if !is_supported_version(server_version_num) {
        return None;
    }
    Some(serde_json::json!({
        "server_version_num": server_version_num,
        "diagnostics": [],
        "summary": {
            "relations": 0,
            "truncated": false,
            "analyze": {"threshold_exceeded": 0, "below_threshold": 0, "unknown": 0, "not_applicable": 0},
            "vacuum": {"threshold_exceeded": 0, "below_threshold": 0, "unknown": 0, "not_applicable": 0}
        }
    }))
}

pub fn payload_from_query(result: &QueryResult) -> Option<serde_json::Value> {
    let version = result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)))?;
    if !is_supported_version(version) {
        return None;
    }
    let (items, truncated) = diagnostics_from_query(result);
    let status_count = |metric: fn(&MaintenanceDiagnostic) -> &MaintenanceMetric, status: &str| {
        items
            .iter()
            .filter(|item| metric(item).status == status)
            .count()
    };
    let summary = |metric: fn(&MaintenanceDiagnostic) -> &MaintenanceMetric| {
        serde_json::json!({
            "threshold_exceeded": status_count(metric, "threshold_exceeded"),
            "below_threshold": status_count(metric, "below_threshold"),
            "unknown": status_count(metric, "unknown"),
            "not_applicable": status_count(metric, "not_applicable")
        })
    };
    Some(serde_json::json!({
        "server_version_num": version,
        "diagnostics": items,
        "summary": {
            "relations": result.rows.len(),
            "truncated": truncated,
            "analyze": summary(|item| &item.analyze),
            "vacuum": summary(|item| &item.vacuum)
        }
    }))
}

fn is_supported_version(version: i64) -> bool {
    matches!(version / 10_000, 15..=18)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(relkind: &str, changes: Option<f64>, dead: Option<f64>) -> Vec<serde_json::Value> {
        vec![
            serde_json::json!(170000),
            serde_json::json!("public"),
            serde_json::json!("t"),
            serde_json::json!(relkind),
            serde_json::json!(1000),
            serde_json::json!(900),
            dead.map_or(serde_json::Value::Null, |v| serde_json::json!(v)),
            changes.map_or(serde_json::Value::Null, |v| serde_json::json!(v)),
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::Value::Null,
            serde_json::json!(true),
            serde_json::json!(0.1),
            serde_json::json!(50),
            serde_json::json!(0.2),
            serde_json::json!(50),
            serde_json::json!(100000000),
            serde_json::json!(0),
            serde_json::json!(10),
            serde_json::json!(0),
            serde_json::json!(0.2),
            serde_json::json!(100),
            serde_json::json!(false),
            serde_json::json!(1),
        ]
    }

    #[test]
    fn classifies_strictly_above_threshold() {
        let rows = vec![
            row("r", Some(151.0), Some(249.0)),
            row("r", Some(151.0), Some(251.0)),
        ];
        let result = QueryResult {
            columns: vec![],
            rows,
            row_count: 2,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let (items, _) = diagnostics_from_query(&result);
        assert_eq!(items[0].analyze.status, "threshold_exceeded");
        assert!(items[0].warnings.is_empty());
        assert_eq!(items[0].analyze_threshold, Some(150.0));
        assert_eq!(items[0].analyze_threshold_setting, Some(50.0));
        assert_eq!(items[0].vacuum.status, "below_threshold");
        assert_eq!(items[0].vacuum_threshold, Some(250.0));
        assert_eq!(items[0].vacuum_threshold_setting, Some(50.0));
        assert_eq!(items[0].vacuum_insert_threshold, Some(300.0));
        assert_eq!(items[1].vacuum.status, "threshold_exceeded");
    }

    #[test]
    fn insert_threshold_can_trigger_vacuum_without_dead_tuples() {
        let mut values = row("r", Some(0.0), Some(0.0));
        values[18] = serde_json::json!(301);
        let result = QueryResult {
            columns: vec![],
            rows: vec![values],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let (items, _) = diagnostics_from_query(&result);
        assert_eq!(items[0].vacuum.status, "threshold_exceeded");
        assert_eq!(items[0].vacuum.reason, "inserts_since_vacuum");
        assert_eq!(items[0].vacuum.observed, Some(301.0));
        assert_eq!(items[0].vacuum.threshold, Some(300.0));
        assert_eq!(items[0].inserts_since_vacuum, Some(301.0));
    }

    #[test]
    fn disabled_insert_threshold_does_not_hide_dead_tuple_decision() {
        let mut values = row("r", Some(0.0), Some(0.0));
        values[18] = serde_json::json!(10000);
        values[22] = serde_json::json!(-1);
        let result = QueryResult {
            columns: vec![],
            rows: vec![values],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let (items, _) = diagnostics_from_query(&result);
        assert_eq!(items[0].vacuum.status, "below_threshold");
        assert_eq!(items[0].vacuum.reason, "dead_tuples");
    }

    #[test]
    fn zero_vacuum_cap_is_applied_and_negative_disables_cap() {
        assert_eq!(
            effective_vacuum_threshold(Some(100.0), Some(0.0)),
            Some(0.0)
        );
        assert_eq!(
            effective_vacuum_threshold(Some(100.0), Some(-1.0)),
            Some(100.0)
        );
    }

    #[test]
    fn unknown_and_partitioned_states_are_fail_closed() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![row("p", None, None)],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let (items, _) = diagnostics_from_query(&result);
        assert_eq!(items[0].analyze.status, "unknown");
        assert_eq!(items[0].vacuum.status, "not_applicable");
    }

    #[test]
    fn disabled_autovacuum_is_reported_as_a_warning_only() {
        let mut values = row("r", Some(0.0), Some(0.0));
        values[12] = serde_json::json!(false);
        let result = QueryResult {
            columns: vec![],
            rows: vec![values],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let (items, _) = diagnostics_from_query(&result);
        assert_eq!(items[0].warnings, vec!["autovacuum_disabled"]);
        assert_eq!(items[0].analyze.status, "below_threshold");
        assert_eq!(items[0].vacuum.status, "below_threshold");
    }

    #[test]
    fn unsupported_versions_produce_no_payload() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![row("r", Some(1.0), Some(1.0))
                .into_iter()
                .enumerate()
                .map(|(i, v)| if i == 0 { serde_json::json!(140000) } else { v })
                .collect()],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        assert!(payload_from_query(&result).is_none());
    }

    #[test]
    fn postgres_15_is_supported() {
        let mut values = row("r", Some(151.0), Some(251.0));
        values[0] = serde_json::json!(150000);
        let result = QueryResult {
            columns: vec![],
            rows: vec![values],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let payload = payload_from_query(&result).expect("PostgreSQL 15 is supported");
        assert_eq!(payload["server_version_num"], 150000);
    }

    #[test]
    fn payload_contains_diagnostics_and_summary_for_supported_version() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![row("r", Some(151.0), Some(251.0))],
            row_count: 1,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let payload = payload_from_query(&result).expect("supported PostgreSQL version");
        assert_eq!(payload["server_version_num"], 170000);
        assert_eq!(payload["summary"]["relations"], 1);
        assert_eq!(payload["summary"]["analyze"]["threshold_exceeded"], 1);
        assert_eq!(payload["summary"]["vacuum"]["threshold_exceeded"], 1);
    }

    #[test]
    fn payload_marks_catalog_truncation() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![{
                let mut row = row("r", Some(1.0), Some(1.0));
                row[24] = serde_json::json!(2);
                row
            }],
            row_count: 2,
            byte_count: 0,
            elapsed_ms: 0,
            elapsed: String::new(),
        };
        let payload = payload_from_query(&result).expect("supported PostgreSQL version");
        assert_eq!(payload["summary"]["truncated"], true);
    }

    #[test]
    fn empty_supported_payload_preserves_server_version() {
        let payload = empty_payload(160000).expect("supported version");
        assert_eq!(payload["server_version_num"], 160000);
        assert_eq!(payload["summary"]["relations"], 0);
    }
}
