use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("safeselect-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("client")).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join(".agents/skills")
    }
    fn config(&self) -> PathBuf {
        self.0.join("client/config.toml")
    }
    fn install(&self) {
        register(&self.root(), &self.config(), || Ok(())).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn embedded_bundle_matches_sources_and_portable_metadata() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let mut actual = BTreeMap::new();
    let dirs = FILES
        .iter()
        .flat_map(|(name, _)| {
            Path::new(name)
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().to_string())
        })
        .collect();
    for name in NAMES {
        inventory(&source, &source.join(name), &mut actual, &dirs).unwrap();
    }
    assert_eq!(actual, desired().files);
    for name in NAMES {
        let content = fs::read_to_string(source.join(name).join("SKILL.md")).unwrap();
        let parts: Vec<_> = content.splitn(3, "---").collect();
        assert_eq!(parts.len(), 3);
        let meta: serde_json::Value = serde_saphyr::from_str(parts[1]).unwrap();
        assert_eq!(meta["name"], name);
        let description = meta["description"].as_str().unwrap();
        assert!(!description.is_empty() && description.len() <= 1024);
        assert!(parts[2].contains("references/workflows.md"));
        assert!(!content.contains("../README"));
    }
}

#[test]
fn installs_idempotently_and_preserves_unrelated_skills() {
    let f = Fixture::new();
    assert_eq!(inspect(&f.root()).unwrap(), "absent");
    f.install();
    let manifest = fs::read(f.root().join(MANIFEST)).unwrap();
    fs::create_dir(f.root().join("foreign")).unwrap();
    fs::write(f.root().join("foreign/SKILL.md"), "user skill").unwrap();
    f.install();
    assert_eq!(inspect(&f.root()).unwrap(), "current");
    assert_eq!(fs::read(f.root().join(MANIFEST)).unwrap(), manifest);
    assert_eq!(
        fs::read_to_string(f.root().join("foreign/SKILL.md")).unwrap(),
        "user skill"
    );
}

