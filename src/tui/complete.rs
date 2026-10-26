//! Completing the token being typed, shared by the `:` command line and the input row.
use vtui::complete::Cycle;
use vtui::toward::Toward;

/// A token replaced in turn by each of its candidates, the text on both sides of it kept as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Swap {
    before: String,
    after: String,
    cycle: Cycle,
}

impl Swap {
    /// Starts on the best candidate for `token`, which sits between `before` and `after`, or
    /// nothing when none matches.
    pub fn new(before: &str, token: &str, after: &str, candidates: &[String]) -> Option<Self> {
        let cycle = Cycle::new(token, candidates)?;
        Some(Self { before: before.to_owned(), after: after.to_owned(), cycle })
    }

    pub fn advance(&mut self, toward: Toward) {
        self.cycle.advance(toward);
    }

    /// The text up to the end of the candidate in hand, and the rest.
    pub fn text(&self) -> (String, &str) {
        (format!("{}{}", self.before, self.cycle.current()), &self.after)
    }

    /// The row shown while cycling: the option in hand between brackets, then the next few.
    /// `label` is how each one reads on screen, which needs not be the value itself.
    pub fn hint(&self, label: fn(&str) -> String) -> String {
        self.cycle.hint(label)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn swap(token: &str) -> Swap {
        let candidates = ["rocket", "rock", "racing_car", "tada"].map(String::from);
        Swap::new("go ", token, " now", &candidates).expect("matches")
    }

    #[test]
    fn cycling_keeps_the_text_around_the_token() {
        let mut swap = swap("rock");
        assert_eq!(swap.text(), ("go rock".to_owned(), " now"));
        swap.advance(Toward::Next);
        assert_eq!(swap.text(), ("go rocket".to_owned(), " now"));
    }

    #[test]
    fn no_match_starts_no_swap() {
        assert_eq!(Swap::new("", "zzz", "", &["rock".to_owned()]), None);
    }
}
