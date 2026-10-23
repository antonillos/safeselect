use crate::error::{Result, SafeselectError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions, Permissions};
use std::path::{Component, Path, PathBuf};

const NAMES: [&str; 4] = [
    "safeselect",
    "safeselect-setup",
    "safeselect-sql",
    "safeselect-mongodb",
];
const MANIFEST: &str = ".safeselect-skills.json";
const LOCK: &str = ".safeselect-skills.lock";
pub const FILES: &[(&str, &str)] = &[
    (
        "safeselect/SKILL.md",
        include_str!("../skills/safeselect/SKILL.md"),
    ),
    (
        "safeselect/references/workflows.md",
        include_str!("../skills/safeselect/references/workflows.md"),
    ),
    (
        "safeselect-setup/SKILL.md",
        include_str!("../skills/safeselect-setup/SKILL.md"),
    ),
    (
        "safeselect-setup/references/workflows.md",
        include_str!("../skills/safeselect-setup/references/workflows.md"),
    ),
    (
        "safeselect-sql/SKILL.md",
        include_str!("../skills/safeselect-sql/SKILL.md"),
    ),
    (
        "safeselect-sql/references/workflows.md",
        include_str!("../skills/safeselect-sql/references/workflows.md"),
    ),
    (
        "safeselect-mongodb/SKILL.md",
        include_str!("../skills/safeselect-mongodb/SKILL.md"),
    ),
    (
        "safeselect-mongodb/references/workflows.md",
        include_str!("../skills/safeselect-mongodb/references/workflows.md"),
    ),
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    owner: String,
    schema: u32,
    version: String,
    files: BTreeMap<String, String>,
}

fn failure(path: &Path, reason: impl std::fmt::Display) -> SafeselectError {
    SafeselectError::Other(format!(
        "Skill registration conflict at {}: {reason}",
        path.display()
    ))
}

pub fn supported(client: &str) -> bool {
    matches!(client, "codex" | "opencode")
}

pub fn root(repo: Option<&Path>, local: bool) -> Result<PathBuf> {
    let base = if local {
        repo.map(Path::to_path_buf)
            .ok_or_else(|| SafeselectError::Other("Project scope requires a project root".into()))?
    } else {
        dirs::home_dir()
            .ok_or_else(|| SafeselectError::Other("Cannot determine home directory".into()))?
    };
    Ok(base.canonicalize()?.join(".agents/skills"))
}

fn desired() -> Manifest {
    Manifest {
        owner: "safeselect".into(),
        schema: 1,
        version: env!("CARGO_PKG_VERSION").into(),
        files: FILES
            .iter()
            .map(|(path, text)| (path.to_string(), hash(text.as_bytes())))
            .collect(),
    }
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn safe(path: &Path, directory: bool) -> Result<()> {
    let Some(meta) = metadata(path)? else {
        return Ok(());
    };
    if meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err(failure(path, "symlink or unexpected file type"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o022 != 0 {
            return Err(failure(path, "group/world-writable permissions"));
        }
    }
    Ok(())
}

fn safe_root(root: &Path) -> Result<()> {
    if let Some(base) = root.parent().and_then(Path::parent) {
        safe(base, true)?;
    }
    safe(
        root.parent()
            .ok_or_else(|| failure(root, "missing parent"))?,
        true,
    )?;
    safe(root, true)
}

fn valid_resource(path: &str) -> bool {
    let mut parts = Path::new(path).components();
    matches!(parts.next(), Some(Component::Normal(name)) if NAMES.iter().any(|known| name == *known))
        && parts.clone().next().is_some()
        && parts.all(|part| matches!(part, Component::Normal(_)))
        && !path.contains('\\')
}

fn inventory(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, String>,
    dirs: &BTreeSet<String>,
) -> Result<()> {
    required_directory(directory)?;
    for entry in fs::read_dir(directory)? {
        inventory_entry(root, &entry?.path(), files, dirs)?;
    }
    Ok(())
}

fn required_directory(directory: &Path) -> Result<()> {
    safe(directory, true)?;
    if metadata(directory)?.is_none() {
        return Err(failure(directory, "missing managed directory"));
    }
    Ok(())
}

fn inventory_entry(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, String>,
    dirs: &BTreeSet<String>,
) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|error| failure(path, error))?
        .to_string_lossy()
        .to_string();
    if meta.is_dir() {
        if !dirs.contains(&relative) {
            return Err(failure(path, "extra directory"));
        }
        inventory(root, path, files, dirs)
    } else {
        safe(path, false)?;
        files.insert(relative, hash(&fs::read(path)?));
        Ok(())
    }
}

fn inspect(root: &Path) -> Result<&'static str> {
    safe_root(root)?;
    let Some(old) = read_manifest(root)? else {
        return Ok("absent");
    };
    verify_manifest(root, &old)?;
    verify_owned_files(root, &old)?;
    Ok(registered_version(&old))
}