#[test]
fn updates_only_unchanged_owned_resources() {
    let f = Fixture::new();
    f.install();
    let path = f.root().join("safeselect/SKILL.md");
    fs::write(&path, "old managed content").unwrap();
    let mut old = desired();
    old.version = "0.0.1".into();
    old.files
        .insert("safeselect/SKILL.md".into(), hash(b"old managed content"));
    fs::write(f.root().join(MANIFEST), serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(inspect(&f.root()).unwrap(), "outdated");
    f.install();
    assert_eq!(inspect(&f.root()).unwrap(), "current");
    assert!(fs::read_to_string(path).unwrap().starts_with("---"));
}

#[test]
fn conflicts_block_config_operation() {
    for kind in [
        "foreign",
        "edited",
        "missing",
        "extra",
        "directory",
        "manifest",
    ] {
        let f = Fixture::new();
        f.install();
        let file = f.root().join("safeselect/SKILL.md");
        match kind {
            "foreign" => fs::remove_file(f.root().join(MANIFEST)).unwrap(),
            "edited" => fs::write(&file, "custom instructions").unwrap(),
            "missing" => fs::remove_file(&file).unwrap(),
            "extra" => fs::write(f.root().join("safeselect/extra.md"), "extra").unwrap(),
            "directory" => fs::create_dir(f.root().join("safeselect/extra")).unwrap(),
            _ => fs::write(f.root().join(MANIFEST), "{}").unwrap(),
        }
        let before = fs::read(&file).ok();
        let mut called = false;
        assert!(register(&f.root(), &f.config(), || {
            called = true;
            Ok(())
        })
        .is_err());
        assert!(!called, "{kind}");
        assert_eq!(fs::read(&file).ok(), before);
        assert!(status(&f.root(), "project").contains("conflict"));
    }
}

#[test]
fn rolls_back_config_backup_and_bundle_on_operation_failure() {
    for installed in [false, true] {
        let f = Fixture::new();
        if installed {
            f.install();
        }
        let old = fs::read(f.root().join(MANIFEST)).ok();
        // Force a replacement of an already-owned bundle as well as a fresh install.
        if installed {
            let mut manifest = desired();
            manifest.version = "0.0.1".into();
            fs::write(
                f.root().join(MANIFEST),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        }
        let before = fs::read(f.root().join(MANIFEST)).ok();
        fs::write(f.config(), "original config").unwrap();
        let backup = f.config().with_extension("safeselect.bak");
        fs::write(&backup, "original backup").unwrap();
        assert!(register(&f.root(), &f.config(), || {
            fs::write(f.config(), "changed config")?;
            fs::write(&backup, "changed backup")?;
            Err(failure(&f.config(), "injected operation failure"))
        })
        .is_err());
        assert_eq!(fs::read_to_string(f.config()).unwrap(), "original config");
        assert_eq!(fs::read_to_string(backup).unwrap(), "original backup");
        assert_eq!(fs::read(f.root().join(MANIFEST)).ok(), before);
        if old.is_none() {
            assert!(!f.root().join("safeselect").exists());
        }
    }
}

#[test]
fn current_bundle_still_rolls_back_failed_config_changes() {
    let f = Fixture::new();
    f.install();
    assert!(register(&f.root(), &f.config(), || {
        fs::write(f.config(), "new config")?;
        Err(failure(&f.config(), "failure"))
    })
    .is_err());
    assert!(!f.config().exists());
    assert_eq!(inspect(&f.root()).unwrap(), "current");
}

#[test]
fn lock_serializes_clients_and_is_released() {
    let f = Fixture::new();
    let guard = lock(&f.root()).unwrap();
    assert!(register(&f.root(), &f.config(), || panic!("must not run")).is_err());
    drop(guard);
    f.install();
    assert!(!f.root().join(LOCK).exists());
}

#[test]
fn rejects_manifest_path_traversal() {
    let f = Fixture::new();
    f.install();
    let mut manifest = desired();
    manifest
        .files
        .insert("safeselect/../../outside".into(), hash(b"anything"));
    fs::write(
        f.root().join(MANIFEST),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(inspect(&f.root()).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_and_unsafe_permissions() {
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    for target in [
        ".agents",
        ".agents/skills",
        ".agents/skills/safeselect",
        ".agents/skills/safeselect/SKILL.md",
        ".agents/skills/.safeselect-skills.json",
        "client",
        "client/config.toml",
    ] {
        for link in [false, true] {
            let f = Fixture::new();
            f.install();
            fs::write(f.config(), "config").unwrap();
            let path = f.0.join(target);
            if link {
                let destination = f.0.join("saved");
                fs::rename(&path, &destination).unwrap();
                symlink(&destination, &path).unwrap();
            } else {
                let mode = if path.is_dir() { 0o777 } else { 0o666 };
                fs::set_permissions(&path, Permissions::from_mode(mode)).unwrap();
            }
            assert!(
                register(&f.root(), &f.config(), || panic!("must not run")).is_err(),
                "{target}, symlink={link}"
            );
        }
    }
}

#[test]
fn partial_replacement_failure_restores_each_moved_resource() {
    let f = Fixture::new();
    f.install();
    let before = fs::read(f.root().join(MANIFEST)).unwrap();
    let staged = stage(&f.root()).unwrap();
    fs::remove_dir_all(staged.0.join("new/safeselect-sql")).unwrap();
    let mut changed = Vec::new();
    assert!(replace(&f.root(), &staged, &mut changed).is_err());
    assert!(changed.len() >= 3);
    rollback(&f.root(), &staged, &changed).unwrap();
    assert_eq!(fs::read(f.root().join(MANIFEST)).unwrap(), before);
    assert_eq!(inspect(&f.root()).unwrap(), "current");
}

#[test]
fn preflight_failure_leaves_resources_and_config_untouched() {
    let f = Fixture::new();
    fs::write(f.config(), "original").unwrap();
    assert!(register_prepared(
        &f.root(),
        &f.config(),
        || -> Result<()> { Err(failure(&f.config(), "invalid client configuration")) },
        |()| panic!("write must not run")
    )
    .is_err());
    assert_eq!(fs::read_to_string(f.config()).unwrap(), "original");
    assert_eq!(inspect(&f.root()).unwrap(), "absent");
    assert!(!f.root().join(LOCK).exists());
}
