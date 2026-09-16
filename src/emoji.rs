//! Slack shortcodes to Unicode. Custom workspace emoji have no glyph and stay as `:name:`.

/// `tada` → 🎉, `+1::skin-tone-3` → 👍, unknown → None.
pub fn glyph(name: &str) -> Option<&'static str> {
    let base = name.split("::").next().unwrap_or(name);
    emojis::get_by_shortcode(base)
        .or_else(|| ALIASES.iter().find(|(slack, _)| *slack == base).and_then(|(_, gh)| emojis::get_by_shortcode(gh)))
        .map(|e| e.as_str())
}

pub fn render(name: &str) -> String {
    glyph(name).map(str::to_owned).unwrap_or_else(|| format!(":{name}:"))
}

const ALIASES: [(&str, &str); 8] = [
    ("simple_smile", "slightly_smiling_face"),
    ("slightly_smiling_face", "slightly_smiling_face"),
    ("thumbsup_all", "thumbsup"),
    ("white_check_mark", "white_check_mark"),
    ("heavy_check_mark", "heavy_check_mark"),
    ("the_horns", "metal"),
    ("large_blue_circle", "large_blue_circle"),
    ("knife_fork_plate", "fork_knife_plate"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_slack_names_resolve() {
        assert_eq!(glyph("tada"), Some("🎉"));
        assert_eq!(glyph("+1"), Some("👍"));
        assert_eq!(glyph("rocket"), Some("🚀"));
        assert_eq!(glyph("white_check_mark"), Some("✅"));
        assert_eq!(glyph("eyes"), Some("👀"));
        assert_eq!(glyph("simple_smile"), Some("🙂"));
    }

    #[test]
    fn skin_tones_fall_back_to_base() {
        assert_eq!(glyph("+1::skin-tone-3"), Some("👍"));
    }

    #[test]
    fn custom_emoji_keep_their_name() {
        assert_eq!(glyph("partyparrot"), None);
        assert_eq!(render("partyparrot"), ":partyparrot:");
        assert_eq!(render("tada"), "🎉");
    }
}
