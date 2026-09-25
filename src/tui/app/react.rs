//! `+`: react to the selected message from a strip of eight — the reactions already on it, then
//! the ones used most — or with any emoji found by name after `/`. Picking one of mine takes it
//! off. The count moves at once; a refusal puts it back.
use super::commands::emoji_names;
use super::{Action, App};
use crate::api::Reaction;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::HashMap;

/// How many choices the row shows at once.
pub const PAGE: usize = 8;

/// What fills the strip before anything was ever used.
const STARTERS: [&str; PAGE] = ["+1", "white_check_mark", "eyes", "tada", "heart", "joy", "pray", "rocket"];

/// The picker open on message `ts`, `selected` the choice `enter` gives.
/// With a `search`, the choices are the emoji whose name matches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub channel: String,
    pub ts: String,
    pub strip: Vec<String>,
    pub selected: usize,
    pub search: Option<Search>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub found: Vec<String>,
}

impl Pick {
    pub fn choices(&self) -> &[String] {
        self.search.as_ref().map_or(&self.strip, |s| &s.found)
    }
}

impl Search {
    fn of(query: String, custom: &[String]) -> Self {
        let names = emoji_names().iter().chain(custom).map(|name| (name.as_str(), name));
        let found = crate::fuzzy::rank(&query, names).into_iter().map(|(_, name)| name.clone()).collect();
        Self { query, found }
    }
}

/// The reactions already on the message, then the most used, then the starters; no repeats.
fn strip(on_message: &[Reaction], favorites: &HashMap<String, u32>) -> Vec<String> {
    let mut used: Vec<(&String, &u32)> = favorites.iter().collect();
    used.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let names = on_message.iter().map(|r| r.name.as_str()).chain(used.into_iter().map(|(name, _)| name.as_str())).chain(STARTERS);
    let mut strip: Vec<String> = Vec::with_capacity(PAGE);
    for name in names {
        if strip.len() == PAGE {
            break;
        }
        if !strip.iter().any(|s| s == name) {
            strip.push(name.to_owned());
        }
    }
    strip
}

impl App {
    /// `+`: the picker on the selected message.
    pub(super) fn open_react(&mut self) {
        let Some((channel, ts)) = self.selected_ref() else { return };
        let on_message = self.selected_message().filter(|m| m.ts == ts).map_or(&[][..], |m| &m.reactions);
        let strip = strip(on_message, &self.favorites);
        self.react = Some(Pick { channel, ts, strip, selected: 0, search: None });
    }

    pub(super) fn handle_react_key(&mut self, key: KeyEvent) -> Vec<Action> {
        let Some(pick) = self.react.take() else { return vec![] };
        let last = pick.choices().len().saturating_sub(1);
        let moved = |selected: usize| Some(Pick { selected, ..pick.clone() });
        let searching = |query: String| Some(Pick { selected: 0, search: Some(Search::of(query, &self.custom_emoji)), ..pick.clone() });
        let (next, chosen) = match (key.code, &pick.search) {
            (KeyCode::Enter, _) => (None, pick.choices().get(pick.selected).cloned()),
            (KeyCode::Right, _) | (KeyCode::Char('l'), None) => (moved((pick.selected + 1).min(last)), None),
            (KeyCode::Left, _) | (KeyCode::Char('h'), None) => (moved(pick.selected.saturating_sub(1)), None),
            (KeyCode::Char(c @ '1'..='8'), None) => (None, pick.strip.get(c as usize - '1' as usize).cloned()),
            (KeyCode::Char('/'), None) => (searching(String::new()), None),
            (KeyCode::Backspace, Some(search)) if search.query.is_empty() => {
                (Some(Pick { selected: 0, search: None, ..pick.clone() }), None)
            }
            (KeyCode::Backspace, Some(search)) => {
                let mut query = search.query.clone();
                query.pop();
                (searching(query), None)
            }
            (KeyCode::Char(c), Some(search)) => (searching(format!("{}{c}", search.query)), None),
            _ => (None, None),
        };
        self.react = next;
        chosen.map(|name| self.toggle_reaction(pick.channel, pick.ts, name)).unwrap_or_default()
    }

    /// Mine on or off, shown at once, then asked of Slack; putting one on counts it as used.
    pub(super) fn toggle_reaction(&mut self, channel: String, ts: String, name: String) -> Vec<Action> {
        let on = !self.is_mine(&ts, &name);
        self.show_reaction(&ts, &name, on);
        let react = Action::React { channel, ts, name: name.clone(), on };
        if !on {
            return vec![react];
        }
        *self.favorites.entry(name).or_default() += 1;
        vec![react, Action::SaveFavorites(self.favorites.clone())]
    }

    /// Slack refused: the count goes back to what it was.
    pub(super) fn react_failed(&mut self, ts: &str, name: &str, on: bool, error: &str) {
        self.show_reaction(ts, name, !on);
        self.fail(format!("no reaction: {error}"));
    }

    pub(in crate::tui) fn is_mine(&self, ts: &str, name: &str) -> bool {
        let thread = self.thread.iter().flat_map(|t| &t.messages);
        let mut reactions = self.messages.iter().chain(thread).filter(|m| m.ts == ts).flat_map(|m| &m.reactions);
        reactions.any(|r| r.name == name && r.users.contains(&self.me))
    }

    fn show_reaction(&mut self, ts: &str, name: &str, on: bool) {
        let me = self.me.clone();
        for m in self.all_messages_mut().into_iter().filter(|m| m.ts == ts) {
            super::live::adjust_reaction(&mut m.reactions, name, &me, on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reaction(name: &str) -> Reaction {
        Reaction { name: name.into(), count: 1, users: vec!["U2".into()] }
    }

    #[test]
    fn strip_puts_the_message_first_then_the_most_used_then_the_starters() {
        let favorites = HashMap::from([("fire".to_owned(), 3), ("eyes".to_owned(), 5), ("tada".to_owned(), 3)]);
        let strip = strip(&[reaction("rocket"), reaction("eyes")], &favorites);
        assert_eq!(strip, ["rocket", "eyes", "fire", "tada", "+1", "white_check_mark", "heart", "joy"]);
    }

    #[test]
    fn search_finds_custom_emoji_too() {
        let search = Search::of("partyp".into(), &["partyparrot".into()]);
        assert_eq!(search.found.first().map(String::as_str), Some("partyparrot"));
    }
}
