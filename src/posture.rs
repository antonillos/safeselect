use crate::config::ResolvedConfig;
use crate::error::{Result, SafeselectError};
use crate::sidecar::{ResultLimits, SidecarProcess};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub severity: &'static str,
    pub message: String,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub version: u8,
    pub backend: &'static str,
    pub role: String,
    pub database: String,
    pub status: &'static str,
    pub findings: Vec<Finding>,
    pub fingerprint: String,
    pub acknowledged: bool,
}

pub fn inspect(resolved: &ResolvedConfig, config_dir: &Path) -> Result<Report> {
    let driver = posture_target(resolved)?;
    let row = run_posture_query(resolved, driver)?;
    let text = |n: usize| {
        row.get(n)
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string()
    };
    let role = text(0);
    let database = text(1);
    Ok(build_report(resolved, config_dir, &role, &database, &row))
}

fn run_posture_query(
    resolved: &ResolvedConfig,
    driver: &crate::config::DriverConfig,
) -> Result<Vec<serde_json::Value>> {
    let mut sidecar = SidecarProcess::start_with_timeout(
        &driver.path,
        &driver.class,
        &resolved.environment.database.url,
        &resolved.environment.database.username,
        &resolved.password,
        0,
        resolved.project.limits.statement_timeout_ms,
        ResultLimits {
            max_rows: 20,
            max_result_bytes: 32 * 1024,
        },
        false,
    )?;
    let query = "SELECT current_user, current_database(), (SELECT rolsuper FROM pg_roles WHERE rolname=current_user), (SELECT rolbypassrls FROM pg_roles WHERE rolname=current_user), EXISTS (SELECT 1 FROM pg_class c WHERE c.relkind IN ('r','p') AND has_table_privilege(c.oid, 'INSERT,UPDATE,DELETE,TRUNCATE')), COALESCE((SELECT ssl FROM pg_stat_ssl WHERE pid=pg_backend_pid()), false)";
    let result = sidecar.execute(query)?;
    sidecar.shutdown()?;
    Ok(result
        .rows
        .first()
        .cloned()
        .ok_or_else(|| SafeselectError::Sidecar("Posture query returned no row".into()))?)
}

fn posture_target(resolved: &ResolvedConfig) -> Result<&crate::config::DriverConfig> {
    let driver = resolved.driver.as_ref().ok_or_else(|| {
        SafeselectError::Config(
            "posture currently supports only JDBC PostgreSQL environments".into(),
        )
    })?;
    let vendor = resolved.environment.database.vendor();
    if vendor.eq_ignore_ascii_case("postgresql") || vendor.eq_ignore_ascii_case("postgres") {
        Ok(driver)
    } else {
        Err(SafeselectError::Config(
            "posture currently supports PostgreSQL only".into(),
        ))
    }
}

fn build_report(
    resolved: &ResolvedConfig,
    config_dir: &Path,
    role: &str,
    database: &str,
    row: &[serde_json::Value],
) -> Report {
    let findings = collect_findings(resolved, row);
    let fingerprint = fingerprint(
        role,
        database,
        &resolved.environment.database.url,
        &findings,
    );
    let acknowledged = acknowledgement_exists(config_dir, &fingerprint);
    let status = report_status(&findings, acknowledged);
    Report {
        version: 1,
        backend: "postgresql",
        role: role.to_string(),
        database: database.to_string(),
        status,
        findings,
        fingerprint,
        acknowledged,
    }
}

fn collect_findings(resolved: &ResolvedConfig, row: &[serde_json::Value]) -> Vec<Finding> {
    let boolean = |n: usize| row.get(n).and_then(|v| v.as_bool()).unwrap_or(false);
    let mut findings = role_findings(&boolean);
    findings.extend(control_findings(resolved, &boolean));
    findings
}

fn role_findings(boolean: &impl Fn(usize) -> bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let role_warnings = [
        (2, "POSTURE_ROLE_SUPERUSER", "Effective role is a PostgreSQL superuser; SafeSelect remains the enforcement boundary."),
        (3, "POSTURE_ROLE_BYPASS_RLS", "Effective role can bypass row-level security; SafeSelect remains the enforcement boundary."),
        (4, "POSTURE_ROLE_WRITE", "Effective role has write privileges; SafeSelect read-only enforcement is active."),
    ];
    for (index, code, message) in role_warnings {
        if boolean(index) {
            findings.push(Finding {
                code,
                severity: "warning",
                message: message.into(),
            });
        }
    }
    findings
}

