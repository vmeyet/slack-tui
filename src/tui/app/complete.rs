//! Tab in a message completes the word under the cursor: `:` an emoji, `@` a person, `#` a
//! channel. The text goes out as typed; sending turns `@handle` and `#name` into mentions.
use super::commands::emoji_names;
use super::{App, Kind, Overlay};
use crate::tui::complete::Swap;
use crate::tui::field::Field;

/// What a word completes to, from the mark it starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Emoji,
    Person,
    Channel,
}

/// The mark of `word` and the name after it. An emoji needs a name, so `:)` stays a smiley; its
/// closing `:` is left out so a finished one completes again.
fn token(word: &str) -> Option<(Mark, &str)> {
    let mut chars = word.chars();
    let mark = match chars.next()? {
        ':' => Mark::Emoji,
        '@' => Mark::Person,
        '#' => Mark::Channel,
        _ => return None,
    };
    let name = chars.as_str();
    if mark != Mark::Emoji {
        return Some((mark, name));
    }
    let name = name.strip_suffix(':').unwrap_or(name);
    let is_name = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || "_+-".contains(c));
    is_name.then_some((mark, name))
}

/// How a candidate reads in the hint: an emoji as its glyph and name, like the picker's search.
fn label(candidate: &str) -> String {
    let Some(name) = candidate.strip_prefix(':').and_then(|c| c.strip_suffix(':')) else { return candidate.to_owned() };
    crate::emoji::glyph(name).map_or_else(|| candidate.to_owned(), |glyph| format!("{glyph} {name}"))
}

impl App {
    /// Tab puts the next candidate in place of the word, shift-tab the one before; `cycling` is
    /// the completion the previous key left, if that key was a tab too.
    pub(super) fn complete(&mut self, cycling: Option<Swap>, backwards: bool) {
        let advanced = cycling.map(|mut swap| {
            swap.advance(backwards);
            swap
        });
        let Some(swap) = advanced.or_else(|| self.start_completion()) else { return };
        let (before, after) = swap.text();
        let gap = if after.is_empty() { " " } else { "" };
        self.buffer = Field::joined(&format!("{before}{gap}"), after);
        self.completion = Some(swap);
    }

    /// The row shown while tab cycles: the candidate in hand, then the next few.
    pub fn completion_hint(&self) -> Option<String> {
        Some(self.completion.as_ref()?.hint(label))
    }

    fn start_completion(&self) -> Option<Swap> {
        let Some(Overlay::Input(input)) = &self.overlay else { return None };
        if !input.is_message() {
            return None;
        }
        let (before, word, after) = self.buffer.word();
        let (mark, name) = token(word)?;
        Swap::new(before, name, after, &self.candidates(mark))
    }

    /// Each candidate as it is typed. A channel goes by its bare name, the one sending resolves.
    fn candidates(&self, mark: Mark) -> Vec<String> {
        match mark {
            Mark::Emoji => emoji_names().iter().chain(&self.custom_emoji).map(|name| format!(":{name}:")).collect(),
            Mark::Person => self.people.iter().map(|(_, handle)| format!("@{handle}")).collect(),
            Mark::Channel => self
                .channels
                .iter()
                .filter(|c| matches!(c.kind, Kind::Public | Kind::Private))
                .map(|c| format!("#{}", c.label.trim_start_matches(['#', '🔒'])))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_completes_from_the_mark_it_starts_with() {
        assert_eq!(token(":roc"), Some((Mark::Emoji, "roc")));
        assert_eq!(token(":rocket:"), Some((Mark::Emoji, "rocket")));
        assert_eq!(token(":+1"), Some((Mark::Emoji, "+1")));
        assert_eq!(token("@bo"), Some((Mark::Person, "bo")));
        assert_eq!(token("@"), Some((Mark::Person, "")));
        assert_eq!(token("#gen"), Some((Mark::Channel, "gen")));
        assert_eq!(token("rocket"), None);
        assert_eq!(token(""), None);
    }

    #[test]
    fn a_colon_needs_a_name_after_it() {
        assert_eq!(token(":"), None);
        assert_eq!(token("::"), None);
        assert_eq!(token(":)"), None);
        assert_eq!(token(":-)"), None);
        assert_eq!(token("10:30"), None);
    }

    #[test]
    fn an_emoji_reads_as_its_glyph_and_name_the_rest_as_typed() {
        assert_eq!(label(":rocket:"), "🚀 rocket");
        assert_eq!(label(":partyparrot:"), ":partyparrot:");
        assert_eq!(label("@bob"), "@bob");
        assert_eq!(label("#general"), "#general");
    }
}
