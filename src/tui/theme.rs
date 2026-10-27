//! The theme the config asks for, on top of vtui's palettes.
use anyhow::{Result, anyhow};
use vtui::theme::Theme;

/// `tui.theme` picks the palette, `tui.highlight` adds a fill under the selected row.
pub fn from_config(config: &crate::config::Tui) -> Result<Theme> {
    let mut theme = match config.theme.as_deref() {
        Some(name) => {
            Theme::named(name).ok_or_else(|| anyhow!("config `tui.theme = \"{name}\"` is not a theme (try {})", Theme::NAMES.join(", ")))?
        }
        None => Theme::default(),
    };
    if let Some(color) = config.highlight.as_deref().filter(|c| *c != "none") {
        let parsed = color
            .parse()
            .map_err(|_| anyhow!("config `tui.highlight = \"{color}\"` is not a colour (try `darkgray`, `#2a2a2a`, `236` or `none`)"))?;
        theme.highlight = Some(parsed);
    }
    Ok(theme)
}

#[cfg(test)]
mod tests {
    /// The whole point of a theme: no pane paints a color the theme did not choose.
    #[test]
    fn no_raw_colors_outside_the_theme() {
        let sources = [
            ("ui/mod.rs", include_str!("ui/mod.rs")),
            ("ui/channels.rs", include_str!("ui/channels.rs")),
            ("ui/confirm.rs", include_str!("ui/confirm.rs")),
            ("ui/empty.rs", include_str!("ui/empty.rs")),
            ("ui/help.rs", include_str!("ui/help.rs")),
            ("ui/input.rs", include_str!("ui/input.rs")),
            ("ui/items.rs", include_str!("ui/items.rs")),
            ("ui/messages.rs", include_str!("ui/messages.rs")),
            ("ui/pictures.rs", include_str!("ui/pictures.rs")),
            ("ui/status.rs", include_str!("ui/status.rs")),
            ("ui/style.rs", include_str!("ui/style.rs")),
            ("ui/thread.rs", include_str!("ui/thread.rs")),
            ("inbox.rs", include_str!("inbox.rs")),
            ("jump.rs", include_str!("jump.rs")),
            ("firehose.rs", include_str!("firehose.rs")),
            ("app/mod.rs", include_str!("app/mod.rs")),
            ("app/state.rs", include_str!("app/state.rs")),
            ("app/incoming.rs", include_str!("app/incoming.rs")),
            ("app/live.rs", include_str!("app/live.rs")),
            ("app/keys.rs", include_str!("app/keys.rs")),
            ("app/commands.rs", include_str!("app/commands.rs")),
            ("app/sidebar.rs", include_str!("app/sidebar.rs")),
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
