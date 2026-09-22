use crate::tui::app::App;
use crate::tui::images::Thumb;
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};

/// Where a picture lands on screen this frame.
pub(super) struct Placement {
    file: String,
    area: Rect,
}

/// Rows a message item reserved for a picture, counted from the item's first line.
pub(super) struct Slot {
    pub line: usize,
    pub file: String,
    pub size: Size,
}

/// Pictures go on last, after the panes and their fades: the terminal paints them as pixels
/// or placeholder cells, and both must stay exactly as the protocol wrote them.
pub(super) fn draw(f: &mut Frame, app: &mut App, pictures: &[Placement]) {
    for p in pictures {
        if let Some(Thumb::Ready(protocol)) = app.thumbs.get_mut(&p.file) {
            let widget = StatefulImage::<StatefulProtocol>::default().resize(Resize::Fit(Some(image::imageops::FilterType::Triangle)));
            f.render_stateful_widget(widget, p.area, protocol);
        }
    }
}

/// Screen areas of the reserved rows that are fully visible, walking items from the list's
/// scroll offset; a picture cut by the pane edge is skipped rather than squeezed.
pub(super) fn placements(inner: Rect, x: u16, offset: usize, rows: &[(usize, Vec<Slot>)]) -> Vec<Placement> {
    let mut out = Vec::new();
    let mut y = usize::from(inner.y);
    let bottom = usize::from(inner.bottom());
    for (height, slots) in rows.iter().skip(offset) {
        for slot in slots {
            let top = y + slot.line;
            if top + usize::from(slot.size.height) <= bottom {
                let width = slot.size.width.min(inner.right().saturating_sub(x));
                out.push(Placement { file: slot.file.clone(), area: Rect::new(x, top as u16, width, slot.size.height) });
            }
        }
        y += height;
        if y >= bottom {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use crate::api::{File, Message};
    use crate::resolve::NameBook;
    use crate::tui::app::{App, Focus, Incoming};
    use crate::tui::images::Thumbs;
    use crate::tui::ui::testing::{message, render_at};

    #[test]
    fn picture_rows_are_reserved_then_painted_under_the_text() {
        let mut app = App::new();
        app.thumbs = Thumbs::with(ratatui_image::picker::Picker::halfblocks());
        app.current_channel = Some("C1".into());
        app.focus = Focus::Messages;
        let shot = File {
            id: "F1".into(),
            name: "shot.png".into(),
            mimetype: "image/png".into(),
            thumb_360: "https://files.slack.com/s.png".into(),
            thumb_360_w: 400,
            thumb_360_h: 200,
            ..Default::default()
        };
        let with_shot = Message { files: vec![shot], ..message("1694700000.000100", "U1", "look") };
        app.apply(Incoming::History { channel: "C1".into(), messages: vec![with_shot], names: NameBook::default() });
        let out = render_at(&mut app, 100, 30);
        assert!(out.contains("⠋ loading image") && out.contains("📎 shot.png"), "{out}");
        assert!(!out.contains(" 📎 shot.png\n"), "no inline attachment line for a picture");
        let gradient = image::RgbImage::from_fn(400, 200, |_, y| image::Rgb([y as u8, y as u8, y as u8]));
        app.apply(Incoming::Thumb { id: "F1".into(), image: Some(image::DynamicImage::ImageRgb8(gradient)) });
        let out = render_at(&mut app, 100, 30);
        let lines: Vec<&str> = out.lines().collect();
        let text_row = lines.iter().position(|l| l.contains("look")).unwrap();
        let painted = |l: &str| l.contains('▀') || l.contains('▄');
        let first_pixels = lines.iter().position(|l| painted(l)).expect("halfblocks painted");
        assert_eq!(first_pixels, text_row + 1, "{out}");
        assert_eq!(lines.iter().filter(|l| painted(l)).count(), 10, "400x200 at 10x20 cells is 40x10");
        assert!(!out.contains("loading image"), "{out}");
    }
}
