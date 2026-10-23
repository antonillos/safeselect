use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    base: PathBuf,
    project: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("safeselect-registration-{}", uuid::Uuid::new_v4()));
        let project = base.join("application");
        let home = base.join("home");
        fs::create_dir_all(project.join(".safeselect")).unwrap();
        fs::create_dir_all(home.join(".config/opencode")).unwrap();
        fs::write(home.join(".config/opencode/opencode.jsonc"), "{\"permissions\": [{\"action\":\"skill\",\"resource\":\"*\",\"effect\":\"ask\"}], \"mcp\": {}}").unwrap();
        Self {
            base,
            project,
            home,
        }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_safeselect"))
            .args(args)
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("CODEX_HOME", self.home.join("custom-codex"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("SAFESELECT_CONFIG_DIR", self.home.join("safeselect"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
    fn success(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn root(&self, local: bool) -> PathBuf {
        if local {
            self.project.join(".agents/skills")
        } else {
            self.home.join(".agents/skills")
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn assert_bundle(root: &Path) {
    for name in [
        "safeselect",
        "safeselect-setup",
        "safeselect-sql",
        "safeselect-mongodb",
    ] {
        assert!(root.join(name).join("SKILL.md").is_file());
        assert!(root.join(name).join("references/workflows.md").is_file());
    }
}

#[test]
fn both_clients_and_scopes_register_update_and_keep_shared_skills() {
    for local in [false, true] {
        let f = Fixture::new();
        for client in ["codex", "opencode"] {
            let mut args = vec![
                "agent",
                "install",
                client,
                "--environment",
                "dev",
                "--name",
                "safe",
            ];
            if local {
                args.push("--local");
            }
            f.success(&args);
            let root = f.root(local);
            assert_bundle(&root);
            let manifest = fs::read(root.join(".safeselect-skills.json")).unwrap();
            f.success(&args);
            assert_eq!(
                fs::read(root.join(".safeselect-skills.json")).unwrap(),
                manifest
            );
            // Repair MCP-only legacy registration even if MCP values already match.
            fs::remove_dir_all(&root).unwrap();
            f.success(&args);
            assert_bundle(&root);
            args[6] = "second";
            f.success(&args);
            let mut upgrade = vec![
                "agent",
                "upgrade",
                client,
                "--name",
                "second",
                "--environment",
                "dev",
            ];
            if local {
                upgrade.push("--local");
            }
            let mut old: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join(".safeselect-skills.json")).unwrap())
                    .unwrap();
            old["version"] = "0.0.1".into();
            fs::write(
                root.join(".safeselect-skills.json"),
                serde_json::to_vec(&old).unwrap(),
            )
            .unwrap();
            f.success(&upgrade);
            let status = f.success(&["agent", "status"]);
            assert!(status.contains("skills: current"));
            assert!(status.contains("client loading unverified"));
            assert!(f
                .success(&["agent", "uninstall", client, "--name", "safe"])
                .contains("skills are retained"));
            assert_bundle(&root);
        }
        let config = fs::read_to_string(f.home.join(".config/opencode/opencode.jsonc")).unwrap();
        let config: serde_json::Value = serde_json::from_str(&config).unwrap();
        assert_eq!(config["permissions"][0]["effect"], "ask");
    }
}

#[test]
fn inferred_local_upgrades_preserve_global_skills() {
    for client in ["codex", "opencode"] {
        for named in [false, true] {
            for global_conflict in [false, true] {
                let f = Fixture::new();
                f.success(&[
                    "agent",
                    "install",
                    client,
                    "--environment",
                    "dev",
                    "--local",
                ]);
                let manifest = f.root(true).join(".safeselect-skills.json");
                let mut old: serde_json::Value =
                    serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
                old["version"] = "0.0.1".into();
                fs::write(&manifest, serde_json::to_vec(&old).unwrap()).unwrap();
                let global_skill = f.root(false).join("safeselect/SKILL.md");
                if global_conflict {
                    fs::create_dir_all(global_skill.parent().unwrap()).unwrap();
                    fs::write(&global_skill, "unrelated global skill").unwrap();
                }
                let mut args = vec!["agent", "upgrade", client];
                if named {
                    args.extend(["--name", "safeselect-application-dev"]);
                }
                f.success(&args);
                let current: serde_json::Value =
                    serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
                assert_eq!(current["version"], env!("CARGO_PKG_VERSION"));
                assert_bundle(&f.root(true));
                if global_conflict {
                    assert_eq!(
                        fs::read_to_string(global_skill).unwrap(),
                        "unrelated global skill"
                    );
                    assert!(!f.root(false).join(".safeselect-skills.json").exists());
                } else {
                    assert!(!f.root(false).exists());
                }
            }
        }
    }
}

#[test]
fn global_upgrades_with_other_local_entries_keep_global_skill_scope() {
    for client in ["codex", "opencode"] {
        for named in [false, true] {
            let f = Fixture::new();
            f.success(&[
                "agent",
                "install",
                client,
                "--environment",
                "pre",
                "--local",
            ]);
            f.success(&["agent", "install", client, "--environment", "dev"]);
            let local_manifest = f.root(true).join(".safeselect-skills.json");
            let local_before = fs::read(&local_manifest).unwrap();
            let global_manifest = f.root(false).join(".safeselect-skills.json");
            let mut old: serde_json::Value =
                serde_json::from_slice(&fs::read(&global_manifest).unwrap()).unwrap();
            old["version"] = "0.0.1".into();
            fs::write(&global_manifest, serde_json::to_vec(&old).unwrap()).unwrap();
            let mut args = vec!["agent", "upgrade", client, "--environment", "dev"];
            if named {
                args.extend(["--name", "safeselect-application-dev"]);
            }
            f.success(&args);
            let current: serde_json::Value =
                serde_json::from_slice(&fs::read(global_manifest).unwrap()).unwrap();
            assert_eq!(current["version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(fs::read(local_manifest).unwrap(), local_before);
        }
    }
}

#[test]
fn conflict_and_invalid_config_never_change_integration_or_skills() {
    let f = Fixture::new();
    let config = f.home.join("custom-codex/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "invalid = [").unwrap();
    assert!(!f
        .run(&["agent", "install", "codex", "--environment", "dev"])
        .status
        .success());
    assert!(!f.root(false).join("safeselect").exists());
    assert_eq!(fs::read_to_string(&config).unwrap(), "invalid = [");
    fs::write(&config, "# preserve comment\nmodel = \"example-model\"\n").unwrap();
    f.success(&["agent", "install", "codex", "--environment", "dev"]);
    let before = fs::read(&config).unwrap();
    let skill = f.root(false).join("safeselect/SKILL.md");
    fs::write(&skill, "user customized skill").unwrap();
    assert!(!f
        .run(&["agent", "install", "codex", "--environment", "other"])
        .status
        .success());
    assert_eq!(fs::read(&config).unwrap(), before);
    assert_eq!(fs::read_to_string(&skill).unwrap(), "user customized skill");
    assert!(f.success(&["agent", "status"]).contains("skills: conflict"));
}

#[test]
fn status_is_read_only_and_other_clients_do_not_install_skills() {
    let f = Fixture::new();
    assert!(f.success(&["agent", "status"]).contains("skills: absent"));
    assert!(!f.root(false).exists());
    assert!(!f.root(true).exists());
    f.success(&[
        "agent",
        "install",
        "cursor",
        "--environment",
        "dev",
        "--local",
    ]);
    assert!(!f.root(true).exists());
}

#[cfg(unix)]
#[test]
fn config_modes_comments_and_existing_backup_survive_registration() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let config = f.home.join("custom-codex/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "# user comment\nmodel = \"example-model\"\n").unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
    let backup = config.with_extension("safeselect.bak");
    fs::write(&backup, "existing backup").unwrap();
    f.success(&[
        "agent",
        "install",
        "codex",
        "--environment",
        "dev",
        "--name",
        "safe",
    ]);
    f.success(&[
        "agent",
        "upgrade",
        "codex",
        "--environment",
        "dev",
        "--name",
        "safe",
    ]);
    let text = fs::read_to_string(&config).unwrap();
    assert!(text.contains("# user comment"));
    assert!(text.contains("model = \"example-model\""));
    assert_eq!(
        fs::metadata(config).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read_to_string(backup).unwrap(), "existing backup");
}
