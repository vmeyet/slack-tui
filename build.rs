use std::path::Path;
use std::process::Command;

const UNKNOWN: &str = "unknown";

fn main() {
    println!("cargo:rustc-env=GIT_HASH={}", git(["rev-parse", "HEAD"]).unwrap_or_else(|| UNKNOWN.to_owned()));
    for path in head_files() {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// The files that change when HEAD moves, so the embedded hash never goes stale.
/// Empty outside a git checkout, which leaves cargo's default "rebuild on any change".
fn head_files() -> Vec<String> {
    let head = git(["rev-parse", "--git-path", "HEAD"]);
    let branch = git(["symbolic-ref", "--quiet", "HEAD"]).and_then(|r| git(["rev-parse", "--git-path", &r]));
    [head, branch].into_iter().flatten().filter(|p| Path::new(p).exists()).collect()
}

fn git<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}
