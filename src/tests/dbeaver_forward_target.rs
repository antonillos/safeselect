use super::*;

fn connection() -> dbeaver::DBeaverConnection {
    dbeaver::DBeaverConnection {
        name: "staging".into(),
        host: "db.example".into(),
        port: 5432,
        database: "app".into(),
        driver: "postgresql".into(),
        username: "demo".into(),
        password: None,
        sslmode: None,
        ssh_host: Some("bastion.example".into()),
        ssh_port: Some(22),
        ssh_user: Some("demo".into()),
        ssh_local_host: None,
        ssh_local_port: None,
        ssh_key_file: None,
        ssh_auth_type: Some("KEY".into()),
    }
}

#[test]
fn forward_target_uses_source_defaults_and_trims_user_input() {
    let conn = connection();
    let mut calls = 0;
    let result = prompt_dbeaver_forward_target_with(&conn, |prompt, default| {
        calls += 1;
        if calls == 1 {
            assert!(prompt.contains("host"));
            assert_eq!(default, "db.example");
            Ok(" target.example ".into())
        } else {
            assert!(prompt.contains("port"));
            assert_eq!(default, "5432");
            Ok(" 6432 ".into())
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(result, ("target.example".into(), 6432));
}

#[test]
fn forward_target_retains_default_port_for_invalid_input() {
    for input in ["", "bad", "65536"] {
        let result = prompt_dbeaver_forward_target_with(&connection(), |prompt, default| {
            Ok(if prompt.contains("port") {
                input.into()
            } else {
                default.into()
            })
        })
        .unwrap();
        assert_eq!(result, ("db.example".into(), 5432));
    }
}

#[test]
fn forward_target_cancellation_propagates_and_stops_reading() {
    for cancel_at in [1, 2] {
        let mut calls = 0;
        let result = prompt_dbeaver_forward_target_with(&connection(), |_, default| {
            calls += 1;
            if calls == cancel_at {
                Err(SafeselectError::Other("Import cancelled".into()))
            } else {
                Ok(default.into())
            }
        });
        assert_eq!(result.unwrap_err().to_string(), "Import cancelled");
        assert_eq!(calls, cancel_at);
    }
}

#[test]
fn local_tunnel_placeholder_does_not_suggest_a_remote_host() {
    let mut conn = connection();
    conn.ssh_local_host = Some("127.0.0.1".into());
    conn.ssh_local_port = Some(conn.port);
    let result =
        prompt_dbeaver_forward_target_with(&conn, |_, default| Ok(default.into())).unwrap();
    assert_eq!(result, (String::new(), 5432));
}
