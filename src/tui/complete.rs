//! Completing the token being typed, shared by the `:` command line and the input row.
use crate::fuzzy;

/// How many candidates tab-cycling walks through.
const MAX: usize = 50;

/// The candidates for `token`, best first; an empty token offers the head of the list.
pub fn rank(token: &str, candidates: &[String]) -> Vec<String> {
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
    let by_prefix = candidates.iter().find(|c| c.to_lowercase().starts_with(&lower) && c.len() > token.len());
    let best = by_prefix.or_else(|| fuzzy::best(token, candidates.iter().map(|c| (c.as_str(), c))))?;
    if best.to_lowercase().starts_with(&lower) { Some(best[token.len()..].to_owned()) } else { None }
}

/// The candidates one token is being cycled through, until the text changes again.
#[derive(Clone, Debug, PartialEq)]
pub struct Cycle {
    options: Vec<String>,
    at: usize,
}

impl Cycle {
    /// Starts on the best candidate for `token`, or nothing when none matches.
    pub fn new(token: &str, candidates: &[String]) -> Option<Self> {
        let options = rank(token, candidates);
        (!options.is_empty()).then_some(Self { options, at: 0 })
    }

    pub fn advance(&mut self, backwards: bool) {
        let n = self.options.len();
        self.at = if backwards { (self.at + n - 1) % n } else { (self.at + 1) % n };
    }

    pub fn current(&self) -> &str {
        &self.options[self.at]
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
    fn cycling_walks_the_candidates_both_ways_and_wraps() {
        let mut cycle = Cycle::new("rock", &candidates()).expect("matches");
        assert_eq!(cycle.current(), "rock");
        cycle.advance(false);
        assert_eq!(cycle.current(), "rocket");
        cycle.advance(false);
        assert_eq!(cycle.current(), "rock", "two matches, so it wraps");
        cycle.advance(true);
        assert_eq!(cycle.current(), "rocket");
        assert_eq!(Cycle::new("zzz", &candidates()), None);
    }

    #[test]
    fn the_hint_marks_the_option_in_hand_and_labels_them_all() {
        let cycle = Cycle::new("rock", &candidates()).expect("matches");
        assert_eq!(cycle.hint(str::to_owned), "[rock]  rocket");
        assert_eq!(cycle.hint(|o| format!("· {o}")), "[· rock]  · rocket");
    }
}
