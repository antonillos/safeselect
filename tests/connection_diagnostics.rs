#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn shared_bastion_checks_distinct_databases_and_reports_endpoint_reuse() {
    check_existing_routes(false);
}

#[test]
fn doctor_reports_selected_duplicate_without_exposing_credential_references() {
    check_existing_routes(true);
}

fn check_existing_routes(duplicate: bool) {
    let root =
        std::env::temp_dir().join(format!("safeselect-diagnostics-{}", uuid::Uuid::new_v4()));
    let environments = root.join(".safeselect/environments");
    let drivers = root.join("global/drivers");
    let java_bin = root.join("java/bin");
    for directory in [&environments, &drivers, &java_bin] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let mut project = "version = 1\n[ssh_bastions.shared]\nhost = 'synthetic-private-bastion'\nusername = 'synthetic-private-ssh-user'\nsecret_account = 'synthetic-private-account-a'\nauth_type = 'PASSWORD'\n".to_string();
    if duplicate {
        project.push_str("[ssh_bastions.other]\nhost = 'synthetic-private-bastion'\nport = 22\nusername = 'synthetic-private-ssh-user'\nsecret_account = 'synthetic-private-account-b'\nauth_type = 'PASSWORD'\n");
    }
    std::fs::write(root.join(".safeselect/project.toml"), &project).unwrap();
    let driver = drivers.join("postgresql.jar");
    std::fs::write(&driver, "synthetic-driver").unwrap();
    let checksum = hex::encode(Sha256::digest(b"synthetic-driver"));
    std::fs::write(drivers.join("postgresql.toml"), format!(
        "version = 1\nvendor = 'postgresql'\npath = '{}'\nclass = 'org.postgresql.Driver'\nsha256 = '{checksum}'\n", driver.display()
    )).unwrap();

    // Exercise the CLI and IPC contract without a database or stored secrets.
    let java = java_bin.join("java");
    std::fs::write(&java, r#"#!/bin/sh
if [ "$1" = '-version' ]; then
    printf '%s\n' 'openjdk version "17.0.1"' >&2
    exit 0
fi
read password
printf '%s\n' 'ready'
read ping
printf '%s\n' '{"id":0,"ok":"pong"}'
read ping
printf '%s\n' '{"id":1,"ok":"pong"}'
read query
printf '%s\n' '{"id":2,"ok":{"columns":["connection_test"],"rows":[[1]],"row_count":1,"byte_count":1}}'
read shutdown
printf '%s\n' '{"id":3,"ok":"bye"}'
"#).unwrap();
    std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut peers = Vec::new();
    let mut original_environments = Vec::new();
    for (name, variable) in [
        ("alpha", "SAFESELECT_TEST_DB_A"),
        ("beta", "SAFESELECT_TEST_DB_B"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let config = format!(
            "version = 1\n[database]\ndriver = 'postgresql'\nurl = 'jdbc:postgresql://{address}/{name}'\nusername = 'synthetic-private-{name}-user'\n[database.secret]\nsource = 'env'\nvariable = '{variable}'\n[ssh]\nenabled = true\nbastion = 'shared'\nlocal_port = {}\nforward_host = 'synthetic-private-{name}-db'\nforward_port = 5432\n", address.port()
        );
        let file = environments.join(format!("{name}.toml"));
        std::fs::write(&file, &config).unwrap();
        original_environments.push((file, config));
        peers.push(std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 8];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(request, [0, 0, 0, 8, 4, 210, 22, 47]);
            stream.write_all(b"N").unwrap();
        }));
    }

    let output = Command::new(env!("CARGO_BIN_EXE_safeselect"))
        .args([if duplicate { "doctor" } else { "check" }, "--project"])
        .arg(&root)
        .env("JAVA_HOME", root.join("java"))
        .env("SAFESELECT_CONFIG_DIR", root.join("global"))
        .env("SAFESELECT_TEST_DB_A", "synthetic-private-password-a")
        .env("SAFESELECT_TEST_DB_B", "synthetic-private-password-b")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert_eq!(
        text.matches("SAFESELECT_SSH_ENDPOINT_REUSED").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("◇ [SAFESELECT_SSH_ENDPOINT_REUSED]").count(),
        2,
        "{text}"
    );
    assert!(!text.contains("⚠ [SAFESELECT_SSH_ENDPOINT_REUSED]"));
    assert_eq!(
        text.matches("SAFESELECT_BACKEND_VERIFICATION_OK").count(),
        2,
        "{text}"
    );
    assert!(text.contains("does not verify the configured remote destination"));
    assert_eq!(
        text.contains("SAFESELECT_SSH_BASTION_CREDENTIALS_DIFFER"),
        duplicate
    );
    if duplicate {
        assert!(text.contains("Selected SSH bastion \"shared\""));
        assert!(text.contains("\"other\""));
    }
    assert!(!text.contains("synthetic-private"), "{text}");
    assert!(!text.contains("SAFESELECT_SSH_TUNNEL_ATTEMPT"), "{text}");
    for (file, content) in original_environments {
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
    }
    assert_eq!(
        std::fs::read_to_string(root.join(".safeselect/project.toml")).unwrap(),
        project
    );
    for peer in peers {
        peer.join().unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}
