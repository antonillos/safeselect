use super::*;
use import_wizard::{Answer, Wizard};
use std::io::Write;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("import-navigation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn compass(&self) -> PathBuf {
        let path = self.0.join("compass.json");
        std::fs::write(&path, r#"{"connections":[{"name":"first","connectionString":"mongodb://reader@one.example/app"},{"name":"second","connectionString":"mongodb://reader@two.example/app"}]}"#).unwrap();
        path
    }
    fn dbeaver(&self) -> PathBuf {
        let path = self.0.join("dbeaver.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        archive
            .start_file(
                "workspace/data-sources.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(br#"{"connections":[{"name":"first","driver":"postgres","username":"reader","configuration":{"url":"jdbc:postgresql://one.example:5432/app"}},{"name":"second","driver":"postgres","username":"reader","configuration":{"url":"jdbc:postgresql://two.example:5432/app"}}]}"#).unwrap();
        archive.finish().unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn text(value: &str) -> Result<Answer> {
    Ok(Answer::Text(value.into()))
}
fn select(indices: &[usize]) -> Result<Answer> {
    Ok(Answer::Selection(indices.into()))
}
fn yes() -> Result<Answer> {
    Ok(Answer::Confirm(true))
}
fn back() -> Result<Answer> {
    Err(SafeselectError::ImportBack)
}

#[test]
fn compass_and_dbeaver_navigate_across_connections_and_cancel_without_writes() {
    for compass in [false, true] {
        let fixture = Fixture::new();
        let path = if compass {
            fixture.compass()
        } else {
            fixture.dbeaver()
        };
        let mut ui = Wizard::scripted(vec![
            ("multi:", select(&[0, 1])),
            ("Environment name:", text("first-edited")),
            ("How do you want", select(&[2])),
            ("Password environment variable:", text("FIRST_DB_PASSWORD")),
            // Esc on the second connection goes back to the first connection's final question.
            ("Environment name:", back()),
            ("Password environment variable:", back()),
            ("How do you want", back()),
            ("Environment name:", back()),
            ("multi:", back()),
        ]);
        let result = ui
            .run(|ui| {
                if compass {
                    prepare_compass(Some(path.clone()), false, &fixture.0, ui)
                } else {
                    prepare_dbeaver(path.to_str().unwrap(), false, &fixture.0, ui)
                }
            })
            .unwrap();
        assert!(result.is_none());
        ui.assert_finished();
        assert!(!fixture.0.join(".safeselect").exists());
    }
}

#[test]
fn all_importers_can_return_from_final_confirmation_then_save_a_revised_batch() {
    for provider in ["compose", "compass", "dbeaver"] {
        let fixture = Fixture::new();
        let path = if provider == "dbeaver" {
            fixture.dbeaver()
        } else {
            fixture.compass()
        };
        let connections = vec![(
            "demo".into(),
            compose::ComposeConnection {
                name: "first".into(),
                env_name: "first".into(),
                service: "db".into(),
                host: "localhost".into(),
                port: 5432,
                database: "app".into(),
                username: "reader".into(),
                password_literal: None,
                password_var: None,
                compose_path: "compose.yaml".into(),
            },
        )];
        let mut ui = Wizard::scripted(vec![
            ("multi:", select(&[0])),
            ("Environment name", text("revised")),
            ("How do you want", select(&[2])),
            ("Password environment variable:", text("BEFORE_DB_PASSWORD")),
            ("Save this import batch?", back()),
            ("Password environment variable:", text("AFTER_DB_PASSWORD")),
            ("Save this import batch?", yes()),
        ]);
        let batch = ui
            .run(|ui| match provider {
                "compose" => prepare_compose(&fixture.0, &connections, ui),
                "compass" => prepare_compass(Some(path.clone()), false, &fixture.0, ui),
                _ => prepare_dbeaver(path.to_str().unwrap(), false, &fixture.0, ui),
            })
            .unwrap()
            .unwrap()
            .unwrap();
        ui.assert_finished();
        assert!(!fixture.0.join(".safeselect").exists());
        assert_eq!(batch.names(), ["revised"]);
        batch.commit().unwrap();
        let environment =
            std::fs::read_to_string(fixture.0.join(".safeselect/environments/revised.toml"))
                .unwrap();
        assert!(environment.contains("AFTER_DB_PASSWORD"));
        assert!(!environment.contains("BEFORE_DB_PASSWORD"));
    }
}

#[test]
fn cancelled_reset_leaves_existing_configuration_untouched() {
    let fixture = Fixture::new();
    let path = fixture.compass();
    let dir = fixture.0.join(".safeselect");
    std::fs::create_dir_all(dir.join("environments")).unwrap();
    let project = "version = 1\ngenerated_by = '0.0.1'\n";
    std::fs::write(dir.join("project.toml"), project).unwrap();
    std::fs::write(dir.join("environments/keep.toml"), "sentinel").unwrap();
    let mut ui = Wizard::scripted(vec![
        ("Reset environments", yes()),
        ("multi:", back()),
        ("Reset environments", back()),
    ]);
    assert!(ui
        .run(|ui| prepare_compass(Some(path.clone()), false, &fixture.0, ui))
        .unwrap()
        .is_none());
    ui.assert_finished();
    assert_eq!(
        std::fs::read_to_string(dir.join("project.toml")).unwrap(),
        project
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("environments/keep.toml")).unwrap(),
        "sentinel"
    );
}

#[test]
fn switching_credential_branch_discards_pending_password_writes() {
    let mut ui = Wizard::scripted(vec![
        ("How do you want", select(&[0])), // Imported password.
        (
            "Where do you want",
            select(&[if cfg!(target_os = "macos") { 1 } else { 0 }]),
        ),
        ("Use the password only", yes()),
        (
            "Password environment variable:",
            text("SAFESELECT_NAVIGATION_TEST_SECRET"),
        ),
        ("done", back()),
        ("Password environment variable:", back()),
        ("Use the password only", back()),
        ("Where do you want", back()),
        ("How do you want", select(&[3])), // Configure later.
        (
            "Password environment variable:",
            text("CONFIGURE_LATER_PASSWORD"),
        ),
        ("done", yes()),
    ]);
    let (secret, pending) = ui
        .run(|ui| {
            let mut pending = import_batch::PendingSecrets::default();
            let secret = compass_import::CredentialPrompt {
                project: "demo",
                environment: "dev",
                ssh: false,
                imported: Some("synthetic-hidden-password"),
                existing: None,
            }
            .run_draft(ui, &mut pending)?;
            ui.confirm("done", false)?;
            Ok((secret, pending))
        })
        .unwrap()
        .unwrap();
    ui.assert_finished();
    assert!(pending.keychain.is_empty());
    assert!(pending.session.is_empty());
    assert_eq!(
        secret.unwrap().variable.as_deref(),
        Some("CONFIGURE_LATER_PASSWORD")
    );
    assert!(std::env::var_os("SAFESELECT_NAVIGATION_TEST_SECRET").is_none());
}

#[test]
fn ssh_authentication_can_be_revised_without_retaining_old_secret_sources() {
    for compass in [false, true] {
        let fixture = Fixture::new();
        let mut dbeaver = dbeaver::import_zip(&fixture.dbeaver()).unwrap().remove(0);
        dbeaver.ssh_host = Some("bastion.example".into());
        dbeaver.ssh_user = Some("reader".into());
        let mut mongo = compass::import_path(&fixture.compass()).unwrap().remove(0);
        mongo.ssh_host = Some("bastion.example".into());
        mongo.ssh_user = Some("reader".into());
        let mut ui = Wizard::scripted(vec![
            ("Configure SSH tunnel now?", yes()),
            ("SSH bastion host:", text("bastion.example")),
            ("SSH port:", text("22")),
            ("SSH user:", text("reader")),
            ("Database target host", text("db.example")),
            ("Database target port", text("5432")),
            ("Authentication method:", select(&[1])),
            ("How do you want", select(&[2])),
            ("Password environment variable:", text("OLD_SSH_PASSWORD")),
            ("done", back()),
            ("Password environment variable:", back()),
            ("How do you want", back()),
            ("Authentication method:", select(&[0])),
            ("SSH key file path:", text("/synthetic/id_ed25519")),
            ("done", yes()),
        ]);
        let (ssh, pending) = ui
            .run(|ui| {
                let mut pending = import_batch::PendingSecrets::default();
                let ssh = if compass {
                    prompt_compass_ssh_config(
                        &mongo,
                        ImportTarget {
                            project: "demo",
                            environment: "dev",
                        },
                        &fixture.0,
                        &[],
                        None,
                        ui,
                        &mut pending,
                    )?
                } else {
                    prompt_ssh_config(
                        &dbeaver,
                        ImportTarget {
                            project: "demo",
                            environment: "dev",
                        },
                        &fixture.0,
                        &[],
                        None,
                        ui,
                        &mut pending,
                    )?
                };
                ui.confirm("done", false)?;
                Ok((ssh, pending))
            })
            .unwrap()
            .unwrap();
        ui.assert_finished();
        assert_eq!(ssh.identity_file.as_deref(), Some("/synthetic/id_ed25519"));
        assert_eq!(ssh.auth_type.as_deref(), Some("KEY"));
        assert!(ssh.secret_account.is_none());
        assert!(ssh.secret_variable.is_none());
        assert!(pending.keychain.is_empty() && pending.session.is_empty());
    }
}

#[test]
fn ctrl_c_and_declining_final_save_leave_no_configuration_in_all_importers() {
    for provider in ["compose", "compass", "dbeaver"] {
        for interrupt in [true, false] {
            let fixture = Fixture::new();
            let path = if provider == "dbeaver" {
                fixture.dbeaver()
            } else {
                fixture.compass()
            };
            let connections = vec![(
                "demo".into(),
                compose::ComposeConnection {
                    name: "first".into(),
                    env_name: "first".into(),
                    service: "db".into(),
                    host: "localhost".into(),
                    port: 5432,
                    database: "app".into(),
                    username: "reader".into(),
                    password_literal: None,
                    password_var: None,
                    compose_path: "compose.yaml".into(),
                },
            )];
            let mut steps = vec![("multi:", select(&[0])), ("Environment name", text("dev"))];
            if interrupt {
                steps.push(("How do you want", Err(SafeselectError::ImportCancelled)));
            } else {
                steps.extend([
                    ("How do you want", select(&[2])),
                    ("Password environment variable:", text("LATER_PASSWORD")),
                    ("Save this import batch?", Ok(Answer::Confirm(false))),
                ]);
            }
            let mut ui = Wizard::scripted(steps);
            assert!(ui
                .run(|ui| match provider {
                    "compose" => prepare_compose(&fixture.0, &connections, ui),
                    "compass" => prepare_compass(Some(path.clone()), false, &fixture.0, ui),
                    _ => prepare_dbeaver(path.to_str().unwrap(), false, &fixture.0, ui),
                })
                .unwrap()
                .is_none());
            ui.assert_finished();
            assert!(!fixture.0.join(".safeselect").exists());
        }
    }
}

#[test]
fn staged_environments_reserve_names_and_distinct_ssh_ports_before_any_write() {
    let fixture = Fixture::new();
    let path = fixture.0.join("connections.json");
    std::fs::write(&path, r#"{"connections":[{"name":"dev","connectionOptions":{"connectionString":"mongodb://one.example/app","sshTunnel":{"host":"bastion.example","username":"reader","authenticationMethod":"password"}}},{"name":"dev","connectionOptions":{"connectionString":"mongodb://two.example/app","sshTunnel":{"host":"bastion.example","username":"reader","authenticationMethod":"password"}}}]}"#).unwrap();
    let batch = prepare_compass(Some(path), true, &fixture.0, &mut Wizard::default())
        .unwrap()
        .unwrap();
    assert_eq!(batch.names(), ["dev", "dev-2"]);
    let ports: std::collections::HashSet<_> = batch
        .environments
        .iter()
        .map(|(_, env)| ssh_local_port_from_config(env).unwrap())
        .collect();
    assert_eq!(ports.len(), 2);
    assert!(!fixture.0.join(".safeselect").exists());
}

#[test]
fn repeated_dbeaver_identity_is_skipped_in_unattended_batch() {
    let fixture = Fixture::new();
    let path = fixture.0.join("duplicate.zip");
    let connection = r#"{"name":"dev","driver":"postgres","username":"reader","configuration":{"url":"jdbc:postgresql://db.example:5432/app"}}"#;
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    archive
        .start_file(
            "workspace/data-sources.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    archive
        .write_all(format!("{{\"connections\":[{connection},{connection}]}}").as_bytes())
        .unwrap();
    archive.finish().unwrap();
    let batch = prepare_dbeaver(
        path.to_str().unwrap(),
        true,
        &fixture.0,
        &mut Wizard::default(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(batch.names(), ["dev"]);
}

#[test]
fn compose_commands_handle_missing_and_empty_sources_without_writes() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("missing");
    assert!(cmd_import_compose(Some(missing.clone()), false).is_err());
    assert!(cmd_import_compose_unattended(Some(missing)).is_err());
    cmd_import_compose(Some(fixture.0.clone()), false).unwrap();
    cmd_import_compose(Some(fixture.0.clone()), true).unwrap();
    assert!(!fixture.0.join(".safeselect").exists());
}

#[test]
fn optional_import_checks_require_confirmation_and_a_successful_jdbc_driver_setup() {
    use std::cell::Cell;
    for jdbc in [false, true] {
        for confirmed in [false, true] {
            let driver_calls = Cell::new(0);
            let verify_calls = Cell::new(0);
            check_import_if_requested_with(
                jdbc,
                || Ok(confirmed),
                || {
                    driver_calls.set(driver_calls.get() + 1);
                    Ok(())
                },
                || {
                    verify_calls.set(verify_calls.get() + 1);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(driver_calls.get(), usize::from(jdbc && confirmed));
            assert_eq!(verify_calls.get(), usize::from(confirmed));
        }
    }
    fn failure<T>() -> Result<T> {
        Err(SafeselectError::Other("synthetic failure".into()))
    }
    assert!(check_import_if_requested_with(
        true,
        failure,
        || panic!("unexpected setup"),
        || panic!("unexpected verification")
    )
    .is_err());
    assert!(check_import_if_requested_with(
        true,
        || Ok(true),
        failure,
        || panic!("driver setup failed")
    )
    .is_err());
    assert!(check_import_if_requested_with(
        false,
        || Ok(true),
        || panic!("MongoDB needs no JDBC driver"),
        failure
    )
    .is_err());
}
