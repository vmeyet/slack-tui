//! One palette for the whole TUI, so a terminal never mixes theme colors with hardcoded ones.
//! Themes only set foregrounds and row surfaces; the terminal keeps painting its own background,
//! so pick the theme that matches the terminal's.
use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    /// The terminal background this palette was designed for; text drawn on a colored fill uses it.
    pub base: Color,
    /// Inline code and code blocks: the ground nudged a few percent toward the text.
    pub surface: Color,
    /// Fill of the selected row. None by default: the `▎` bar alone marks it, which stays
    /// invisible-proof on terminals whose background the theme cannot see.
    pub highlight: Option<Color>,
    pub border: Color,
    /// Tertiary text: section headers, muted channels, receded panes.
    pub faded: Color,
    /// Secondary text: times, hints, counts.
    pub muted: Color,
    /// Titles, cursor bar, badges, your own reactions.
    pub accent: Color,
    pub link: Color,
    pub code: Color,
    pub mention: Color,
    pub success: Color,
    pub warn: Color,
    /// Author names, picked by a hash of the name.
    pub users: [Color; 6],
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `pct` percent of the way from `from` to `to`, per channel. Terminals have no alpha, so a
/// "translucent" surface is the background nudged a few percent toward the text.
const fn mix(from: u32, to: u32, pct: u32) -> Color {
    const fn channel(from: u32, to: u32, pct: u32, shift: u32) -> u8 {
        let (a, b) = ((from >> shift) & 0xff, (to >> shift) & 0xff);
        ((a * (100 - pct) + b * pct) / 100) as u8
    }
    Color::Rgb(channel(from, to, pct, 16), channel(from, to, pct, 8), channel(from, to, pct, 0))
}

const SURFACE_PCT: u32 = 3;

impl Default for Theme {
    fn default() -> Self {
        DEFAULT
    }
}

