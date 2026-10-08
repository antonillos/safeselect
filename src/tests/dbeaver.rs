use super::*;

fn sample_connection() -> DBeaverConnection {
    parse_data_sources(r#"{"connections":[{"name":"staging","host":"db.example","database":"app","driver":"postgres","username":"demo","password":"fixture"}]}"#).unwrap().remove(0)
}

#[test]
fn identity_ignores_names_and_passwords_but_distinguishes_endpoints_and_bastions() {
    let mut conn = sample_connection();
    let key = fingerprint(&conn);
    conn.password = Some("changed".into());
    conn.name = "renamed".into();
    assert_eq!(key, fingerprint(&conn));
    conn.ssh_port = Some(22);
    assert_eq!(key, fingerprint(&conn));
    conn.host = "other.example".into();
    assert_ne!(key, fingerprint(&conn));
    conn.host = "db.example".into();
    conn.ssh_host = Some("bastion.example".into());
    assert_ne!(key, fingerprint(&conn));
}

#[test]
fn jdbc_url_preserves_sslmode_and_uses_the_tunnel_endpoint() {
    let mut conn = sample_connection();
    conn.sslmode = Some("verify-full".into());
    assert_eq!(
        database_url(&conn, None),
        "jdbc:postgresql://db.example:5432/app?sslmode=verify-full"
    );
    let ssh = crate::dbeaver_ssh_config(&conn, "project", "staging");
    assert_eq!(
        database_url(&conn, Some(&ssh)),
        "jdbc:postgresql://localhost:15432/app?sslmode=verify-full"
    );
}

#[test]
fn legacy_matching_rejects_other_users_backends_targets_and_bastions() {
    let conn = sample_connection();
    let mut env: crate::config::EnvironmentConfig = toml::from_str("version=1\n[database]\nkind='jdbc'\ndriver='postgresql'\nurl='jdbc:postgresql://db.example:5432/app'\nusername='demo'\n").unwrap();
    assert!(legacy_match(&conn, &env));
    env.database.username = "other".into();
    assert!(!legacy_match(&conn, &env));
    env.database.username = "demo".into();
    env.database.kind = crate::backend::BackendKind::Document;
    assert!(!legacy_match(&conn, &env));
    env.database.kind = crate::backend::BackendKind::Jdbc;
    env.database.url = "jdbc:postgresql://other.example:5432/app".into();
    assert!(!legacy_match(&conn, &env));
    let mut conn = conn;
    conn.ssh_host = Some("bastion.example".into());
    conn.ssh_user = Some("demo".into());
    let ssh = crate::dbeaver_ssh_config(&conn, "project", "staging");
    env.database.url = database_url(&conn, Some(&ssh));
    env.ssh = Some(ssh);
    assert!(legacy_match(&conn, &env));
    env.ssh.as_mut().unwrap().host = Some("other.example".into());
    assert!(!legacy_match(&conn, &env));
}

#[test]
fn provenance_retains_custom_names_without_plaintext_passwords() {
    let root = std::env::temp_dir().join(format!("dbeaver-provenance-{}", uuid::Uuid::new_v4()));
    let envs = root.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    std::fs::write(envs.join("custom.toml"), "not a legacy match").unwrap();
    let conn = sample_connection();
    let mut index = crate::compass_import::ImportIndex::default();
    index.record_key(fingerprint(&conn), "custom");
    let path = root.join("dbeaver-imports.toml");
    index.save_file(&path).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("fixture"));
    let index = crate::compass_import::ImportIndex::load_file(&path).unwrap();
    assert_eq!(
        candidates(
            &envs,
            &index,
            &conn,
            &crate::config::ProjectConfig::default()
        ),
        vec!["custom"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalizes_postgres_driver_aliases() {
    assert_eq!(normalize_driver("postgres"), "postgresql");
    assert_eq!(normalize_driver("POSTGRES-JDBC"), "postgresql");
    assert_eq!(normalize_driver("mysql"), "mysql");
}

#[test]
fn converts_connection_lists_and_maps_to_vectors() {
    let list = ConnectionsField::List(vec![]).into_vec();
    let map = ConnectionsField::Map(HashMap::new()).into_vec();
    assert!(list.is_empty());
    assert!(map.is_empty());
}

#[test]
fn parses_postgres_jdbc_url_with_optional_port() {
    let parsed = parse_postgres_jdbc_url("jdbc:postgresql://db.example:5433/app").unwrap();
    assert_eq!(parsed.host, "db.example");
    assert_eq!(parsed.port, 5433);
    assert_eq!(parsed.database, "app");
    assert!(parse_postgres_jdbc_url("jdbc:mysql://db/app").is_none());
}

#[test]
fn rejects_incomplete_postgres_jdbc_url() {
    assert!(parse_postgres_jdbc_url("jdbc:postgresql://").is_none());
    assert!(parse_postgres_jdbc_url("jdbc:postgresql://db").is_none());
}

#[test]
fn preserves_explicit_sslmode_from_jdbc_url() {
    assert_eq!(
        parse_sslmode("jdbc:postgresql://db:5432/app?sslmode=require&connectTimeout=5"),
        Some("require".to_string())
    );
    assert_eq!(parse_sslmode("jdbc:postgresql://db:5432/app"), None);
}

#[test]
fn parses_dbeaver_sources_from_list_and_map_shapes() {
    let content = r#"{
      "connections": [{"name":"list","host":"db1","port":"5432","database":"app","driver":"postgres"}],
      "data-sources": {"map": {"name":"map","url":"jdbc:postgresql://db2:5433/app"}}
    }"#;
    let connections = parse_data_sources(content).unwrap();
    assert_eq!(connections.len(), 2);
    assert_eq!(connections[0].driver, "postgresql");
    assert_eq!(connections[1].host, "db2");
    assert_eq!(connections[1].port, 5433);
}

#[test]
fn prefers_jdbc_url_when_dbeaver_fields_are_stale() {
    let content = r#"{
      "connections": [{
        "name":"url-configured",
        "host":"localhost",
        "port":"5432",
        "database":"postgres",
        "url":"jdbc:postgresql://localhost:15432/safeselect_demo"
      }]
    }"#;

    let connections = parse_data_sources(content).unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].host, "localhost");
    assert_eq!(connections[0].port, 15432);
    assert_eq!(connections[0].database, "safeselect_demo");
}