fn control_findings(resolved: &ResolvedConfig, boolean: &impl Fn(usize) -> bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let remote = !jdbc_hosts(&resolved.environment.database.url)
        .is_some_and(|hosts| !hosts.is_empty() && hosts.iter().all(|host| is_loopback_host(host)));
    if remote && !boolean(5) {
        findings.push(Finding {
            code: "POSTURE_TLS_REQUIRED",
            severity: "critical",
            message: "Remote PostgreSQL connection is not using TLS.".into(),
        });
    }
    if !resolved.project.audit.enabled {
        findings.push(Finding {
            code: "POSTURE_AUDIT_DISABLED",
            severity: "critical",
            message: "SafeSelect audit is disabled.".into(),
        });
    }
    if resolved.project.limits.statement_timeout_ms == 0
        || resolved.project.limits.max_rows == 0
        || resolved.project.limits.max_result_bytes == 0
    {
        findings.push(Finding {
            code: "POSTURE_LIMITS_UNSAFE",
            severity: "critical",
            message: "SafeSelect requires non-zero timeout, row, and byte limits.".into(),
        });
    }
    findings
}

fn jdbc_hosts(url: &str) -> Option<Vec<&str>> {
    let (_, remainder) = url.split_once("://")?;
    let authority = remainder.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, value)| value);
    let hosts = authority.split(',').filter_map(endpoint_host).collect();
    Some(hosts)
}

fn endpoint_host(endpoint: &str) -> Option<&str> {
    let endpoint = endpoint.trim();
    if let Some(bracketed) = endpoint.strip_prefix('[') {
        return bracketed.split_once(']').map(|(host, _)| host);
    }
    let host = endpoint.rsplit_once(':').map_or(endpoint, |(host, _)| host);
    (!host.is_empty()).then_some(host)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn report_status(findings: &[Finding], acknowledged: bool) -> &'static str {
    if findings.iter().any(|f| f.severity == "critical") {
        "unsafe"
    } else if findings.is_empty() {
        "safe"
    } else if acknowledged {
        "accepted"
    } else {
        "warning"
    }
}

