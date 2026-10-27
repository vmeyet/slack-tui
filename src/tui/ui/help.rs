//! The `?` overlay: every key, grouped the way a reader looks for them, then how a message is written. vtui draws it.
use ratatui::Frame;
use ratatui::layout::Rect;
use vtui::help::{Help, Row, Section};
use vtui::theme::Theme;

/// Every browse key, grouped the way a reader looks for them.
const HELP_GROUPS: [(&str, &[(&str, &str)]); 4] = [
    (
        "move",
        &[
            ("tab", "cycle panes"),
            ("j k", "move down · up"),
            ("g G", "top · bottom"),
            ("→ / l", "open channel · open the message's thread"),
            ("← / h", "close thread · back to channels"),
            ("enter", "open channel · open thread · jump to result"),
        ],
    ),
    (
        "message",
        &[
            ("r", "reply in the focused conversation"),
            ("t", "reply in the selected message's thread"),
            ("e", "rewrite your own message"),
            ("^e", "write the message in $EDITOR"),
            ("tab", "while writing: complete :emoji @person #channel"),
            ("+", "react: 1-8 from the strip, / to search, again to take it off"),
            ("o / y", "open in Slack · copy permalink"),
            ("u", "open the message's link in the browser"),
            (":delete", "delete your own message, after a yes"),
        ],
    ),
    (
        "find",
        &[
            ("s", "search messages"),
            ("/", "filter channels"),
            ("⌘k / ^k", "jump to a channel, person or thread · > searches"),
            ("i", "inbox: unread DMs, mentions, thread replies"),
            ("p", "promises: follow-ups you said you would do, still open"),
            ("f", "firehose: every channel as one live ticker, + to react"),
        ],
    ),
    (
        "modes",
        &[
            ("z", "reading mode: one centered conversation, nothing else"),
            (":", "command line: :join :go :msg :react :search :export :read …"),
            ("R", "refresh"),
            ("esc", "close thread · clear search or filter"),
            ("q q", "quit (ctrl-c twice works too)"),
        ],
    ),
];

/// How a message is written: what is typed, and what it becomes.
const MARKDOWN: [(&str, &str); 4] =
    [("**bold**", "bold"), ("_italic_", "italic"), ("`code`", "code"), ("@user #channel", "a mention · a channel link")];

fn sections() -> Vec<Section> {
    let groups = HELP_GROUPS.iter().map(|(title, bindings)| section(title, bindings));
    groups.chain([section("markdown", &MARKDOWN)]).collect()
}

fn section(title: &'static str, rows: &[(&str, &'static str)]) -> Section {
    Section { title, rows: rows.iter().map(|(keys, does)| Row { keys: (*keys).to_owned(), does }).collect() }
}

/// The last row the list can scroll to.
pub fn last_row() -> usize {
    vtui::help::last_row(&sections())
}

pub(super) fn draw(f: &mut Frame, theme: &Theme, area: Rect, help: Help) {
    vtui::help::draw(f, theme, area, help, &sections());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::tui::app::{App, Overlay};
    use crate::tui::ui::testing::{cells, render_at};

    fn help_rows(width: u16, height: u16) -> Vec<String> {
        let mut app = App::new();
        app.overlay = Some(Overlay::help());
        render_at(&mut app, width, height).lines().map(|row| cells(row).to_owned()).collect()
    }

    #[test]
    fn a_wide_screen_lists_every_key_in_two_columns() {
        let rows = help_rows(140, 50);
        assert!(rows.iter().any(|row| row.contains("MOVE") && row.contains("FIND")), "{rows:#?}");
    }

    #[test]
    fn every_key_and_the_markdown_rows_show_on_a_tall_screen() {
        let rows = help_rows(200, 60);
        for (keys, does) in super::HELP_GROUPS.iter().flat_map(|(_, bindings)| bindings.iter()).chain(&super::MARKDOWN) {
            let row = rows.iter().find(|row| row.contains(does)).unwrap_or_else(|| panic!("missing “{does}”: {rows:#?}"));
            assert!(row.contains(keys), "“{does}” lost its key: {row}");
        }
    }

    #[test]
    fn a_short_screen_says_how_to_scroll() {
        let rows = help_rows(80, 20);
        assert!(rows.iter().any(|row| row.contains("j k scroll")), "{rows:#?}");
    }

    #[test]
    fn listing_every_key_hides_the_hint_for_more() {
        let rows = help_rows(140, 60);
        assert!(!rows.iter().any(|row| row.contains("? every key")), "{rows:#?}");
    }

    #[test]
    fn a_terminal_smaller_than_the_modal_clips_it_instead_of_panicking() {
        let rows = help_rows(40, 12);
        assert_eq!(rows.len(), 12);
        assert!(rows.iter().any(|row| row.contains("keys")), "{rows:?}");
    }
}
