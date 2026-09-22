//! Regexes written by hand in the source, compiled once at first use.
use regex::Regex;

/// A typo in a hand-written pattern is a bug in the binary, not a runtime error.
#[allow(clippy::expect_used)]
pub(crate) fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("hand-written regex compiles")
}