fn fingerprint(role: &str, database: &str, url: &str, findings: &[Finding]) -> String {
    let mut h = Sha256::new();
    h.update(role);
    h.update(database);
    h.update(url);
    for f in findings {
        h.update(f.code);
    }
    hex::encode(h.finalize())
}
fn acknowledgement_exists(dir: &Path, fingerprint: &str) -> bool {
    std::fs::read_to_string(dir.join("posture-acknowledgements.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .is_some_and(|items| items.iter().any(|item| item == fingerprint))
}
pub fn acknowledge(dir: &Path, fingerprint: &str) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("posture-acknowledgements.json");
    let mut items = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();
    if !items.iter().any(|item| item == fingerprint) {
        items.push(fingerprint.to_string());
        write_acknowledgements(&path, &items)?;
    }
    Ok(())
}

fn write_acknowledgements(path: &Path, items: &[String]) -> Result<()> {
    let bytes = serde_json::to_vec(items).map_err(|e| SafeselectError::Other(e.to_string()))?;
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EnvironmentConfig, ProjectConfig};

    fn resolved(url: &str) -> ResolvedConfig {
        ResolvedConfig {
            project: ProjectConfig::default(),
            environment: EnvironmentConfig {
                version: 1,
                database: crate::config::DatabaseConfig {
                    kind: crate::backend::BackendKind::Jdbc,
                    vendor: Some("postgresql".into()),
                    driver: Some("postgresql".into()),
                    url: url.into(),
                    username: "reader".into(),
                    secret: None,
                },
                tls: None,
                ssh: None,
                limits: Default::default(),
            },
            driver: None,
            password: String::new(),
            repo_root: Path::new(".").into(),
        }
    }

    #[test]
    fn jdbc_locality_uses_the_parsed_authority_host() {
        assert!(super::jdbc_hosts("jdbc:postgresql://localhost:5432/app")
            .is_some_and(|hosts| hosts.iter().all(|host| super::is_loopback_host(host))));
        assert!(super::jdbc_hosts("jdbc:postgresql://[::1]:5432/app")
            .is_some_and(|hosts| hosts.iter().all(|host| super::is_loopback_host(host))));
        assert!(
            !super::jdbc_hosts("jdbc:postgresql://[::1]:5432,db.example:5432/app")
                .is_some_and(|hosts| hosts.iter().all(|host| super::is_loopback_host(host)))
        );
        assert!(
            !super::jdbc_hosts("jdbc:postgresql://db-localhost.example/app")
                .is_some_and(|hosts| hosts.iter().all(|host| super::is_loopback_host(host)))
        );
        assert!(
            !super::jdbc_hosts("jdbc:postgresql://db.example/app?note=127.0.0.1")
                .is_some_and(|hosts| hosts.iter().all(|host| super::is_loopback_host(host)))
        );
    }

    #[test]
    fn fingerprint_is_deterministic_and_acknowledgement_stores_only_hash() {
        let report = build_report(
            &resolved("postgresql://db/app"),
            Path::new("/tmp"),
            "reader",
            "app",
            &[
                serde_json::json!("reader"),
                serde_json::json!("app"),
                serde_json::json!(true),
                serde_json::json!(false),
                serde_json::json!(true),
                serde_json::json!(true),
            ],
        );
        let second = build_report(
            &resolved("postgresql://db/app"),
            Path::new("/tmp"),
            "reader",
            "app",
            &[
                serde_json::json!("reader"),
                serde_json::json!("app"),
                serde_json::json!(true),
                serde_json::json!(false),
                serde_json::json!(true),
                serde_json::json!(true),
            ],
        );
        assert_eq!(report.fingerprint, second.fingerprint);
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "POSTURE_ROLE_SUPERUSER"));
    }

    #[test]
    fn acknowledgement_round_trip_contains_only_the_digest() {
        let dir = std::env::temp_dir().join(format!("safeselect-posture-{}", uuid::Uuid::new_v4()));
        acknowledge(&dir, "deadbeef").unwrap();
        assert!(acknowledgement_exists(&dir, "deadbeef"));
        let saved = std::fs::read_to_string(dir.join("posture-acknowledgements.json")).unwrap();
        assert_eq!(saved, "[\"deadbeef\"]");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.join("posture-acknowledgements.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn posture_target_rejects_missing_driver_and_non_postgres_vendor() {
        let config = resolved("postgresql://db/app");
        assert!(posture_target(&config).is_err());
        let mut with_driver = config;
        with_driver.driver = Some(crate::config::DriverConfig {
            version: 1,
            vendor: "postgresql".into(),
            path: "/tmp/driver.jar".into(),
            class: "org.postgresql.Driver".into(),
            sha256: "00".into(),
        });
        assert!(posture_target(&with_driver).is_ok());
    }

    #[test]
    fn inspect_fails_before_connecting_without_driver() {
        let config = resolved("postgresql://db/app");
        let error = inspect(&config, Path::new("/tmp")).unwrap_err();
        assert!(error.to_string().contains("JDBC PostgreSQL"));
    }

    #[test]
    fn inspect_propagates_sidecar_start_failure() {
        let mut config = resolved("postgresql://db/app");
        config.driver = Some(crate::config::DriverConfig {
            version: 1,
            vendor: "postgresql".into(),
            path: "/missing/driver.jar".into(),
            class: "org.postgresql.Driver".into(),
            sha256: "00".into(),
        });
        assert!(inspect(&config, Path::new("/tmp")).is_err());
    }

    #[test]
    fn posture_query_fails_closed_when_driver_cannot_start() {
        let config = resolved("postgresql://127.0.0.1/app");
        let driver = crate::config::DriverConfig {
            version: 1,
            vendor: "postgresql".into(),
            path: "/missing/driver.jar".into(),
            class: "org.postgresql.Driver".into(),
            sha256: "00".into(),
        };
        assert!(run_posture_query(&config, &driver).is_err());
    }

    #[test]
    fn build_report_covers_control_failures_and_clean_posture() {
        let mut config = resolved("postgresql://db.internal/app");
        config.project.audit.enabled = false;
        config.project.limits.statement_timeout_ms = 0;
        config.project.limits.max_rows = 0;
        config.project.limits.max_result_bytes = 0;
        let report = build_report(
            &config,
            Path::new("/tmp"),
            "reader",
            "app",
            &[
                serde_json::json!("reader"),
                serde_json::json!("app"),
                serde_json::json!(false),
                serde_json::json!(true),
                serde_json::json!(false),
                serde_json::json!(false),
            ],
        );
        assert_eq!(report.status, "unsafe");
        assert!(report
            .findings
            .iter()
            .any(|f| f.code == "POSTURE_TLS_REQUIRED"));
    }

    #[test]
    fn report_status_covers_safe_and_accepted_warning() {
        assert_eq!(report_status(&[], false), "safe");
        let warning = Finding {
            code: "X",
            severity: "warning",
            message: String::new(),
        };
        assert_eq!(
            report_status(std::slice::from_ref(&warning), false),
            "warning"
        );
        assert_eq!(
            report_status(std::slice::from_ref(&warning), true),
            "accepted"
        );
    }
}
