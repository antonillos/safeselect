//! Real PostgreSQL security regression suite.

use crate::postgres;
use std::path::Path;

pub fn run() {
    if std::env::var("SAFESELECT_SECURITY_TEST").is_err() {
        eprintln!("Skipping: set SAFESELECT_SECURITY_TEST=1 to run real PostgreSQL security tests");
        return;
    }
    std::env::set_var(
        "SAFESELECT_TEST_SUFFIX",
        format!("security_{}", std::process::id()),
    );

    let tmp = std::env::temp_dir().join(format!("safeselect-security-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let repo_root = tmp.join("repo");
    let config_dir = tmp.join("config");
    std::fs::create_dir_all(&repo_root).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();

    postgres::setup_database();
    postgres::write_config(&repo_root);
    postgres::download_driver(&config_dir);

    let result = std::panic::catch_unwind(|| {
        let (stdout, stderr, success) =
            postgres::run_safeselect(&repo_root, &config_dir, "SELECT 1 AS ok");
        assert!(
            success,
            "SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(stdout.contains("| 1"), "unexpected SELECT output: {stdout}");

        let (stdout, stderr, success) = postgres::run_safeselect(
            &repo_root,
            &config_dir,
            "EXPLAIN SELECT id FROM public.safe_table WHERE id = 1",
        );
        assert!(
            success,
            "EXPLAIN SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("Seq Scan")
                || stdout.contains("Index Scan")
                || stdout.contains("QUERY PLAN"),
            "unexpected EXPLAIN output: {stdout}"
        );

        let (stdout, stderr, success) = postgres::run_safeselect(
            &repo_root,
            &config_dir,
            "SELECT * FROM unnest(ARRAY[10, 20]) WITH ORDINALITY AS t(value, ord)",
        );
        assert!(
            success,
            "WITH ORDINALITY SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("| 10    | 1") || stdout.contains("| 10 | 1   |"),
            "unexpected WITH ORDINALITY output: {stdout}"
        );

        let (stdout, stderr, success) = postgres::run_safeselect(
                &repo_root,
                &config_dir,
                "WITH filtered AS (SELECT id, name FROM public.safe_table WHERE id <= 2) SELECT * FROM filtered ORDER BY id",
            );
        assert!(
            success,
            "WITH SELECT failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("alpha") && stdout.contains("beta"),
            "unexpected WITH SELECT output: {stdout}"
        );

        for (name, sql) in [
            ("DELETE", "DELETE FROM public.safe_table WHERE id = 1"),
            (
                "UPDATE",
                "UPDATE public.safe_table SET name = 'changed' WHERE id = 1",
            ),
            (
                "INSERT",
                "INSERT INTO public.safe_table VALUES (4, 'delta', 'd')",
            ),
            ("DROP", "DROP TABLE public.safe_table"),
            (
                "WITH delete",
                "WITH deleted AS (DELETE FROM public.safe_table RETURNING *) SELECT * FROM deleted",
            ),
            (
                "WITH final delete",
                "WITH x AS (SELECT 1) DELETE FROM public.safe_table",
            ),
            (
                "EXPLAIN DELETE",
                "EXPLAIN DELETE FROM public.safe_table WHERE id = 1",
            ),
            (
                "EXPLAIN ANALYZE SELECT",
                "EXPLAIN ANALYZE SELECT * FROM public.safe_table",
            ),
            (
                "EXPLAIN ANALYZE DELETE",
                "EXPLAIN ANALYZE DELETE FROM public.safe_table WHERE id = 1",
            ),
            ("denied relation", "SELECT * FROM public.secret_table"),
            (
                "session change",
                "SELECT set_config('role', 'postgres', false)",
            ),
            ("sleep", "SELECT pg_sleep(5)"),
        ] {
            assert_rejected(&repo_root, &config_dir, name, sql);
        }

        assert_rejected(
            &repo_root,
            &config_dir,
            "row limit",
            "SELECT id FROM public.safe_table ORDER BY id",
        );
        assert_rejected(
            &repo_root,
            &config_dir,
            "byte limit",
            "SELECT payload FROM public.large_payload WHERE id = 1",
        );

        let counts = postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) || ':' || string_agg(name, ',' ORDER BY id) FROM public.safe_table;",
        );
        assert_eq!(counts, "3:alpha,beta,gamma");
        let secret_count = postgres::psql(
            &postgres::test_db(),
            "SELECT count(*) FROM public.secret_table;",
        );
        assert_eq!(secret_count, "1");
    });

    postgres::cleanup_database();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

fn assert_rejected(root: &Path, config_dir: &Path, name: &str, sql: &str) {
    let (stdout, stderr, success) = postgres::run_safeselect(root, config_dir, sql);
    assert!(
        !success,
        "{name} unexpectedly succeeded\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Query rejected")
            || stderr.contains("Read-only mode")
            || stderr.contains("RESULT_LIMIT_EXCEEDED"),
        "{name} failed for the wrong reason\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
