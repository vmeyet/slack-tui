use crate::cache::Cache;
use crate::version;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::process::Command;

pub const REPO: &str = "https://github.com/vmeyet/slack-tui";

const CHECK_FILE: &str = "update-check";
const A_DAY: i64 = 24 * 60 * 60;

/// How the running binary compares to the newest commit of the repo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Current,
    Behind,
    /// Built without git history, or the check could not run: nothing can be said.
    Unknown,
}

pub fn standing(installed: &str, latest: Option<&str>) -> Standing {
    match latest {
        _ if installed == version::UNKNOWN => Standing::Unknown,
        Some(latest) if latest == installed => Standing::Current,
        Some(_) => Standing::Behind,
        None => Standing::Unknown,
    }
}

/// The newest commit of the repo, asked at most once a day and remembered between runs.
/// `None` while it cannot be known, which every caller reads as "say nothing".
pub fn latest_commit() -> Option<String> {
    let cache = Cache::shared();
    let last: Option<Check> = cache.load(CHECK_FILE);
    let now = Utc::now().timestamp();
    if !due(last.as_ref(), now) {
        return last.and_then(|c| c.commit);
    }
    let check = Check::now(remote_head().ok(), now);
    let _ = cache.save(CHECK_FILE, &check);
    check.commit
}

/// Remembers what a fresh check found, so the hint agrees with a update that just ran.
pub fn remember(commit: &str) {
    let _ = Cache::shared().save(CHECK_FILE, &Check::now(Some(commit.to_owned()), Utc::now().timestamp()));
}

pub fn remote_head() -> Result<String> {
    let output =
        Command::new("git").args(["ls-remote", REPO, "HEAD"]).env("GIT_TERMINAL_PROMPT", "0").output().context("running git ls-remote")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let listing = String::from_utf8(output.stdout)?;
    let Some(commit) = listing.split_whitespace().next() else { bail!("{REPO} has no HEAD") };
    Ok(commit.to_owned())
}

/// What the last check found, so a failed one also waits a day before trying again.
/// `installed` is the binary that asked: a check made by an older one says nothing about this one.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Check {
    commit: Option<String>,
    at: i64,
    #[serde(default)]
    installed: String,
}

impl Check {
    fn now(commit: Option<String>, at: i64) -> Self {
        Self { commit, at, installed: version::COMMIT.to_owned() }
    }
}

fn due(last: Option<&Check>, now: i64) -> bool {
    last.is_none_or(|last| last.installed != version::COMMIT || now - last.at >= A_DAY)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const INSTALLED: &str = "43d0edda4dd29c731c39a49023a6cc7ac013ba52";
    const NEWER: &str = "635cf1b19af9f04a8789f73d78df1dca02259c35";
    const NOON: i64 = 1_789_660_000;

    fn checked(at: i64) -> Check {
        Check::now(Some(NEWER.into()), at)
    }

    #[test]
    fn the_same_commit_is_current() {
        assert_eq!(standing(INSTALLED, Some(INSTALLED)), Standing::Current);
    }

    #[test]
    fn another_commit_means_behind() {
        assert_eq!(standing(INSTALLED, Some(NEWER)), Standing::Behind);
    }

    #[test]
    fn a_missing_answer_says_nothing() {
        assert_eq!(standing(INSTALLED, None), Standing::Unknown);
    }

    #[test]
    fn a_binary_built_without_git_says_nothing() {
        assert_eq!(standing(version::UNKNOWN, Some(NEWER)), Standing::Unknown);
    }

    #[test]
    fn never_checked_is_due() {
        assert!(due(None, NOON));
    }

    #[test]
    fn a_check_from_today_is_not_due() {
        assert!(!due(Some(&checked(NOON - A_DAY + 1)), NOON));
    }

    #[test]
    fn a_check_from_yesterday_is_due() {
        assert!(due(Some(&checked(NOON - A_DAY)), NOON));
    }

    #[test]
    fn a_check_stamped_in_the_future_is_not_due() {
        assert!(!due(Some(&checked(NOON + A_DAY)), NOON));
    }

    #[test]
    fn a_check_made_by_another_binary_is_due() {
        let older = Check { commit: Some(NEWER.into()), at: NOON, installed: INSTALLED.into() };
        assert!(due(Some(&older), NOON), "an update replaced the binary, so the answer is stale");
    }

    #[test]
    fn a_check_without_a_binary_is_due() {
        let legacy: Check = serde_json::from_str(&format!(r#"{{"commit":"{NEWER}","at":{NOON}}}"#)).unwrap();
        assert!(due(Some(&legacy), NOON), "a cache written before this field says nothing");
    }
}
