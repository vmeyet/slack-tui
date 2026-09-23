//! The app's name on disk, in the keychain and in the environment, and the `slack-cli` name it had before.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Names the config, cache and state folders, the keychain service and the user agent.
pub const NAME: &str = "slack-tui";
/// The name before the rename; still read so existing installs keep working.
pub const LEGACY_NAME: &str = "slack-cli";

/// `SLACK_TUI_<key>`, or the older `SLACK_CLI_<key>`.
pub fn env(key: &str) -> Option<OsString> {
    std::env::var_os(format!("SLACK_TUI_{key}")).or_else(|| std::env::var_os(format!("SLACK_CLI_{key}")))
}

/// `<base>/slack-tui`, taking over `<base>/slack-cli` the first time.
pub fn folder(base: &Path) -> PathBuf {
    let path = base.join(NAME);
    let legacy = base.join(LEGACY_NAME);
    if !path.exists() && legacy.is_dir() {
        let _ = std::fs::rename(&legacy, &path);
    }
    path
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn folder_takes_over_the_legacy_folder_once() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir(base.path().join(LEGACY_NAME)).unwrap();
        std::fs::write(base.path().join(LEGACY_NAME).join("config.toml"), "x").unwrap();
        let path = folder(base.path());
        assert_eq!(path, base.path().join(NAME));
        assert_eq!(std::fs::read_to_string(path.join("config.toml")).unwrap(), "x");
        assert!(!base.path().join(LEGACY_NAME).exists());
        assert_eq!(folder(base.path()), path);
    }

    #[test]
    fn folder_keeps_the_new_folder_when_both_exist() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir(base.path().join(NAME)).unwrap();
        std::fs::create_dir(base.path().join(LEGACY_NAME)).unwrap();
        folder(base.path());
        assert!(base.path().join(LEGACY_NAME).exists());
    }

    #[test]
    fn folder_without_a_legacy_folder_changes_nothing() {
        let base = tempfile::tempdir().unwrap();
        assert_eq!(folder(base.path()), base.path().join(NAME));
        assert!(!base.path().join(NAME).exists());
    }
}
