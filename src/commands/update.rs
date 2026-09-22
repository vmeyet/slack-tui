//! `slack update`: rebuild and install the latest commit.
use crate::cache::Cache;
use crate::cli::UpdateArgs;
use crate::render::Theme;
use crate::update::{self, REPO, Standing, remote_head, standing};
use crate::version;
use anyhow::{Context, Result, bail};
use tokio::process::Command;

const BUILD_FOLDER: &str = "cargo_target";

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Install,
    UpToDate,
}

/// Only a commit we know we already run spares the rebuild; anything unanswered installs.
fn decide(installed: &str, latest: Option<&str>, force: bool) -> Action {
    match standing(installed, latest) {
        Standing::Current if !force => Action::UpToDate,
        _ => Action::Install,
    }
}

/// Rebuilds and installs the latest commit unless the running binary already is it.
pub async fn run(args: &UpdateArgs) -> Result<()> {
    let theme = Theme::detect();
    let latest = if args.force { None } else { latest_commit(&theme).await };
    if let Some(commit) = &latest {
        update::remember(commit).await;
    }
    match decide(version::COMMIT, latest.as_deref(), args.force) {
        Action::UpToDate => {
            println!("{} already up to date ({})", theme.ok("✓"), version::label());
            Ok(())
        }
        Action::Install => install(&theme).await,
    }
}

/// Asked fresh, never from the daily cache the TUI hint uses: the user is here for the latest.
async fn latest_commit(theme: &Theme) -> Option<String> {
    match remote_head().await {
        Ok(commit) => Some(commit),
        Err(err) => {
            eprintln!("{} could not check the latest version: {err}", theme.accent("!"));
            None
        }
    }
}

/// Built in a kept folder so the next update only recompiles what changed.
async fn install(theme: &Theme) -> Result<()> {
    println!("{} installing the latest slack from {REPO}…", theme.accent("→"));
    let build = Cache::shared().folder(BUILD_FOLDER).await?;
    let status = Command::new("cargo")
        .args(["install", "--git", REPO, "--force", "--target-dir"])
        .arg(&build)
        .status()
        .await
        .context("running cargo install")?;
    if !status.success() {
        bail!("cargo install failed");
    }
    println!("{} updated, run `slack --version` to see it", theme.ok("✓"));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const INSTALLED: &str = "9731436a0e7c4d1b2f3a4b5c6d7e8f9a0b1c2d3e";
    const NEWER: &str = "635cf1b0000000000000000000000000000000ff";

    #[test]
    fn same_commit_needs_no_install() {
        assert_eq!(decide(INSTALLED, Some(INSTALLED), false), Action::UpToDate);
    }

    #[test]
    fn a_newer_commit_installs() {
        assert_eq!(decide(INSTALLED, Some(NEWER), false), Action::Install);
    }

    #[test]
    fn force_installs_over_the_same_commit() {
        assert_eq!(decide(INSTALLED, Some(INSTALLED), true), Action::Install);
    }

    #[test]
    fn a_failed_check_installs() {
        assert_eq!(decide(INSTALLED, None, false), Action::Install);
    }

    #[test]
    fn an_unknown_installed_commit_installs() {
        assert_eq!(decide(version::UNKNOWN, Some(version::UNKNOWN), false), Action::Install);
    }
}
