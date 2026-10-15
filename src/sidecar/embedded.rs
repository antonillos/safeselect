use crate::error::Result;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn ensure_jar(directory: &Path, embedded: &[u8]) -> Result<PathBuf> {
    let digest = hex::encode(Sha256::digest(embedded));
    let jar = directory.join(format!("safeselect-sidecar-{digest}.jar"));
    match std::fs::read(&jar) {
        Ok(contents) if contents == embedded => return Ok(jar),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    publish_jar(directory, &jar, embedded)?;
    Ok(jar)
}

fn publish_jar(directory: &Path, jar: &Path, embedded: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".sidecar-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(embedded)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        }
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, jar)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_cached_sidecar_without_replacing_older_versions() {
        let directory =
            std::env::temp_dir().join(format!("sidecar-upgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let legacy = directory.join("safeselect-sidecar.jar");
        std::fs::write(&legacy, b"legacy").unwrap();
        let previous = ensure_jar(&directory, b"previous").unwrap();
        let current = ensure_jar(&directory, b"current").unwrap();
        assert_ne!(current, previous);
        assert_ne!(current, legacy);
        assert_eq!(std::fs::read(&current).unwrap(), b"current");
        assert_eq!(std::fs::read(&previous).unwrap(), b"previous");
        assert_eq!(std::fs::read(&legacy).unwrap(), b"legacy");
        assert_eq!(ensure_jar(&directory, b"current").unwrap(), current);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repairs_incomplete_or_modified_cached_jar() {
        let directory =
            std::env::temp_dir().join(format!("sidecar-repair-{}", uuid::Uuid::new_v4()));
        let jar = ensure_jar(&directory, b"embedded contents").unwrap();
        std::fs::write(&jar, b"partial").unwrap();
        assert_eq!(ensure_jar(&directory, b"embedded contents").unwrap(), jar);
        assert_eq!(std::fs::read(jar).unwrap(), b"embedded contents");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unreadable_cache_and_cleans_failed_publication() {
        let directory =
            std::env::temp_dir().join(format!("sidecar-failure-{}", uuid::Uuid::new_v4()));
        let jar = ensure_jar(&directory, b"embedded contents").unwrap();
        std::fs::remove_file(&jar).unwrap();
        std::fs::create_dir(&jar).unwrap();
        assert!(ensure_jar(&directory, b"embedded contents").is_err());
        assert!(publish_jar(&directory, &jar, b"embedded contents").is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        assert!(jar.is_dir());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_extraction_publishes_one_complete_jar() {
        let directory =
            std::env::temp_dir().join(format!("sidecar-parallel-{}", uuid::Uuid::new_v4()));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let directory = directory.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    ensure_jar(&directory, b"embedded contents").unwrap()
                })
            })
            .collect();
        let paths: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert!(paths.iter().all(|path| path == &paths[0]));
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"embedded contents");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
