use super::{MODAL_FRAME_H, MODAL_FRAME_W, centered_cells, modal_pane};
use crate::render;
use crate::render::text;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

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
            ("e", "write the message in $EDITOR"),
            ("+", "react (type the emoji name)"),
            ("o / y", "open in Slack · copy permalink"),
            ("u", "open the message's link in the browser"),
            (":edit", "rewrite your own message"),
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
            ("f", "firehose: every channel as one live ticker"),
        ],
    ),
    (
        "modes",
        &[
            ("z", "reading mode: one centered conversation, nothing else"),
            (":", "command line: :join :go :msg :react :search :export :read …"),
            ("R", "refresh"),
            ("esc", "close thread · clear search or filter"),
            ("q", "quit"),
        ],
    ),
];

const HELP_FOOTER: &str = "messages are markdown: **bold** _italic_ `code` @user #channel";
/// Blank columns between the key column and what the key does.
const HELP_GUTTER: usize = 2;
/// Bindings sit under their group name, not next to it.
const HELP_INDENT: usize = 2;

pub(super) fn draw(f: &mut Frame, theme: &Theme, area: Rect) {
    let lines = help_lines(theme);
    let popup = centered_cells(area, help_width() + MODAL_FRAME_W, lines.len() as u16 + MODAL_FRAME_H);
    let block = modal_pane(theme, "keys")
        .title_bottom(Line::from(Span::styled(" esc or ? to close ", Style::new().fg(theme.faded))).right_aligned());
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(block), popup);
}

fn help_bindings() -> impl Iterator<Item = (&'static str, &'static str)> {
    HELP_GROUPS.iter().flat_map(|(_, bindings)| bindings.iter().copied())
}

fn help_key_width() -> usize {
    help_bindings().map(|(key, _)| text::visible_width(key)).max().unwrap_or(0)
}

/// Columns the widest row needs: a binding, a group name or the footer.
fn help_width() -> u16 {
    let key_w = help_key_width();
    let bindings = help_bindings().map(|(_, what)| HELP_INDENT + key_w + HELP_GUTTER + text::visible_width(what));
    let names = HELP_GROUPS.iter().map(|(name, _)| text::visible_width(name));
    bindings.chain(names).chain([text::visible_width(HELP_FOOTER)]).max().unwrap_or(0) as u16
}

/// Keys in one column and their meaning in the next, a quiet header per group, the markdown
/// note set apart at the end.
fn help_lines(theme: &Theme) -> Vec<Line<'static>> {
    let key_w = help_key_width();
    let mut lines = Vec::new();
    for (name, bindings) in HELP_GROUPS {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::from(Span::styled(name, Style::new().fg(theme.faded).add_modifier(Modifier::BOLD))));
        for (key, what) in bindings {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(HELP_INDENT)),
                Span::styled(render::fit(key, key_w), Style::new().fg(theme.accent).bold()),
                Span::raw(" ".repeat(HELP_GUTTER)),
                Span::styled(*what, Style::new().fg(theme.muted)),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(HELP_FOOTER, Style::new().fg(theme.faded))));
    lines
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::tui::app::App;
    use crate::tui::ui::testing::{cells, render_at};

    #[test]
    fn help_overlay_and_empty_state_render() {
        let mut app = App::new();
        app.help = true;
        let out = render_at(&mut app, 80, 20);
        assert!(out.contains("keys"));
        assert!(out.contains("reply in the focused conversation"));
    }

    fn help_rows(width: u16, height: u16) -> Vec<String> {
        let mut app = App::new();
        app.help = true;
        render_at(&mut app, width, height).lines().map(|row| cells(row).to_owned()).collect()
    }

    #[test]
    fn every_binding_sits_whole_on_one_line_with_its_key() {
        for (width, height) in [(80, 40), (140, 50)] {
            let rows = help_rows(width, height);
            for (key, what) in help_bindings() {
                let found: Vec<&String> = rows.iter().filter(|row| row.contains(what)).collect();
                assert_eq!(found.len(), 1, "{width}x{height}: “{what}” on {} lines", found.len());
                assert!(found[0].contains(key), "{width}x{height}: “{what}” lost its key: {}", found[0]);
            }
            for note in [HELP_FOOTER, "esc or ? to close"] {
                assert!(rows.iter().any(|row| row.contains(note)), "{width}x{height}: missing “{note}”");
            }
        }
    }

    #[test]
    fn descriptions_all_start_at_the_same_column() {
        let rows = help_rows(140, 50);
        let column = |what: &str| {
            let row = rows.iter().find(|row| row.contains(what)).unwrap();
            text::visible_width(&row[..row.find(what).unwrap()])
        };
        let columns: std::collections::HashSet<usize> = help_bindings().map(|(_, what)| column(what)).collect();
        assert_eq!(columns.len(), 1, "descriptions start at {columns:?}");
    }

    #[test]
    fn a_terminal_smaller_than_the_modal_clips_it_instead_of_panicking() {
        let rows = help_rows(40, 12);
        assert_eq!(rows.len(), 12);
        assert!(rows.iter().any(|row| row.contains("keys")), "{rows:?}");
        assert!(rows.iter().all(|row| text::visible_width(row) == 40), "{rows:?}");
    }
}
