use owo_colors::{AnsiColors, OwoColorize};
use std::io::IsTerminal;

/// How command-line output is styled: colour, width and link rendering.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Whether ANSI colour is on.
    pub color: bool,
    /// Columns available for wrapping.
    pub width: usize,
    /// Print `label (url)` instead of a clickable label.
    pub show_urls: bool,
}

const USER_COLORS: [AnsiColors; 6] =
    [AnsiColors::Cyan, AnsiColors::Green, AnsiColors::Yellow, AnsiColors::Magenta, AnsiColors::Blue, AnsiColors::BrightRed];

impl Theme {
    /// Reads the terminal: colour unless piped or `NO_COLOR`, width from `COLUMNS` or the terminal.
    pub fn detect() -> Self {
        let tty = std::io::stdout().is_terminal();
        let color = tty && std::env::var_os("NO_COLOR").is_none();
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .or_else(|| crossterm::terminal::size().ok().map(|(w, _)| w as usize))
            .filter(|w| *w > 0)
            .unwrap_or(100);
        Self { color, width, show_urls: false }
    }

    /// No colour and a fixed width, for tests and piped output.
    pub fn plain(width: usize) -> Self {
        Self { color: false, width, show_urls: false }
    }

    /// Sets whether links print their URL next to the label.
    pub fn with_show_urls(mut self, show_urls: bool) -> Self {
        self.show_urls = show_urls;
        self
    }

    /// A label that opens `url` on terminals speaking OSC 8, plain underlined text elsewhere.
    pub fn hyperlink(&self, label: &str, url: &str) -> String {
        let text = self.paint(label, |s| s.blue().underline().to_string());
        if self.color && !url.is_empty() { format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\") } else { text }
    }

    fn paint(&self, s: &str, f: impl FnOnce(&str) -> String) -> String {
        if self.color { f(s) } else { s.to_owned() }
    }

    /// Secondary text.
    pub fn dim(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    /// Emphasised text.
    pub fn bold(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().to_string())
    }
    /// Italic text.
    pub fn italic(&self, s: &str) -> String {
        self.paint(s, |s| s.italic().to_string())
    }
    /// Struck-through text.
    pub fn strike(&self, s: &str) -> String {
        self.paint(s, |s| s.strikethrough().to_string())
    }
    /// A heading.
    pub fn title(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().white().to_string())
    }
    /// Text that should catch the eye without shouting.
    pub fn accent(&self, s: &str) -> String {
        self.paint(s, |s| s.cyan().to_string())
    }
    /// A timestamp.
    pub fn time(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    /// Inline code.
    pub fn code(&self, s: &str) -> String {
        self.paint(s, |s| s.yellow().to_string())
    }
    /// One line of a quote or code block, with its left bar.
    pub fn block(&self, s: &str) -> String {
        if s.is_empty() { self.dim("▎") } else { format!("{}{s}", self.dim("▎ ")) }
    }
    /// A bare URL.
    pub fn link(&self, s: &str) -> String {
        self.paint(s, |s| s.blue().underline().to_string())
    }
    /// A link with its own label: clickable, or `label (url)` when URLs are shown.
    pub fn link_labelled(&self, label: &str, url: &str) -> String {
        if label == url || url.is_empty() {
            return self.link(label);
        }
        if self.show_urls {
            return format!("{} {}", self.paint(label, |s| s.underline().to_string()), self.dim(&format!("({url})")));
        }
        self.hyperlink(label, url)
    }
    /// An `@user` or `#channel` mention inside a message.
    pub fn mention(&self, s: &str) -> String {
        self.paint(s, |s| s.magenta().to_string())
    }
    /// A success mark.
    pub fn ok(&self, s: &str) -> String {
        self.paint(s, |s| s.green().bold().to_string())
    }
    /// A failure mark.
    pub fn err(&self, s: &str) -> String {
        self.paint(s, |s| s.red().bold().to_string())
    }
    /// A channel name, in a colour that stays the same for that channel.
    pub fn channel(&self, name: &str) -> String {
        let idx = name.trim().bytes().fold(7usize, |h, b| h.wrapping_mul(33).wrapping_add(b as usize)) % USER_COLORS.len();
        self.paint(name, |s| s.color(USER_COLORS[idx]).to_string())
    }
    /// Text matching a highlight pattern.
    pub fn highlight(&self, s: &str) -> String {
        self.paint(s, |s| s.black().on_yellow().bold().to_string())
    }
    /// A person's name, in a colour that stays the same for that person.
    pub fn user(&self, name: &str) -> String {
        let idx = name.trim().bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) % USER_COLORS.len();
        self.paint(name, |s| s.color(USER_COLORS[idx]).bold().to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn labelled_links_follow_the_setting() {
        let plain = Theme::plain(80);
        assert_eq!(plain.link_labelled("docs", "https://a.io"), "docs");
        assert_eq!(plain.with_show_urls(true).link_labelled("docs", "https://a.io"), "docs (https://a.io)");
        assert_eq!(Theme::plain(80).link_labelled("https://a.io", "https://a.io"), "https://a.io");
        let color = Theme { color: true, width: 80, show_urls: false };
        let out = color.link_labelled("docs", "https://a.io");
        assert!(out.starts_with("\x1b]8;;https://a.io\x1b\\"), "{out:?}");
        assert!(out.ends_with("\x1b]8;;\x1b\\"));
    }
}
