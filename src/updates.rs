use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DAY: u64 = 24 * 60 * 60;
const RELEASES: &str = "https://github.com/antonillos/safeselect/releases/latest";

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    latest: Option<String>,
    notified_at: u64,
    notified_version: Option<String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn newer(latest: &str, current: &str) -> bool {
    match (
        semver::Version::parse(latest.trim_start_matches('v')),
        semver::Version::parse(
            current
                .split_whitespace()
                .next()
                .unwrap_or(current)
                .trim_start_matches('v'),
        ),
    ) {
        (Ok(latest), Ok(current)) => latest.pre.is_empty() && latest > current,
        _ => false,
    }
}

fn fetch_latest() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("safeselect/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let response = client
        .get("https://api.github.com/repos/antonillos/safeselect/releases/latest")
        .send()
        .ok()?
        .error_for_status()
        .ok()?;
    parse_release(response)
}

fn parse_release(reader: impl Read) -> Option<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let mut bytes = Vec::new();
    reader.take(64 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    let release: Release = serde_json::from_slice(&bytes).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    stable_version(&release.tag_name)
}

fn stable_version(tag: &str) -> Option<String> {
    let version = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    if !version.pre.is_empty() {
        return None;
    }
    Some(version.to_string())
}

fn update_hint(executable: &Path) -> String {
    let path = executable.to_string_lossy();
    if path.contains("/Cellar/safeselect/") || path.contains("/linuxbrew/") {
        "brew update && brew upgrade antonillos/tap/safeselect".into()
    } else if path.contains("/installs/safeselect/") {
        "asdf install safeselect latest && asdf set -u safeselect latest && asdf reshim safeselect"
            .into()
    } else if let Some(command) = installed_source_hint(executable) {
        command
    } else if dirs::home_dir().is_some_and(|home| executable == home.join(".local/bin/safeselect"))
    {
        "curl -fsSL https://raw.githubusercontent.com/antonillos/safeselect/main/packaging/install/install-release.sh | sh".into()
    } else {
        format!("Download the matching binary from {RELEASES} (or use your original installation method).")
    }
}

