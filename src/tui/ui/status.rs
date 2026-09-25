use crate::render::text;
use crate::tui::app::{App, Focus, Live};
use crate::tui::palette::Palette;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// The connection dot and the status on the left, the key hints pushed to the right edge.
pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let (dot, dot_style) = live_dot(&app.live, &app.theme);
    let status = format!("{} ", app.status_line());
    let room = area.width as usize;
    let used = 1 + dot.len() + text::visible_width(&status);
    let right = text::truncate(&hints(app), room.saturating_sub(used + 1));
    let pad = room.saturating_sub(used + text::visible_width(&right));
    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(dot, dot_style),
        Span::styled(status, Style::new().bold()),
        Span::raw(" ".repeat(pad)),
        Span::styled(right, Style::new().fg(app.theme.muted)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn live_dot(live: &Live, theme: &Theme) -> (&'static str, Style) {
    match live {
        Live::Connected => ("● ", Style::new().fg(theme.success)),
        Live::Connecting => ("○ ", Style::new().fg(theme.muted)),
        Live::Polling(_) => ("↻ ", Style::new().fg(theme.warn)),
    }
}

/// The completion being cycled, or the keys that work right now, after the update hint when there is one.
fn hints(app: &App) -> String {
    let cycling = app.palette.as_ref().and_then(Palette::hint);
    let hints = cycling.as_deref().unwrap_or(key_hints(app));
    match app.update_hint() {
        Some(update) => format!("{update} · {hints}"),
        None => hints.to_owned(),
    }
}

fn key_hints(app: &App) -> &'static str {
    match (&app.input, app.focus) {
        _ if app.react.as_ref().is_some_and(|p| p.search.is_some()) => "type a name · ←/→ move · enter react · esc cancel",
        _ if app.react.is_some() => "1-8 react · h/l move · enter react · / search · esc cancel",
        _ if app.palette.is_some() => "tab cycle · → accept · ↑ history · enter run · esc cancel",
        (Some(_), _) => "enter send · esc cancel",
        (None, Focus::Channels) => "j/k move · enter open · / filter · ^k jump · ? more",
        (None, Focus::Messages) => "j/k move · enter thread · r reply · + react · ? more",
        (None, Focus::Thread) => "j/k move · r reply · + react · esc close · ? more",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::Incoming;
    use crate::tui::ui::testing::{cells, render};

    fn status_bar(app: &mut App) -> String {
        render(app).lines().last().unwrap_or_default().to_owned()
    }

    #[test]
    fn toast_shows_in_the_status_bar_until_it_ends() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        let bar = status_bar(&mut app);
        assert!(bar.contains("C1"), "{bar}");
        app.apply(Incoming::Toast("permalink copied".into()));
        let bar = status_bar(&mut app);
        assert!(bar.contains("permalink copied") && !bar.contains("C1"), "{bar}");
        app.now += std::time::Duration::from_secs(2);
        let bar = status_bar(&mut app);
        assert!(bar.contains("C1") && !bar.contains("permalink copied"), "{bar}");
    }

    #[test]
    fn a_newer_commit_shows_the_update_hint_next_to_the_key_hints() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        assert!(!status_bar(&mut app).contains("update available"));
        app.apply(Incoming::Latest(Some("0000000000000000000000000000000000000000".into())));
        let bar = status_bar(&mut app);
        assert!(bar.contains("update available · slack update"), "{bar}");
        assert!(bar.contains("C1"), "{bar}");
    }

    #[test]
    fn a_failed_update_check_shows_nothing_and_wakes_nothing() {
        let mut app = App::new();
        let redraw_in = app.redraw_in();
        assert!(app.apply(Incoming::Latest(None)).is_empty());
        assert!(!status_bar(&mut app).contains("update available"));
        assert_eq!(app.redraw_in(), redraw_in);
    }

    #[test]
    fn a_toast_still_owns_the_left_side_while_the_hint_shows() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.apply(Incoming::Latest(Some("0000000000000000000000000000000000000000".into())));
        app.apply(Incoming::Toast("permalink copied".into()));
        let bar = status_bar(&mut app);
        assert!(bar.contains("permalink copied") && !bar.contains("C1"), "{bar}");
        assert!(bar.contains("update available"), "{bar}");
    }

    #[test]
    fn key_hints_stay_short_and_follow_the_focus() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        let hint_of = |app: &mut App, focus| {
            app.focus = focus;
            let bar = status_bar(app);
            cells(&bar).split_once("C1 ").map(|(_, hints)| hints.trim().to_owned()).unwrap_or_default()
        };
        for focus in [Focus::Channels, Focus::Messages, Focus::Thread] {
            let hints = hint_of(&mut app, focus);
            assert!(hints.starts_with("j/k move"), "{focus:?}: {hints}");
            assert!(hints.ends_with("? more"), "{focus:?}: {hints}");
            assert_eq!(hints.split(" · ").count(), 5, "{focus:?}: {hints}");
        }
        assert!(hint_of(&mut app, Focus::Messages).contains("enter thread"));
        assert!(hint_of(&mut app, Focus::Thread).contains("esc close"));
        assert!(hint_of(&mut app, Focus::Channels).contains("/ filter"));
    }

    #[test]
    fn the_update_hint_leaves_room_for_the_keys() {
        let mut app = App::new();
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        app.apply(Incoming::Latest(Some("0000000000000000000000000000000000000000".into())));
        let bar = status_bar(&mut app);
        assert!(bar.contains("update available · slack update · j/k move · enter thread"), "{bar}");
    }
}
