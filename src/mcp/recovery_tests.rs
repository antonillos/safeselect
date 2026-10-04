use super::*;

fn unavailable_bastion() -> crate::config::SshConfig {
    toml::from_str("enabled = true\nhost = '127.0.0.1'\nport = 1\n").unwrap()
}

#[test]
fn existing_postgres_route_does_not_require_a_new_bastion_connection() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut request = [0u8; 8];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
        stream.write_all(b"N").unwrap();
    });
    assert!(crate::is_ssh_ready_for_query(
        &unavailable_bastion(),
        &format!("jdbc:postgresql://{address}/synthetic"),
    ));
    peer.join().unwrap();
    assert!(!crate::is_ssh_ready_for_query(
        &unavailable_bastion(),
        "invalid"
    ));
}

#[test]
fn existing_document_route_does_not_require_a_new_bastion_connection() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("mongodb://{}/synthetic", listener.local_addr().unwrap());
    assert!(McpServer::is_document_backend_ready(
        &unavailable_bastion(),
        &url
    ));
    assert!(!McpServer::is_document_backend_ready(
        &unavailable_bastion(),
        "mongodb://127.0.0.1:0/synthetic"
    ));
}

#[test]
fn check_verifies_existing_mcp_backend_even_when_bastion_is_unavailable() {
    let repo_root =
        std::env::temp_dir().join(format!("safeselect-health-{}", uuid::Uuid::new_v4()));
    let environments = repo_root.join(".safeselect/environments");
    std::fs::create_dir_all(&environments).unwrap();
    std::fs::write(environments.join("test.toml"),
        "version = 1\n[database]\nkind = 'document'\nvendor = 'mongodb'\nurl = 'mongodb://127.0.0.1:1/synthetic'\n[ssh]\nenabled = true\nhost = '127.0.0.1'\nport = 1\n"
    ).unwrap();
    let mut server = super::tests::test_server(&repo_root);
    server.backend.kind = BackendKind::Document;
    server.sidecar = Some(crate::sidecar::recovery_tests::mock_sidecar(
        r#"read verification
printf '%s\n' '{"id":0,"ok":{"ok":1.0}}'
read ping
printf '%s\n' '{"id":1,"ok":"pong"}'
read next_request"#,
    ));
    server.handle_check(Some(serde_json::json!(1))).unwrap();
    // The following ping can succeed only if check actually sent verification
    // request 0, rather than returning early on the independent bastion probe.
    server.sidecar_mut().unwrap().ping().unwrap();
    drop(server);
    std::fs::remove_dir_all(repo_root).unwrap();
}

#[test]
fn recognizes_sidecar_transport_failures_for_recovery() {
    assert!(is_sidecar_timeout(
        "sidecar did not respond within the deadline for 'execute'"
    ));
    assert!(is_recoverable_connection_error(
        "sidecar returned an incomplete response"
    ));
    assert!(is_recoverable_connection_error(
        "sidecar returned a mismatched response id"
    ));
}

#[test]
fn jdbc_health_check_requires_the_expected_result() {
    let repo_root =
        std::env::temp_dir().join(format!("safeselect-health-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repo_root).unwrap();
    let mut server = super::tests::test_server(&repo_root);
    server.sidecar = Some(crate::sidecar::recovery_tests::mock_sidecar(
        r#"read request
printf '%s\n' '{"id":0,"ok":{"columns":["connection_test"],"rows":[[1]],"row_count":1,"byte_count":1}}'
read next_request"#,
    ));
    assert_eq!(
        server.verify_sidecar_backend().unwrap(),
        "SELECT 1 returned 1 row"
    );
    server.sidecar = Some(crate::sidecar::recovery_tests::mock_sidecar(
        r#"read request
printf '%s\n' '{"id":0,"ok":{"columns":["connection_test"],"rows":[],"row_count":0,"byte_count":0}}'
read next_request"#,
    ));
    assert!(server
        .verify_sidecar_backend()
        .unwrap_err()
        .to_string()
        .contains("unexpected connection test result"));
    drop(server);
    std::fs::remove_dir_all(repo_root).unwrap();
}

#[test]
fn document_health_check_requires_a_successful_database_ping() {
    let repo_root =
        std::env::temp_dir().join(format!("safeselect-health-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repo_root).unwrap();
    let mut server = super::tests::test_server(&repo_root);
    server.backend.kind = BackendKind::Document;
    for (result, succeeds) in [("{\"ok\":1.0}", true), ("{\"ok\":0}", false), ("{}", false)] {
        server.sidecar = Some(crate::sidecar::recovery_tests::mock_sidecar(&format!(
            "read request\nprintf '%s\\n' '{{\"id\":0,\"ok\":{result}}}'\nread next_request"
        )));
        assert_eq!(server.verify_sidecar_backend().is_ok(), succeeds);
    }
    drop(server);
    std::fs::remove_dir_all(repo_root).unwrap();
}
