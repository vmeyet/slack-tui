use owo_colors::{AnsiColors, OwoColorize};
use std::io::IsTerminal;

#[derive(Clone, Debug)]
pub struct Theme {
    pub color: bool,
    pub width: usize,
}

const USER_COLORS: [AnsiColors; 6] =
    [AnsiColors::Cyan, AnsiColors::Green, AnsiColors::Yellow, AnsiColors::Magenta, AnsiColors::Blue, AnsiColors::BrightRed];

impl Theme {
    pub fn detect() -> Self {
        let tty = std::io::stdout().is_terminal();
        let color = tty && std::env::var_os("NO_COLOR").is_none();
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|c| c.parse().ok())
            .or_else(|| crossterm::terminal::size().ok().map(|(w, _)| w as usize))
            .filter(|w| *w > 0)
            .unwrap_or(100);
        Self { color, width }
    }

    pub fn plain(width: usize) -> Self {
        Self { color: false, width }
    }

    fn paint(&self, s: &str, f: impl FnOnce(&str) -> String) -> String {
        if self.color { f(s) } else { s.to_owned() }
    }

    pub fn dim(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    pub fn bold(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().to_string())
    }
    pub fn italic(&self, s: &str) -> String {
        self.paint(s, |s| s.italic().to_string())
    }
    pub fn strike(&self, s: &str) -> String {
        self.paint(s, |s| s.strikethrough().to_string())
    }
    pub fn title(&self, s: &str) -> String {
        self.paint(s, |s| s.bold().white().to_string())
    }
    pub fn accent(&self, s: &str) -> String {
        self.paint(s, |s| s.cyan().to_string())
    }
    pub fn time(&self, s: &str) -> String {
        self.paint(s, |s| s.dimmed().to_string())
    }
    pub fn code(&self, s: &str) -> String {
        self.paint(s, |s| s.yellow().to_string())
    }
    pub fn link(&self, s: &str) -> String {
        self.paint(s, |s| s.blue().underline().to_string())
    }
    pub fn link_labelled(&self, label: &str, url: &str) -> String {
        if label == url || url.is_empty() {
            return self.link(label);
        }
        format!("{} {}", self.paint(label, |s| s.underline().to_string()), self.dim(&format!("({url})")))
    }
    pub fn mention(&self, s: &str) -> String {
        self.paint(s, |s| s.magenta().to_string())
    }
    pub fn ok(&self, s: &str) -> String {
        self.paint(s, |s| s.green().bold().to_string())
    }
    pub fn err(&self, s: &str) -> String {
        self.paint(s, |s| s.red().bold().to_string())
    }
    pub fn user(&self, name: &str) -> String {
        let idx = name.trim().bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize)) % USER_COLORS.len();
        self.paint(name, |s| s.color(USER_COLORS[idx]).bold().to_string())
    }
}
