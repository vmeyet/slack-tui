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

/// The way back, for a pasted glyph. Names share a glyph often enough that the winner is picked
/// here: the shortest name, then the first alphabetically, so it never depends on map order.
/// Aliases stay out so they cannot stand in for the standard name.
static BY_GLYPH: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut by_glyph: HashMap<&str, &str> = HashMap::new();
    for (name, glyph) in TABLE.iter().map(|(name, glyph)| (*name, *glyph)) {
        let key = base(glyph);
        if key.is_empty() || is_alias(name) {
            continue;
        }
        let best = by_glyph.entry(key).or_insert(name);
        if (name.len(), name) < (best.len(), *best) {
            *best = name;
        }
    }
    by_glyph
});

const ALIASES: [(&str, &str); 2] = [("simple_smile", "slightly_smiling_face"), ("thumbsup_all", "+1")];

const MODIFIERS: [char; 7] = ['\u{fe0f}', '\u{fe0e}', '\u{1f3fb}', '\u{1f3fc}', '\u{1f3fd}', '\u{1f3fe}', '\u{1f3ff}'];

/// `tada` → 🎉, `+1::skin-tone-3` → 👍, unknown → None.
pub fn glyph(name: &str) -> Option<&'static str> {
    let base = name.split("::").next().unwrap_or(name);
    TABLE.get(base).copied()
}

/// 🚀 → `rocket`, 👍🏽 → `+1`, a custom or unknown glyph → None.
pub fn name_for(glyph: &str) -> Option<&'static str> {
    BY_GLYPH.get(base(glyph)).copied()
}

/// The glyph without the variation selector or skin tone trailing it, which Slack names never carry.
fn base(glyph: &str) -> &str {
    glyph.trim_end_matches(MODIFIERS)
}

fn is_alias(name: &str) -> bool {
    ALIASES.iter().any(|(alias, _)| *alias == name)
}

/// Every standard shortcode, for completion.
pub fn names() -> impl Iterator<Item = &'static str> {
    TABLE.keys().copied()
}

pub fn render(name: &str) -> String {
    glyph(name).map_or_else(|| format!(":{name}:"), str::to_owned)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
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
    fn pasted_glyphs_resolve_to_a_name() {
        assert_eq!(name_for("🚀"), Some("rocket"));
        assert_eq!(name_for("🎉"), Some("tada"));
        assert_eq!(name_for("🖊\u{fe0f}"), Some("lower_left_ballpoint_pen"));
        assert_eq!(name_for("🖊"), Some("lower_left_ballpoint_pen"), "with or without the variation selector");
        assert_eq!(name_for("partyparrot"), None);
        assert_eq!(name_for("🯅"), None, "an unknown glyph has no name to send");
    }

    #[test]
    fn skin_tones_resolve_to_the_base_name() {
        for tone in ['\u{1f3fb}', '\u{1f3fc}', '\u{1f3fd}', '\u{1f3fe}', '\u{1f3ff}'] {
            assert_eq!(name_for(&format!("👍{tone}")), Some("+1"), "skin tone {tone}");
        }
        assert_eq!(name_for("👍🏽\u{fe0f}"), Some("+1"));
    }

    #[test]
    fn a_glyph_shared_by_several_names_always_picks_the_same_one() {
        assert_eq!(name_for("👍"), Some("+1"), "the shortest of +1 and thumbsup");
        assert_eq!(name_for("🐬"), Some("dolphin"), "the first alphabetically of dolphin and flipper");
        assert_eq!(name_for("🙂"), Some("slightly_smiling_face"), "the alias simple_smile never wins");
    }

    #[test]
    fn every_name_the_completion_offers_comes_back_from_its_glyph() {
        for name in names() {
            let Some(drawn) = glyph(name) else { continue };
            if base(drawn).is_empty() {
                continue;
            }
            let back = name_for(drawn).expect("a glyph of the table has a name");
            assert_eq!(glyph(back).map(base), Some(base(drawn)), "{name} came back as {back}");
        }
    }

    #[test]
    fn custom_emoji_keep_their_name() {
        assert_eq!(glyph("partyparrot"), None);
        assert_eq!(render("partyparrot"), ":partyparrot:");
        assert_eq!(render("tada"), "🎉");
    }
}
