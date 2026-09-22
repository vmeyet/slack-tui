use std::sync::LazyLock;

/// The commit this binary was built from, or [`UNKNOWN`] when built without git history.
pub const COMMIT: &str = env!("GIT_HASH");

pub const UNKNOWN: &str = "unknown";

/// `0.1.0 (a1b2c3d)`, as shown by `slack --version`.
pub fn label() -> &'static str {
    static LABEL: LazyLock<String> = LazyLock::new(|| format!("{} ({})", env!("CARGO_PKG_VERSION"), short(COMMIT)));
    &LABEL
}

pub fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn short_keeps_the_first_seven_characters() {
        assert_eq!(short("9731436a0e7c4d1b2f3a4b5c6d7e8f9a0b1c2d3e"), "9731436");
    }

    #[test]
    fn short_leaves_an_unknown_commit_readable() {
        assert_eq!(short(UNKNOWN), UNKNOWN);
    }

    #[test]
    fn short_leaves_a_shorter_commit_untouched() {
        assert_eq!(short("abc12"), "abc12");
    }

    #[test]
    fn label_carries_the_crate_version() {
        assert!(label().starts_with(env!("CARGO_PKG_VERSION")));
    }
}
