#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn credentialed_tunnel_failure(
    password_auth: bool,
    bastion_reachable: bool,
    rejected_password: bool,
    command: &str,
) {
    let root = std::env::temp_dir().join(format!("safeselect-hint-{}", uuid::Uuid::new_v4()));
    let environments = root.join(".safeselect/environments");
    let bin = root.join("bin");
    std::fs::create_dir_all(&environments).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(root.join(".safeselect/project.toml"), "version = 1\n").unwrap();

    // No real SSH executable, credentials, or remote infrastructure are used.
    let failure = if rejected_password {
        "printf '%s\\n' 'Permission denied: synthetic-private-user synthetic-private-destination' >&2\nexit 5"
    } else {
        "exit 1"
    };
    for name in ["ssh", "sshpass"] {
        let executable = bin.join(name);
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nif [ \"$1\" = --help ]; then exit 0; fi\nprintf attempt > \"$SAFESELECT_TEST_ATTEMPT\"\n{failure}\n"),
        )
        .unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let listener = bastion_reachable.then(|| std::net::TcpListener::bind("127.0.0.1:0").unwrap());
    let (host, port) = match &listener {
        Some(listener) => ("127.0.0.1", listener.local_addr().unwrap().port()),
        // Invalid hostname makes failure deterministic without DNS or port races.
        None => ("invalid host", 2222),
    };
    let authentication = if password_auth {
        "auth_type = 'PASSWORD'\nsecret_variable = 'SAFESELECT_TEST_SSH_PASSWORD'"
    } else {
        "identity_file = 'synthetic.key'"
    };
    std::fs::write(
        environments.join("demo.toml"),
        format!(
            "version = 1\n[database]\nkind = 'document'\nurl = 'mongodb://127.0.0.1:0/demo'\n\
             [ssh]\nenabled = true\nhost = '{host}'\nport = {port}\nusername = 'demo'\n\
             forward_host = '127.0.0.1'\nforward_port = 27017\n\
             local_host = '127.0.0.1'\nlocal_port = 0\n{authentication}\n"
        ),
    )
    .unwrap();
    let attempt = root.join("attempt");
    let start = std::time::Instant::now();
    let mut process = Command::new(env!("CARGO_BIN_EXE_safeselect"));
    process
        .args([command, "--environment", "demo", "--project"])
        .arg(&root);
    if command == "query" {
        process.args(["--sql", "SELECT 1"]);
    }
    let output = process
        .env("PATH", &bin)
        .env("SAFESELECT_CONFIG_DIR", root.join("global"))
        .env("SAFESELECT_TEST_ATTEMPT", &attempt)
        .env("SAFESELECT_TEST_SSH_PASSWORD", "synthetic-password")
        .output()
        .unwrap();
    let attempted = attempt.exists();
    std::fs::remove_dir_all(&root).unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(attempted, "The credentialed SSH attempt must run: {stderr}");
    let expected = if rejected_password {
        "SSH authentication failed"
    } else {
        "SSH tunnel establishment failed"
    };
    assert!(stderr.contains(expected), "{stderr}");
    assert!(
        stderr.contains("Database authentication was not attempted"),
        "{stderr}"
    );
    assert!(!stderr.contains("synthetic-private"), "{stderr}");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "SSH exit must skip the 20s poll"
    );
    assert_eq!(
        stderr.contains("Test TCP from the same terminal"),
        !bastion_reachable,
        "{stderr}"
    );
    if !bastion_reachable {
        assert!(stderr.contains("-- 'invalid host' 2222"), "{stderr}");
    }
    assert!(!stderr.contains("synthetic-password"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-private"));
    if command == "query" {
        assert!(output.stdout.is_empty(), "Hints must not pollute stdout");
    }
}

#[test]
fn key_authenticated_tunnel_failure_prints_connectivity_hint() {
    credentialed_tunnel_failure(false, false, false, "query");
}

#[test]
fn password_authenticated_tunnel_failure_prints_connectivity_hint() {
    credentialed_tunnel_failure(true, false, false, "query");
}

#[test]
fn reachable_bastion_with_failed_forwarding_does_not_print_connectivity_hint() {
    credentialed_tunnel_failure(false, true, false, "query");
}

#[test]
fn rejected_ssh_password_is_classified_and_redacted_without_waiting_for_deadline() {
    for command in ["query", "check", "doctor"] {
        credentialed_tunnel_failure(true, true, true, command);
    }
}
