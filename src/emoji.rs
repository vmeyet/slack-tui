//! Slack shortcodes to Unicode, from Slack's own `emoji-data` names.
//! Custom workspace emoji have no glyph and stay as `:name:`.
use std::collections::HashMap;
use std::sync::LazyLock;

static TABLE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut table: HashMap<&str, &str> = include_str!("emoji_data.tsv").lines().filter_map(|l| l.split_once('\t')).collect();
    for (slack_only, standard) in ALIASES {
        if let Some(glyph) = table.get(standard).copied() {
            table.insert(slack_only, glyph);
        }
    }
    table
});

const ALIASES: [(&str, &str); 2] = [("simple_smile", "slightly_smiling_face"), ("thumbsup_all", "+1")];

/// `tada` → 🎉, `+1::skin-tone-3` → 👍, unknown → None.
pub fn glyph(name: &str) -> Option<&'static str> {
    let base = name.split("::").next().unwrap_or(name);
    TABLE.get(base).copied()
}

/// Every standard shortcode, for completion.
pub fn names() -> impl Iterator<Item = &'static str> {
    TABLE.keys().copied()
}

pub fn render(name: &str) -> String {
    glyph(name).map(str::to_owned).unwrap_or_else(|| format!(":{name}:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_names_resolve() {
        assert_eq!(glyph("tada"), Some("🎉"));
        assert_eq!(glyph("+1"), Some("👍"));
        assert_eq!(glyph("rocket"), Some("🚀"));
        assert_eq!(glyph("white_check_mark"), Some("✅"));
        assert_eq!(glyph("lower_left_ballpoint_pen"), Some("🖊\u{fe0f}"));
        assert_eq!(glyph("simple_smile"), Some("🙂"));
        assert_eq!(glyph("thumbsup"), Some("👍"));
        assert!(TABLE.len() > 1900);
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
