//! Completing the token being typed, shared by the `:` command line and the input row.
use crate::fuzzy;

/// How many candidates tab-cycling walks through.
const MAX: usize = 50;

/// The candidates for `token`, best first; an empty token offers the head of the list.
fn rank(token: &str, candidates: &[String]) -> Vec<String> {
    if token.is_empty() {
        return candidates.iter().take(MAX).cloned().collect();
    }
    fuzzy::rank(token, candidates.iter().map(|c| (c.as_str(), c))).into_iter().map(|(_, c)| c.clone()).take(MAX).collect()
}

/// The grey text a shell shows after the cursor: the rest of the best candidate for `token`,
/// or nothing when the token is empty or matches nothing. A candidate `token` starts is preferred
/// over a fuzzy match, so typing a name to its end never jumps somewhere else.
pub fn ghost(token: &str, candidates: &[String]) -> Option<String> {
    if token.is_empty() {
        return None;
    }
    let lower = token.to_lowercase();
    let typed = token.chars().count();
    let by_prefix = candidates.iter().find(|c| c.to_lowercase().starts_with(&lower) && c.chars().count() > typed);
    let best = by_prefix.or_else(|| fuzzy::best(token, candidates.iter().map(|c| (c.as_str(), c))))?;
    if !best.to_lowercase().starts_with(&lower) {
        return None;
    }
    // Lowercasing can change byte lengths (İ is 2 bytes, its lowercase 3), so the typed part is skipped by characters.
    let rest = best.char_indices().nth(typed).map_or(best.len(), |(i, _)| i);
    Some(best[rest..].to_owned())
}

/// A token replaced in turn by each of its candidates, the text on both sides of it kept as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Swap {
    before: String,
    after: String,
    options: Vec<String>,
    at: usize,
}

impl Swap {
    /// Starts on the best candidate for `token`, which sits between `before` and `after`, or
    /// nothing when none matches.
    pub fn new(before: &str, token: &str, after: &str, candidates: &[String]) -> Option<Self> {
        let options = rank(token, candidates);
        (!options.is_empty()).then(|| Self { before: before.to_owned(), after: after.to_owned(), options, at: 0 })
    }

    pub fn advance(&mut self, backwards: bool) {
        let n = self.options.len();
        self.at = if backwards { (self.at + n - 1) % n } else { (self.at + 1) % n };
    }

    /// The text up to the end of the candidate in hand, and the rest.
    pub fn text(&self) -> (String, &str) {
        (format!("{}{}", self.before, self.options[self.at]), &self.after)
    }

    /// The row shown while cycling: the option in hand between brackets, then the next few.
    /// `label` is how each one reads on screen, which needs not be the value itself.
    pub fn hint(&self, label: fn(&str) -> String) -> String {
        let shown = self.options.iter().enumerate().skip(self.at).take(4);
        shown.map(|(i, o)| if i == self.at { format!("[{}]", label(o)) } else { label(o) }).collect::<Vec<_>>().join("  ")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn candidates() -> Vec<String> {
        ["rocket", "rock", "racing_car", "tada"].map(String::from).to_vec()
    }

    #[test]
    fn ranking_puts_the_closest_match_first_and_keeps_the_rest() {
        assert_eq!(rank("rock", &candidates())[0], "rock");
        assert!(rank("rock", &candidates()).contains(&"rocket".to_string()));
        assert_eq!(rank("zzz", &candidates()), Vec::<String>::new());
    }

    #[test]
    fn an_empty_token_offers_every_candidate_in_order() {
        assert_eq!(rank("", &candidates()), candidates());
    }

    #[test]
    fn the_ghost_completes_what_the_token_starts() {
        assert_eq!(ghost("rock", &candidates()).as_deref(), Some("et"), "a candidate it starts wins over the fuzzy match");
        assert_eq!(ghost("ROCK", &candidates()).as_deref(), Some("et"), "case does not matter");
        assert_eq!(ghost("", &candidates()), None);
        assert_eq!(ghost("zzz", &candidates()), None);
        assert_eq!(ghost("rcar", &candidates()), None, "a fuzzy match the token does not start suggests nothing");
    }

    #[test]
    fn the_ghost_survives_letters_whose_lowercase_changes_length() {
        let people = vec!["@İlker".to_string()];
        assert_eq!(ghost("@i", &people).as_deref(), Some("lker"));
        assert_eq!(ghost("@İ", &people).as_deref(), Some("lker"));
    }

    fn swap(token: &str) -> Swap {
        Swap::new("go ", token, " now", &candidates()).expect("matches")
    }

    #[test]
    fn cycling_walks_the_candidates_both_ways_and_wraps() {
        let mut swap = swap("rock");
        assert_eq!(swap.text(), ("go rock".to_owned(), " now"));
        swap.advance(false);
        assert_eq!(swap.text(), ("go rocket".to_owned(), " now"), "the text around the token stays put");
        swap.advance(false);
        assert_eq!(swap.text().0, "go rock", "two matches, so it wraps");
        swap.advance(true);
        assert_eq!(swap.text().0, "go rocket");
        assert_eq!(Swap::new("", "zzz", "", &candidates()), None);
    }

    #[test]
    fn the_hint_marks_the_option_in_hand_and_labels_them_all() {
        let mut swap = swap("rock");
        assert_eq!(swap.hint(str::to_owned), "[rock]  rocket");
        assert_eq!(swap.hint(|o| format!("· {o}")), "[· rock]  · rocket");
        swap.advance(false);
        assert_eq!(swap.hint(str::to_owned), "[rocket]");
    }
}