impl Theme {
    pub const NAMES: [&'static str; 9] =
        ["default", "dracula", "catppuccin", "catppuccin-latte", "rosepine", "rosepine-dawn", "nord", "tokyonight", "monokai"];

    /// Case, spaces and dashes do not matter: `Tokyo Night`, `tokyo-night` and `tokyonight` are one theme.
    pub fn named(name: &str) -> Option<Theme> {
        let key: String = name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect();
        match key.as_str() {
            "default" | "slack" => Some(DEFAULT),
            "dracula" => Some(DRACULA),
            "catppuccin" | "catppuccinmocha" | "mocha" => Some(CATPPUCCIN),
            "catppuccinlatte" | "latte" => Some(CATPPUCCIN_LATTE),
            "rosepine" => Some(ROSEPINE),
            "rosepinedawn" | "dawn" => Some(ROSEPINE_DAWN),
            "nord" => Some(NORD),
            "tokyonight" => Some(TOKYONIGHT),
            "monokai" => Some(MONOKAI),
            _ => None,
        }
    }

    pub fn user(&self, name: &str) -> Color {
        let idx = name.trim().bytes().fold(7usize, |h, b| h.wrapping_mul(33).wrapping_add(b as usize)) % self.users.len();
        self.users[idx]
    }
}

/// The terminal's own ANSI palette, so it follows whatever the terminal already looks like.
const DEFAULT: Theme = Theme {
    name: "default",
    base: Color::Black,
    surface: Color::Indexed(234),
    highlight: None,
    border: Color::Indexed(238),
    faded: Color::Indexed(240),
    muted: Color::Indexed(245),
    accent: Color::Cyan,
    link: Color::Blue,
    code: Color::Yellow,
    mention: Color::Magenta,
    success: Color::Green,
    warn: Color::Yellow,
    users: [Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Blue, Color::LightRed],
};

const DRACULA: Theme = Theme {
    name: "dracula",
    base: rgb(0x282a36),
    surface: mix(0x282a36, 0xf8f8f2, SURFACE_PCT),
    highlight: None,
    border: rgb(0x44475a),
    faded: rgb(0x6272a4),
    muted: rgb(0x9098bd),
    accent: rgb(0x8be9fd),
    link: rgb(0xbd93f9),
    code: rgb(0xf1fa8c),
    mention: rgb(0xff79c6),
    success: rgb(0x50fa7b),
    warn: rgb(0xffb86c),
    users: [rgb(0x8be9fd), rgb(0x50fa7b), rgb(0xf1fa8c), rgb(0xff79c6), rgb(0xbd93f9), rgb(0xffb86c)],
};

const CATPPUCCIN: Theme = Theme {
    name: "catppuccin",
    base: rgb(0x1e1e2e),
    surface: mix(0x1e1e2e, 0xcdd6f4, SURFACE_PCT),
    highlight: None,
    border: rgb(0x45475a),
    faded: rgb(0x6c7086),
    muted: rgb(0xa6adc8),
    accent: rgb(0x89dceb),
    link: rgb(0x89b4fa),
    code: rgb(0xf9e2af),
    mention: rgb(0xcba6f7),
    success: rgb(0xa6e3a1),
    warn: rgb(0xfab387),
    users: [rgb(0x89dceb), rgb(0xa6e3a1), rgb(0xf9e2af), rgb(0xf5c2e7), rgb(0x89b4fa), rgb(0xf38ba8)],
};

const CATPPUCCIN_LATTE: Theme = Theme {
    name: "catppuccin-latte",
    base: rgb(0xeff1f5),
    surface: mix(0xeff1f5, 0x4c4f69, SURFACE_PCT),
    highlight: None,
    border: rgb(0xbcc0cc),
    faded: rgb(0x9ca0b0),
    muted: rgb(0x6c6f85),
    accent: rgb(0x04a5e5),
    link: rgb(0x1e66f5),
    code: rgb(0xdf8e1d),
    mention: rgb(0x8839ef),
    success: rgb(0x40a02b),
    warn: rgb(0xfe640b),
    users: [rgb(0x04a5e5), rgb(0x40a02b), rgb(0xdf8e1d), rgb(0xea76cb), rgb(0x1e66f5), rgb(0xd20f39)],
};

const ROSEPINE: Theme = Theme {
    name: "rosepine",
    base: rgb(0x191724),
    surface: mix(0x191724, 0xe0def4, SURFACE_PCT),
    highlight: None,
    border: rgb(0x403d52),
    faded: rgb(0x6e6a86),
    muted: rgb(0x908caa),
    accent: rgb(0x9ccfd8),
    link: rgb(0xc4a7e7),
    code: rgb(0xf6c177),
    mention: rgb(0xebbcba),
    success: rgb(0x31748f),
    warn: rgb(0xf6c177),
    users: [rgb(0x9ccfd8), rgb(0x31748f), rgb(0xf6c177), rgb(0xeb6f92), rgb(0xc4a7e7), rgb(0xebbcba)],
};

const ROSEPINE_DAWN: Theme = Theme {
    name: "rosepine-dawn",
    base: rgb(0xfaf4ed),
    surface: mix(0xfaf4ed, 0x575279, SURFACE_PCT),
    highlight: None,
    border: rgb(0xdfdad9),
    faded: rgb(0x9893a5),
    muted: rgb(0x797593),
    accent: rgb(0x56949f),
    link: rgb(0x907aa9),
    code: rgb(0xea9d34),
    mention: rgb(0xd7827e),
    success: rgb(0x286983),
    warn: rgb(0xea9d34),
    users: [rgb(0x56949f), rgb(0x286983), rgb(0xea9d34), rgb(0xb4637a), rgb(0x907aa9), rgb(0xd7827e)],
};

const NORD: Theme = Theme {
    name: "nord",
    base: rgb(0x2e3440),
    surface: mix(0x2e3440, 0xd8dee9, SURFACE_PCT),
    highlight: None,
    border: rgb(0x434c5e),
    faded: rgb(0x4c566a),
    muted: rgb(0x7b88a1),
    accent: rgb(0x88c0d0),
    link: rgb(0x81a1c1),
    code: rgb(0xebcb8b),
    mention: rgb(0xb48ead),
    success: rgb(0xa3be8c),
    warn: rgb(0xd08770),
    users: [rgb(0x88c0d0), rgb(0xa3be8c), rgb(0xebcb8b), rgb(0xb48ead), rgb(0x81a1c1), rgb(0xbf616a)],
};

const TOKYONIGHT: Theme = Theme {
    name: "tokyonight",
    base: rgb(0x1a1b26),
    surface: mix(0x1a1b26, 0xc0caf5, SURFACE_PCT),
    highlight: None,
    border: rgb(0x3b4261),
    faded: rgb(0x565f89),
    muted: rgb(0x737aa2),
    accent: rgb(0x7dcfff),
    link: rgb(0x7aa2f7),
    code: rgb(0xe0af68),
    mention: rgb(0xbb9af7),
    success: rgb(0x9ece6a),
    warn: rgb(0xff9e64),
    users: [rgb(0x7dcfff), rgb(0x9ece6a), rgb(0xe0af68), rgb(0xbb9af7), rgb(0x7aa2f7), rgb(0xf7768e)],
};

const MONOKAI: Theme = Theme {
    name: "monokai",
    base: rgb(0x272822),
    surface: mix(0x272822, 0xf8f8f2, SURFACE_PCT),
    highlight: None,
    border: rgb(0x49483e),
    faded: rgb(0x75715e),
    muted: rgb(0xa59f85),
    accent: rgb(0x66d9ef),
    link: rgb(0xae81ff),
    code: rgb(0xe6db74),
    mention: rgb(0xf92672),
    success: rgb(0xa6e22e),
    warn: rgb(0xfd971f),
    users: [rgb(0x66d9ef), rgb(0xa6e22e), rgb(0xe6db74), rgb(0xf92672), rgb(0xae81ff), rgb(0xfd971f)],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_resolves_to_itself() {
        for name in Theme::NAMES {
            assert_eq!(Theme::named(name).map(|t| t.name), Some(name), "{name}");
        }
    }

    #[test]
    fn names_are_forgiving_and_unknown_is_none() {
        assert_eq!(Theme::named("Tokyo Night").unwrap().name, "tokyonight");
        assert_eq!(Theme::named("catppuccin_mocha").unwrap().name, "catppuccin");
        assert_eq!(Theme::named("Rose-Pine Dawn").unwrap().name, "rosepine-dawn");
        assert_eq!(Theme::named("solarized"), None);
        assert_eq!(Theme::named(""), None);
    }

    #[test]
    fn mix_walks_each_channel_toward_the_target() {
        assert_eq!(mix(0x000000, 0xffffff, 50), Color::Rgb(127, 127, 127));
        assert_eq!(mix(0x1e1e2e, 0xcdd6f4, 0), rgb(0x1e1e2e));
        assert_eq!(mix(0x1e1e2e, 0xcdd6f4, 100), rgb(0xcdd6f4));
        assert_eq!(mix(0x102030, 0x304050, 10), Color::Rgb(0x13, 0x23, 0x33));
    }

    #[test]
    fn user_colors_are_stable_and_spread() {
        let theme = Theme::default();
        assert_eq!(theme.user("vivien"), theme.user(" vivien "));
        let distinct: std::collections::HashSet<_> =
            ["a", "b", "c", "d", "e", "f", "g", "h"].iter().map(|n| format!("{:?}", theme.user(n))).collect();
        assert!(distinct.len() > 1);
    }

    /// The whole point of a theme: no pane paints a color the theme did not choose.
    #[test]
    fn no_raw_colors_outside_the_theme() {
        let sources = [
            ("ui.rs", include_str!("ui.rs")),
            ("inbox.rs", include_str!("inbox.rs")),
            ("jump.rs", include_str!("jump.rs")),
            ("firehose.rs", include_str!("firehose.rs")),
            ("app.rs", include_str!("app.rs")),
            ("palette.rs", include_str!("palette.rs")),
            ("mod.rs", include_str!("mod.rs")),
        ];
        let raw = ["Color::", ".cyan()", ".yellow()", ".green()", ".magenta()", ".blue()", ".red()", ".black()", ".on_yellow()", ".dim()"];
        let leaks: Vec<String> = sources
            .iter()
            .flat_map(|(file, src)| src.lines().enumerate().map(move |(i, l)| (file, i + 1, l)))
            .filter(|(_, _, line)| !line.trim_start().starts_with("//") && raw.iter().any(|r| line.contains(r)))
            .map(|(file, n, line)| format!("{file}:{n}: {}", line.trim()))
            .collect();
        assert!(leaks.is_empty(), "raw colors:\n{}", leaks.join("\n"));
    }
}
