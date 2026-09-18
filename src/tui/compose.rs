//! Writing a message in `$EDITOR`: the draft goes to a temp file, what comes back is sent.
use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, ExitStatus};

const FALLBACK: &str = "vi";

/// Opens the draft in the user's editor and gives back the message to send, or `None` when
/// the user backed out. The temp file is gone either way.
pub fn edit(draft: &str) -> Result<Option<String>> {
    let file = tempfile::Builder::new().prefix("slack-").suffix(".md").tempfile()?;
    std::fs::write(file.path(), draft)?;
    let status = Editor::from_env().open(file.path())?;
    let edited = std::fs::read_to_string(file.path())?;
    Ok(to_send(draft, &edited, status.success()))
}

/// Nothing to send when the editor gave up, the file came back empty, or the draft is untouched.
fn to_send(draft: &str, edited: &str, editor_ok: bool) -> Option<String> {
    let text = edited.trim();
    let changed = text != draft.trim();
    (editor_ok && changed && !text.is_empty()).then(|| text.to_owned())
}

/// The editor to open, and the arguments it was configured with, as in `EDITOR="code -w"`.
#[derive(Debug, PartialEq, Eq)]
struct Editor {
    program: String,
    args: Vec<String>,
}

impl Editor {
    fn from_env() -> Self {
        Self::pick(var("VISUAL"), var("EDITOR"))
    }

    fn pick(visual: Option<String>, editor: Option<String>) -> Self {
        let command = visual.or(editor).unwrap_or_default();
        let mut words = command.split_whitespace();
        let program = words.next().unwrap_or(FALLBACK).to_owned();
        Self { program, args: words.map(str::to_owned).collect() }
    }

    fn open(&self, path: &Path) -> Result<ExitStatus> {
        Command::new(&self.program).args(&self.args).arg(path).status().with_context(|| format!("could not run {}", self.program))
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(program: &str, args: &[&str]) -> Editor {
        Editor { program: program.into(), args: args.iter().map(|a| (*a).to_owned()).collect() }
    }

    #[test]
    fn visual_comes_first_then_editor_then_vi() {
        assert_eq!(Editor::pick(Some("emacs".into()), Some("nano".into())), editor("emacs", &[]));
        assert_eq!(Editor::pick(None, Some("nano".into())), editor("nano", &[]));
        assert_eq!(Editor::pick(None, None), editor("vi", &[]));
        assert_eq!(Editor::pick(Some("  ".into()), None), editor("vi", &[]));
    }

    #[test]
    fn an_editor_keeps_the_arguments_it_was_given() {
        assert_eq!(Editor::pick(None, Some("code -w --new-window".into())), editor("code", &["-w", "--new-window"]));
    }

    #[test]
    fn a_changed_file_is_sent_without_its_surrounding_blank() {
        assert_eq!(to_send("", "hello\n", true), Some("hello".into()));
        assert_eq!(to_send("hi", "\nhi there\n\n", true), Some("hi there".into()));
        assert_eq!(to_send("a", "one\ntwo", true), Some("one\ntwo".into()));
    }

    #[test]
    fn nothing_is_sent_when_the_editor_failed_the_file_is_empty_or_the_draft_is_untouched() {
        assert_eq!(to_send("", "hello", false), None);
        assert_eq!(to_send("", "  \n ", true), None);
        assert_eq!(to_send("hi", "", true), None);
        assert_eq!(to_send("hi", "hi\n", true), None);
    }
}
