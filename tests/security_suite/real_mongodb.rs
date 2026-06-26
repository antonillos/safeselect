//! Real MongoDB security regression suite.

use crate::mongodb;
use serde_json::json;

pub fn run() {
    if std::env::var("SAFESELECT_SECURITY_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_SECURITY_TEST=1 to run real MongoDB security tests");
        return;
    }
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("security_mongo_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!("safeselect-mongo-security-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    log_step("starting real MongoDB security suite");
    log_step(&format!("workspace: {}", tmp.display()));
    log_step("setting up MongoDB fixtures");
    mongodb::setup_database();
    log_step("writing SafeSelect test config");
    mongodb::write_config(&repo_root);

    let result = std::panic::catch_unwind(|| {
        log_check("`safeselect check` happy path");
        let (stdout, stderr, success) = mongodb::run_safeselect_args(
            &repo_root,
            &config_dir,
            &["check", "--environment", "testing"],
        );
        assert!(success, "check failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
        assert!(stdout.contains("MongoDB ping succeeded"), "unexpected check output: {stdout}");

        let baseline = database_state();
        log_step(&format!("captured baseline MongoDB state: {:?}", baseline));

        let mut harness = mongodb::McpHarness::start(&repo_root, &config_dir);

        log_check("list_databases is filtered to allowed databases");
        let databases = harness.call_tool(10, "list_databases", json!({}));
        assert!(databases.success, "list_databases failed: {}", databases.text);
        assert!(databases.text.contains(&mongodb::test_db()), "allowed database missing: {}", databases.text);
        assert!(!databases.text.contains("admin"), "unexpected admin database leak: {}", databases.text);

        log_check("list_collections hides denied collections");
        let collections = harness.call_tool(
            11,
            "list_collections",
            json!({ "database": mongodb::test_db() }),
        );
        assert!(collections.success, "list_collections failed: {}", collections.text);
        assert!(collections.text.contains("safe_docs"), "safe_docs missing: {}", collections.text);
        assert!(collections.text.contains("large_docs"), "large_docs missing: {}", collections.text);
        assert!(
            !collections.text.contains("secret_docs"),
            "secret_docs should have been filtered out: {}",
            collections.text
        );

        log_check("find_documents happy path");
        let find = harness.call_tool(
            12,
            "find_documents",
            json!({
                "database": mongodb::test_db(),
                "collection": "safe_docs",
                "filter": { "active": true },
                "sort": { "_id": 1 },
                "limit": 2
            }),
        );
        assert!(find.success, "find_documents failed: {}", find.text);
        assert!(find.text.contains("alpha") && find.text.contains("beta"), "unexpected find result: {}", find.text);

        log_check("aggregate_documents happy path");
        let aggregate = harness.call_tool(
            13,
            "aggregate_documents",
            json!({
                "database": mongodb::test_db(),
                "collection": "safe_docs",
                "pipeline": [
                    { "$match": { "active": true } },
                    { "$sort": { "_id": 1 } }
                ],
                "limit": 2
            }),
        );
        assert!(aggregate.success, "aggregate_documents failed: {}", aggregate.text);
        assert!(aggregate.text.contains("alpha"), "unexpected aggregate result: {}", aggregate.text);

        log_check("explain_documents happy path");
        let explain = harness.call_tool(
            14,
            "explain_documents",
            json!({
                "database": mongodb::test_db(),
                "collection": "safe_docs",
                "filter": { "active": true },
                "limit": 1
            }),
        );
        assert!(explain.success, "explain_documents failed: {}", explain.text);
        assert!(
            explain.text.contains("queryPlanner")
                || explain.text.contains("winningPlan")
                || explain.text.contains("explain"),
            "unexpected explain result: {}",
            explain.text
        );

        for (id, name, tool, args) in [
            (
                20,
                "denied database",
                "list_collections",
                json!({ "database": "admin" }),
            ),
            (
                21,
                "denied collection",
                "find_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "secret_docs",
                    "filter": {},
                    "limit": 1
                }),
            ),
            (
                22,
                "invalid filter",
                "find_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "safe_docs",
                    "filter": "not-an-object",
                    "limit": 1
                }),
            ),
            (
                23,
                "row limit",
                "find_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "safe_docs",
                    "filter": {},
                    "limit": 3
                }),
            ),
            (
                24,
                "aggregate $out",
                "aggregate_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "safe_docs",
                    "pipeline": [
                        { "$match": { "active": true } },
                        { "$out": "evil_copy" }
                    ],
                    "limit": 1
                }),
            ),
            (
                25,
                "aggregate $merge",
                "aggregate_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "safe_docs",
                    "pipeline": [
                        { "$match": { "active": true } },
                        { "$merge": "evil_copy" }
                    ],
                    "limit": 1
                }),
            ),
            (
                26,
                "aggregate $currentOp",
                "aggregate_documents",
                json!({
                    "database": mongodb::test_db(),
                    "collection": "safe_docs",
                    "pipeline": [
                        { "$currentOp": {} }
                    ],
                    "limit": 1
                }),
            ),
        ] {
            assert_rejected(&mut harness, id, name, tool, args, &baseline);
        }

        log_check("byte limit rejection");
        let byte_limit = harness.call_tool(
            27,
            "find_documents",
            json!({
                "database": mongodb::test_db(),
                "collection": "large_docs",
                "filter": {},
                "limit": 1
            }),
        );
        assert!(
            !byte_limit.success,
            "byte limit unexpectedly succeeded: {}",
            byte_limit.text
        );
        assert!(
            byte_limit.text.contains("RESULT_LIMIT_EXCEEDED")
                || byte_limit.text.contains("Result size limit exceeded"),
            "byte limit failed for wrong reason: {}",
            byte_limit.text
        );
        assert_eq!(&database_state(), &baseline, "byte limit changed MongoDB state");

        log_check("timeout rejection is visible");
        let timeout = harness.call_tool(
            28,
            "aggregate_documents",
            json!({
                "database": mongodb::test_db(),
                "collection": "safe_docs",
                "pipeline": [
                    {
                        "$addFields": {
                            "slow": {
                                "$function": {
                                    "body": "function(name) { var start = Date.now(); while (Date.now() - start < 7000) {} return name; }",
                                    "args": ["$name"],
                                    "lang": "js"
                                }
                            }
                        }
                    }
                ],
                "limit": 1
            }),
        );
        assert!(!timeout.success, "timeout scenario unexpectedly succeeded: {}", timeout.text);
        assert!(
            timeout.text.contains("ExecutionTimeout")
                || timeout.text.contains("exceeded time limit")
                || timeout.text.contains("MaxTimeMSExpired")
                || timeout.text.contains("did not respond")
                || timeout.text.contains("stalled output")
                || timeout.text.to_lowercase().contains("timed out"),
            "timeout failed for wrong reason: {}",
            timeout.text
        );
        assert_eq!(&database_state(), &baseline, "timeout changed MongoDB state");

        log_check("baseline remained unchanged after all rejections");
        assert_eq!(database_state(), baseline);
    });

    log_step("cleaning up MongoDB fixtures");
    mongodb::cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn assert_rejected(
    harness: &mut mongodb::McpHarness,
    id: u64,
    name: &str,
    tool: &str,
    args: serde_json::Value,
    baseline: &DatabaseState,
) {
    log_check(&format!("expect rejection: {name}"));
    log_step(&format!("tool={tool} args={args}"));
    let response = harness.call_tool(id, tool, args);
    assert!(!response.success, "{name} unexpectedly succeeded: {}", response.text);
    assert!(
        response.text.contains("Request rejected")
            || response.text.contains("not read-only")
            || response.text.contains("not in the allowed databases list")
            || response.text.contains("denied")
            || response.text.contains("must be a JSON object")
            || response.text.contains("must be between 1 and"),
        "{name} failed for the wrong reason: {}",
        response.text
    );
    assert_eq!(&database_state(), baseline, "{name} changed MongoDB state despite rejection");
    log_step(&format!("confirmed rejection without mutation: {name}"));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseState {
    safe_docs_count: String,
    large_docs_count: String,
    secret_docs_count: String,
    evil_copy_exists: String,
}

fn database_state() -> DatabaseState {
    DatabaseState {
        safe_docs_count: mongodb::collection_count("safe_docs"),
        large_docs_count: mongodb::collection_count("large_docs"),
        secret_docs_count: mongodb::collection_count("secret_docs"),
        evil_copy_exists: mongodb::collection_exists("evil_copy"),
    }
}

fn log_step(message: &str) {
    eprintln!("[security-mongo-real] {message}");
}

fn log_check(message: &str) {
    eprintln!("[check][security-mongo-real] {message}");
}
