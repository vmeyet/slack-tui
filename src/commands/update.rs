use crate::cli::UpdateArgs;
use crate::render::Theme;
use crate::update::{REPO, Standing, remote_head, standing};
use crate::version;
use anyhow::{Context, Result, bail};
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    UpToDate,
}

/// Only a commit we know we already run spares the rebuild; anything unanswered installs.
pub fn decide(installed: &str, latest: Option<&str>, force: bool) -> Action {
    match standing(installed, latest) {
        Standing::Current if !force => Action::UpToDate,
        _ => Action::Install,
    }
}

pub fn run(args: UpdateArgs) -> Result<()> {
    let theme = Theme::detect();
    let latest = if args.force { None } else { latest_commit(&theme) };
    match decide(version::COMMIT, latest.as_deref(), args.force) {
        Action::UpToDate => {
            println!("{} already up to date ({})", theme.ok("✓"), version::label());
            Ok(())
        }
        Action::Install => install(&theme),
    }
}

/// Asked fresh, never from the daily cache the TUI hint uses: the user is here for the latest.
fn latest_commit(theme: &Theme) -> Option<String> {
    match remote_head() {
        Ok(commit) => Some(commit),
        Err(err) => {
            eprintln!("{} could not check the latest version: {err}", theme.accent("!"));
            None
        }
    }
}

fn install(theme: &Theme) -> Result<()> {
    println!("{} installing the latest slack from {REPO}…", theme.accent("→"));
    let status = Command::new("cargo").args(["install", "--git", REPO, "--force"]).status().context("running cargo install")?;
    if !status.success() {
        bail!("cargo install failed");
    }
    println!("{} updated, run `slack --version` to see it", theme.ok("✓"));
    Ok(())
}

#[cfg(test)]
mod tests {
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