fn installed_source_hint(executable: &Path) -> Option<String> {
    source_update_hint(
        Path::new(option_env!("SAFESELECT_SOURCE_DIR")?),
        executable,
        option_env!("SAFESELECT_SOURCE_MODE").unwrap_or("release"),
        option_env!("SAFESELECT_SOURCE_BRANCH").unwrap_or_default(),
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn source_update_hint(
    directory: &Path,
    executable: &Path,
    mode: &str,
    branch: &str,
) -> Option<String> {
    if !valid_branch(branch) {
        return Some(format!(
            "Source branch unavailable. Update manually from {RELEASES}."
        ));
    }
    if !source_checkout_exists(directory) {
        return None;
    }
    let destination = executable.parent()?.to_str()?;
    let directory = directory.to_str()?;
    if [directory, destination]
        .iter()
        .any(|path| path.contains(['\n', '\r']))
    {
        return None;
    }
    let mode = build_mode_flag(mode);
    Some(format!(
        "cd {} && BIN_DIR={} ./install.sh --update {mode} --branch {}",
        shell_quote(directory),
        shell_quote(destination),
        shell_quote(branch)
    ))
}

fn build_mode_flag(mode: &str) -> &str {
    if mode == "debug" {
        "--debug"
    } else {
        "--release"
    }
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty() && !branch.contains(['\n', '\r'])
}

fn source_checkout_exists(directory: &Path) -> bool {
    directory.join("install.sh").is_file() && directory.join(".git").exists()
}

fn notice(latest: &str) -> String {
    let executable = std::env::current_exe().unwrap_or_default();
    format!(
        "SafeSelect {} → {latest} available. Update: {}",
        crate::cli::VERSION,
        update_hint(&executable)
    )
}

pub(crate) fn explicit_check() -> String {
    check_with(fetch_latest)
}

fn check_with(fetch: impl FnOnce() -> Option<String>) -> String {
    match fetch() {
        Some(latest) if newer(&latest, crate::cli::VERSION) => notice(&latest),
        Some(latest) => format!(
            "SafeSelect {}: no newer stable release (latest: {latest}).",
            crate::cli::VERSION
        ),
        None => format!("Could not check for updates. Try again later or visit {RELEASES}."),
    }
}

pub(crate) fn automatic_notice() -> Option<String> {
    if std::env::var_os("SAFESELECT_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    cached_notice(&dirs::cache_dir()?.join("safeselect"), now(), fetch_latest)
}

fn cached_notice(
    directory: &Path,
    time: u64,
    fetch: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let path = directory.join("update-check.json");
    let mut cache: Cache = std::fs::read(&path)
        .ok()
        .filter(|bytes| bytes.len() <= 4096)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    // Do not make recurring network requests when the cache cannot be written.
    std::fs::create_dir_all(directory).ok()?;
    if time.saturating_sub(cache.checked_at) >= DAY {
        cache.checked_at = time;
        std::fs::write(&path, serde_json::to_vec(&cache).ok()?).ok()?;
        cache.latest = fetch();
    }
    let result = cache.latest.as_deref().and_then(|latest| {
        if should_notify(&cache, latest, crate::cli::VERSION, time) {
            Some(notice(latest))
        } else {
            None
        }
    });
    if result.is_some() {
        cache.notified_at = time;
        cache.notified_version = cache.latest.clone();
    }
    std::fs::write(path, serde_json::to_vec(&cache).ok()?).ok()?;
    result
}

fn should_notify(cache: &Cache, latest: &str, current: &str, time: u64) -> bool {
    newer(latest, current)
        && (cache.notified_version.as_deref() != Some(latest)
            || time.saturating_sub(cache.notified_at) >= 7 * DAY)
}

pub(crate) fn mcp_instructions(notice: Option<&str>) -> String {
    notice.map(|notice| format!("{notice} Mention this once briefly at a convenient pause. Do not interrupt database work or run the update without the user's request.")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_metadata_accepts_only_bounded_stable_releases() {
        let release = |tag: &str, draft: bool, prerelease: bool| {
            serde_json::json!({
                "tag_name": tag, "draft": draft, "prerelease": prerelease
            })
            .to_string()
        };
        assert_eq!(
            parse_release(release("v1.2.3", false, false).as_bytes()),
            Some("1.2.3".into())
        );
        for (tag, draft, prerelease) in [
            ("1.2.3", true, false),
            ("1.2.3", false, true),
            ("1.2.3-rc.1", false, false),
            ("invalid", false, false),
        ] {
            assert!(parse_release(release(tag, draft, prerelease).as_bytes()).is_none());
        }
        assert!(parse_release(b"invalid JSON".as_slice()).is_none());
        assert!(parse_release(vec![b' '; 64 * 1024 + 1].as_slice()).is_none());
        struct FailedReader;
        impl Read for FailedReader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("read failed"))
            }
        }
        assert!(parse_release(FailedReader).is_none());
    }

    #[test]
    fn explicit_check_reports_update_current_and_unavailable_states() {
        assert!(check_with(|| Some("999.0.0".into())).contains("Update:"));
        assert!(check_with(|| Some("0.0.0".into())).contains("no newer stable release"));
        assert!(check_with(|| None).contains("Could not check"));
    }

    #[test]
    fn compares_semantic_versions_and_rejects_untrusted_tags() {
        assert!(newer("v0.10.0", "0.9.0"));
        assert!(newer("0.9.0", "0.8.0 (2026.10.23.23.00)"));
        assert!(!newer("0.8.0", "0.8.0"));
        assert!(!newer("0.7.0", "0.8.0"));
        assert!(!newer("1.0.0-rc.1", "0.8.0"));
        assert!(!newer("1.0.0\nrun something", "0.8.0"));
        assert!(!newer("1.0.0", "unknown"));
    }

    #[test]
    fn suppresses_repeated_notices_but_announces_new_releases() {
        let cache = Cache {
            checked_at: DAY,
            latest: Some("0.9.0".into()),
            notified_at: DAY,
            notified_version: Some("0.9.0".into()),
        };
        assert!(!should_notify(&cache, "0.9.0", "0.8.0", 2 * DAY));
        assert!(should_notify(&cache, "0.9.0", "0.8.0", 8 * DAY));
        assert!(should_notify(&cache, "0.10.0", "0.8.0", 2 * DAY));
        assert!(!should_notify(&cache, "0.9.0", "0.9.0", 8 * DAY));
    }

    #[test]
    fn update_commands_follow_installation_method() {
        assert_eq!(
            update_hint(Path::new(
                "/opt/homebrew/Cellar/safeselect/0.8.0/bin/safeselect"
            )),
            "brew update && brew upgrade antonillos/tap/safeselect"
        );
        assert!(update_hint(Path::new(
            "/opt/asdf/installs/safeselect/0.8.0/bin/safeselect"
        ))
        .starts_with("asdf install"));
        assert!(update_hint(Path::new("/custom/bin/safeselect")).contains(RELEASES));
        assert!(!update_hint(Path::new("/custom/bin/safeselect")).contains("curl"));
    }

    #[test]
    fn caches_checks_and_notices_and_throttles_failures() {
        let directory =
            std::env::temp_dir().join(format!("safeselect-updates-{}", uuid::Uuid::new_v4()));
        assert!(cached_notice(&directory, DAY, || Some("999.0.0".into())).is_some());
        assert!(cached_notice(&directory, DAY + 1, || panic!("must use cache")).is_none());
        assert!(cached_notice(&directory, 2 * DAY, || None).is_none());
        assert!(
            cached_notice(&directory, 2 * DAY + 1, || panic!("must throttle failures")).is_none()
        );
        assert!(cached_notice(&directory, 3 * DAY, || Some("999.0.1".into())).is_some());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn source_update_preserves_destination_and_quotes_paths() {
        let directory =
            std::env::temp_dir().join(format!("safeselect source's-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(directory.join(".git")).unwrap();
        std::fs::write(directory.join("install.sh"), "").unwrap();
        let command = source_update_hint(
            &directory,
            Path::new("/custom install/bin/safeselect"),
            "debug",
            "main",
        )
        .unwrap();
        assert!(command.contains("'\"'\"'"));
        assert!(command.ends_with("--branch 'main'"));
        assert!(
            source_update_hint(&directory, Path::new("/bin/safeselect"), "release", "")
                .unwrap()
                .contains("Update manually")
        );
        assert!(command.contains("BIN_DIR='/custom install/bin' ./install.sh --update --debug"));
        std::fs::remove_file(directory.join("install.sh")).unwrap();
        assert!(
            source_update_hint(&directory, Path::new("/bin/safeselect"), "release", "main")
                .is_none()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn mcp_hint_is_quiet_without_an_update() {
        assert!(mcp_instructions(None).is_empty());
        assert!(mcp_instructions(Some("Update available")).contains("Do not interrupt"));
    }
}