fn read_manifest(root: &Path) -> Result<Option<Manifest>> {
    let path = root.join(MANIFEST);
    safe(&path, false)?;
    if metadata(&path)?.is_none() {
        ensure_unclaimed(root)?;
        return Ok(None);
    }
    load_manifest(&path).map(Some)
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    serde_json::from_slice(&fs::read(path)?).map_err(|error| failure(path, error))
}

fn ensure_unclaimed(root: &Path) -> Result<()> {
    for name in NAMES {
        let path = root.join(name);
        if metadata(&path)?.is_some() {
            return Err(failure(
                &path,
                "foreign resource without ownership manifest",
            ));
        }
    }
    Ok(())
}

fn verify_manifest(root: &Path, manifest: &Manifest) -> Result<()> {
    if manifest.owner != "safeselect"
        || manifest.schema != 1
        || manifest.version.is_empty()
        || manifest.files.keys().any(|path| !valid_resource(path))
        || NAMES
            .iter()
            .any(|name| !manifest.files.contains_key(&format!("{name}/SKILL.md")))
    {
        return Err(failure(&root.join(MANIFEST), "invalid ownership manifest"));
    }
    Ok(())
}

fn verify_owned_files(root: &Path, manifest: &Manifest) -> Result<()> {
    let dirs = manifest
        .files
        .keys()
        .flat_map(|path| {
            Path::new(path)
                .ancestors()
                .skip(1)
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(|parent| parent.to_string_lossy().to_string())
        })
        .collect();
    let mut actual = BTreeMap::new();
    for name in NAMES {
        inventory(root, &root.join(name), &mut actual, &dirs)?;
    }
    verify_inventory(root, &manifest.files, &actual)
}

fn registered_version(manifest: &Manifest) -> &'static str {
    let current = desired();
    if manifest.version == current.version && manifest.files == current.files {
        "current"
    } else {
        "outdated"
    }
}

fn verify_inventory(
    root: &Path,
    expected: &BTreeMap<String, String>,
    actual: &BTreeMap<String, String>,
) -> Result<()> {
    for (path, checksum) in expected {
        match actual.get(path) {
            None => return Err(failure(&root.join(path), "missing managed file")),
            Some(found) if found != checksum => {
                return Err(failure(
                    &root.join(path),
                    "edited managed file; preserve local content before retrying",
                ))
            }
            _ => {}
        }
    }
    if let Some(path) = actual.keys().find(|path| !expected.contains_key(*path)) {
        return Err(failure(&root.join(path), "extra managed resource"));
    }
    Ok(())
}

pub fn status_lines(repo: Option<&Path>) -> Vec<String> {
    let mut lines = vec![scope_status(None, false, "user")];
    if let Some(repo) = repo {
        lines.push(scope_status(Some(repo), true, "project"));
    }
    lines
}

fn scope_status(repo: Option<&Path>, local: bool, scope: &str) -> String {
    match root(repo, local) {
        Ok(path) => status(&path, scope),
        Err(error) => format!("  skills: conflict [scope={scope}]: {error}"),
    }
}