#[test]
fn applies_nested_configuration_fallbacks_and_skips_sources_without_hosts() {
    let content = r###"{
      "connections": [
        {"name":"nested","configuration":{"host":"db","port":"bad","database":"app","driver":"postgres","userName":"agent","handlers":{"ssh_tunnel":{"enabled":true,"properties":{"#host":"jumpbox","#port":2222,"#user":"tunnel-user","#localHost":"127.0.0.1","#localPort":15432,"#keyFile":"/tmp/id_ed25519","#authType":"KEY"}}}}},
        {"name":"missing-host","database":"ignored"}
      ],
      "data-sources": []
    }"###;
    let connections = parse_data_sources(content).unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].name, "nested");
    assert_eq!(connections[0].host, "db");
    assert_eq!(connections[0].port, 5432);
    assert_eq!(connections[0].database, "app");
    assert_eq!(connections[0].username, "agent");
    assert_eq!(connections[0].driver, "");
    assert_eq!(connections[0].ssh_host.as_deref(), Some("jumpbox"));
    assert_eq!(connections[0].ssh_port, Some(2222));
    assert_eq!(connections[0].ssh_user.as_deref(), Some("tunnel-user"));
    assert_eq!(connections[0].ssh_local_host.as_deref(), Some("127.0.0.1"));
    assert_eq!(connections[0].ssh_local_port, Some(15432));
    assert_eq!(
        connections[0].ssh_key_file.as_deref(),
        Some("/tmp/id_ed25519")
    );
    assert_eq!(connections[0].ssh_auth_type.as_deref(), Some("KEY"));
}

#[test]
fn imports_data_sources_from_a_zip_archive() {
    let path =
        std::env::temp_dir().join(format!("safeselect-dbeaver-{}.zip", uuid::Uuid::new_v4()));
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "workspace/readme.txt",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(&mut zip, b"not a data source").unwrap();
    zip.start_file(
        "workspace/data-sources.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    std::io::Write::write_all(
        &mut zip,
        br#"{"connections":[{"name":"local","host":"localhost","database":"app"}]}"#,
    )
    .unwrap();
    zip.finish().unwrap();

    let connections = import_zip(&path).unwrap();
    assert_eq!(connections[0].name, "local");
    let _ = std::fs::remove_file(path);
}

#[test]
fn legacy_direct_imports_without_sslmode_are_recognized_but_not_other_tls_modes() {
    let mut conn = sample_connection();
    conn.sslmode = Some("verify-full".into());
    let mut env: crate::config::EnvironmentConfig = toml::from_str("version=1\n[database]\nkind='jdbc'\ndriver='postgresql'\nurl='jdbc:postgresql://db.example:5432/app'\nusername='demo'\n").unwrap();
    assert!(legacy_match(&conn, &env));
    env.database.url.push_str("?sslmode=verify-full");
    assert!(legacy_match(&conn, &env));
    env.database.url = "jdbc:postgresql://db.example:5432/app?sslmode=disable".into();
    assert!(!legacy_match(&conn, &env));
    conn.ssh_host = Some("bastion.example".into());
    let ssh = crate::dbeaver_ssh_config(&conn, "project", "staging");
    env.database.url = database_url(&conn, Some(&ssh));
    env.ssh = Some(ssh);
    assert!(legacy_match(&conn, &env));
    env.database.url = env.database.url.split('?').next().unwrap().into();
    assert!(!legacy_match(&conn, &env));
}