pub fn status(root: &Path, scope: &str) -> String {
    match inspect(root) {
        Ok(state) => format!("  skills: {state} [scope={scope}, source=embedded, version={}, path={}] (registration only; client loading unverified)", env!("CARGO_PKG_VERSION"), root.display()),
        Err(error) => format!("  skills: conflict [scope={scope}, path={}]: {error}", root.display()),
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    private_options().open(path)?.write_all(bytes)?;
    Ok(())
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock(root: &Path) -> Result<Lock> {
    safe_root(root)?;
    private_directory(root)?;
    let path = root.join(LOCK);
    OpenOptions::new().write(true).create_new(true).open(&path)
        .map_err(|error| failure(&path, format!("setup locked or unavailable: {error}; verify no setup is active before removing a stale lock")))?;
    Ok(Lock(path))
}

struct Snapshot {
    path: PathBuf,
    original: Option<(Vec<u8>, Permissions)>,
}
impl Snapshot {
    fn capture(path: &Path) -> Result<Self> {
        safe(path, false)?;
        let original = metadata(path)?
            .map(|meta| fs::read(path).map(|bytes| (bytes, meta.permissions())))
            .transpose()?;
        Ok(Self {
            path: path.into(),
            original,
        })
    }
    fn restore(&self) -> Result<()> {
        match &self.original {
            Some((bytes, permissions)) => restore_file(&self.path, bytes, permissions),
            None => remove_created_file(&self.path),
        }
    }
}

fn restore_file(path: &Path, bytes: &[u8], permissions: &Permissions) -> Result<()> {
    let temp = path.with_file_name(format!(".safeselect-restore-{}", uuid::Uuid::new_v4()));
    private_write(&temp, bytes)?;
    fs::set_permissions(&temp, permissions.clone())?;
    fs::rename(temp, path)?;
    Ok(())
}

fn remove_created_file(path: &Path) -> Result<()> {
    if metadata(path)?.is_some() {
        fs::remove_file(path)?;
    }
    Ok(())
}

struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stage(root: &Path) -> Result<Stage> {
    let path = root.join(format!(".safeselect-stage-{}", uuid::Uuid::new_v4()));
    private_directory(&path)?;
    let stage = Stage(path);
    write_bundle(&stage.0.join("new"))?;
    private_directory(&stage.0.join("old"))?;
    Ok(stage)
}

fn write_bundle(directory: &Path) -> Result<()> {
    for (relative, content) in FILES {
        write_resource(&directory.join(relative), content.as_bytes())?;
    }
    private_write(
        &directory.join(MANIFEST),
        &serde_json::to_vec_pretty(&desired())?,
    )
}

fn write_resource(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| failure(path, "missing parent"))?;
    private_directory(parent)?;
    private_write(path, bytes)
}

fn replace(root: &Path, stage: &Stage, changed: &mut Vec<(&'static str, bool)>) -> Result<()> {
    for name in NAMES.into_iter().chain([MANIFEST]) {
        let target = root.join(name);
        let existed = metadata(&target)?.is_some();
        if existed {
            fs::rename(&target, stage.0.join("old").join(name))?;
        }
        changed.push((name, existed));
        fs::rename(stage.0.join("new").join(name), target)?;
    }
    Ok(())
}

fn rollback(root: &Path, stage: &Stage, changed: &[(&str, bool)]) -> Result<()> {
    let mut errors = Vec::new();
    for (name, existed) in changed.iter().rev() {
        let target = root.join(name);
        let restored = (|| -> Result<()> {
            if let Some(meta) = metadata(&target)? {
                if meta.is_dir() {
                    fs::remove_dir_all(&target)?;
                } else {
                    fs::remove_file(&target)?;
                }
            }
            if *existed {
                fs::rename(stage.0.join("old").join(name), target)?;
            }
            Ok(())
        })();
        if let Err(error) = restored {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(failure(root, errors.join("; ")))
    }
}

/// Serializes both clients and restores resources/config after operation failures.
/// This is an operation transaction, not crash or power-loss recovery.
pub fn register_prepared<T>(
    root: &Path,
    config: &Path,
    prepare: impl FnOnce() -> Result<T>,
    operation: impl FnOnce(T) -> Result<()>,
) -> Result<()> {
    let _lock = lock(root)?;
    let state = inspect(root)?;
    let (snapshot, backup) = snapshot_config(config)?;
    let prepared = prepare()?;
    let operation = || operation(prepared);
    if state == "current" {
        return run_operation(operation, &snapshot, &backup);
    }
    register_staged(root, operation, &snapshot, &backup)
}

fn snapshot_config(config: &Path) -> Result<(Snapshot, Snapshot)> {
    verify_config_location(config)?;
    let snapshot = Snapshot::capture(config)?;
    let backup = Snapshot::capture(&config.with_extension("safeselect.bak"))?;
    Ok((snapshot, backup))
}

fn verify_config_location(config: &Path) -> Result<()> {
    let parent = config
        .parent()
        .ok_or_else(|| failure(config, "missing parent"))?;
    safe(parent, true)?;
    if let Some(grandparent) = parent.parent() {
        safe(grandparent, true)?;
    }
    Ok(())
}

fn register_staged(
    root: &Path,
    operation: impl FnOnce() -> Result<()>,
    config: &Snapshot,
    backup: &Snapshot,
) -> Result<()> {
    let stage = stage(root)?;
    let mut changed = Vec::new();
    let result =
        replace(root, &stage, &mut changed).and_then(|()| run_operation(operation, config, backup));
    if let Err(error) = result {
        if let Err(restore_error) = rollback(root, &stage, &changed) {
            let recovery = stage.0.clone();
            std::mem::forget(stage);
            return Err(failure(
                &recovery,
                format!("{error}; rollback failed: {restore_error}; recovery files retained"),
            ));
        }
        return Err(error);
    }
    println!(
        "Skills registered at {}. Reload the client and verify discovery/loading separately.",
        root.display()
    );
    Ok(())
}

fn run_operation(
    operation: impl FnOnce() -> Result<()>,
    config: &Snapshot,
    backup: &Snapshot,
) -> Result<()> {
    if let Err(error) = operation() {
        let restored = [config.restore(), backup.restore()];
        let failures: Vec<_> = restored
            .into_iter()
            .filter_map(std::result::Result::err)
            .map(|error| error.to_string())
            .collect();
        return if failures.is_empty() {
            Err(error)
        } else {
            Err(failure(
                &config.path,
                format!("{error}; config rollback failed: {}", failures.join("; ")),
            ))
        };
    }
    Ok(())
}

#[cfg(test)]
fn register(root: &Path, config: &Path, operation: impl FnOnce() -> Result<()>) -> Result<()> {
    register_prepared(root, config, || Ok(()), |()| operation())
}

#[cfg(test)]
#[path = "agent_skills_test.rs"]
mod tests;
