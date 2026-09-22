//! Inline image thumbnails: fetched and decoded off the draw path, sized in cells from the
//! terminal's font, and drawn over rows the message list reserves for them.
use crate::api::File;
use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use std::collections::HashMap;
use std::fmt;

/// Widest a thumbnail gets, in cells; height follows the image's aspect.
pub const MAX_COLS: u16 = 60;
pub const MAX_ROWS: u16 = 14;
const MAX_PIXELS: u32 = 4096;

pub enum Thumb {
    Loading,
    Ready(Box<StatefulProtocol>),
    Failed,
}

/// Everything the TUI knows about pictures: whether the terminal can draw them, and each file's state.
pub struct Thumbs {
    picker: Option<Picker>,
    slots: HashMap<String, Thumb>,
}

impl fmt::Debug for Thumbs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Thumbs({} enabled, {} files)", self.enabled(), self.slots.len())
    }
}

impl Default for Thumbs {
    fn default() -> Self {
        Self::off()
    }
}

impl Thumbs {
    pub fn off() -> Self {
        Self { picker: None, slots: HashMap::new() }
    }

    pub fn with(picker: Picker) -> Self {
        Self { picker: Some(picker), slots: HashMap::new() }
    }

    /// Asks the terminal what it can draw. Pixel protocols only: half-block mosaics look worse
    /// than the plain 📎 line, so those terminals get text.
    pub fn from_terminal() -> Self {
        match Picker::from_query_stdio() {
            Ok(picker) if picker.protocol_type() != ProtocolType::Halfblocks => Self::with(picker),
            _ => Self::off(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.picker.is_some()
    }

    /// Cell size a file's thumbnail will take within `max_cols`, never upscaled past its pixels.
    pub fn cells(&self, file: &File, max_cols: u16) -> Option<Size> {
        let picker = self.picker.as_ref()?;
        let thumb = file.thumb()?;
        let font = picker.font_size();
        let (fw, fh) = (f64::from(font.width.max(1)), f64::from(font.height.max(1)));
        let (w, h) = (f64::from(thumb.width), f64::from(thumb.height));
        let max_cols = f64::from(max_cols.clamp(1, MAX_COLS));
        let scale = (max_cols * fw / w).min(f64::from(MAX_ROWS) * fh / h).min(1.0);
        let cols = (w * scale / fw).ceil().max(1.0) as u16;
        let rows = (h * scale / fh).ceil().max(1.0) as u16;
        Some(Size::new(cols, rows))
    }

    /// Image files not asked for yet, marked as loading so each is fetched once.
    pub fn wanted<'a>(&mut self, files: impl IntoIterator<Item = &'a File>) -> Vec<(String, String)> {
        if !self.enabled() {
            return vec![];
        }
        let mut out = Vec::new();
        for file in files {
            let Some(thumb) = file.thumb() else { continue };
            if self.slots.contains_key(&file.id) {
                continue;
            }
            self.slots.insert(file.id.clone(), Thumb::Loading);
            out.push((file.id.clone(), thumb.url.to_owned()));
        }
        out
    }

    pub fn arrived(&mut self, id: &str, image: Option<DynamicImage>) {
        let thumb = match (image, &self.picker) {
            (Some(image), Some(picker)) => Thumb::Ready(Box::new(picker.new_resize_protocol(image))),
            _ => Thumb::Failed,
        };
        self.slots.insert(id.to_owned(), thumb);
    }

    pub fn loading(&self) -> bool {
        self.slots.values().any(|t| matches!(t, Thumb::Loading))
    }

    pub fn get(&self, id: &str) -> Option<&Thumb> {
        self.slots.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Thumb> {
        self.slots.get_mut(id)
    }

    /// Forgets files no longer on screen so memory follows the conversation, not the session.
    pub fn keep_only(&mut self, ids: impl IntoIterator<Item = String>) {
        let keep: std::collections::HashSet<String> = ids.into_iter().collect();
        self.slots.retain(|id, _| keep.contains(id));
    }
}

/// Decodes with hard limits so a hostile upload cannot balloon into gigabytes of pixels.
pub fn decode(bytes: &[u8]) -> Option<DynamicImage> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PIXELS);
    limits.max_image_height = Some(MAX_PIXELS);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    reader.decode().ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::new_rgb8(w, h).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn image_file(id: &str, w: u32, h: u32) -> File {
        File {
            id: id.into(),
            mimetype: "image/png".into(),
            thumb_720: "https://files.slack.com/t.png".into(),
            thumb_720_w: w,
            thumb_720_h: h,
            ..Default::default()
        }
    }

    /// Halfblocks picker with a 10x20 font: draws into a plain buffer, so tests can see it.
    pub fn test_thumbs() -> Thumbs {
        Thumbs::with(Picker::halfblocks())
    }

    #[test]
    fn cells_follow_aspect_and_never_upscale() {
        let thumbs = test_thumbs();
        assert_eq!(thumbs.cells(&image_file("F1", 720, 480), 80), Some(Size::new(42, 14)));
        assert_eq!(thumbs.cells(&image_file("F2", 720, 120), 80), Some(Size::new(60, 5)));
        assert_eq!(thumbs.cells(&image_file("F3", 720, 480), 20), Some(Size::new(20, 7)));
        assert_eq!(thumbs.cells(&image_file("F4", 16, 16), 80), Some(Size::new(2, 1)));
        assert_eq!(thumbs.cells(&File { mimetype: "application/pdf".into(), ..Default::default() }, 80), None);
        assert_eq!(Thumbs::off().cells(&image_file("F1", 720, 480), 80), None);
    }

    #[test]
    fn each_image_is_requested_once_and_settles_ready_or_failed() {
        let mut thumbs = test_thumbs();
        let files = [image_file("F1", 8, 8), image_file("F2", 8, 8), File { id: "F3".into(), ..Default::default() }];
        assert_eq!(
            thumbs.wanted(&files),
            vec![("F1".to_string(), "https://files.slack.com/t.png".to_string()), ("F2".into(), "https://files.slack.com/t.png".into())]
        );
        assert!(thumbs.wanted(&files).is_empty());
        thumbs.arrived("F1", decode(&png(8, 8)));
        assert!(thumbs.loading(), "F2 is still on its way");
        thumbs.arrived("F2", None);
        assert!(!thumbs.loading());
        assert!(matches!(thumbs.get("F1"), Some(Thumb::Ready(_))));
        assert!(matches!(thumbs.get("F2"), Some(Thumb::Failed)));
        thumbs.keep_only(["F2".to_string()]);
        assert!(thumbs.get("F1").is_none() && thumbs.get("F2").is_some());
        assert!(Thumbs::off().wanted(&files).is_empty());
    }

    #[test]
    fn decode_rejects_garbage_and_oversized_images() {
        assert!(decode(b"not an image").is_none());
        assert!(decode(&png(4, 4)).is_some());
        assert!(decode(&png(MAX_PIXELS + 1, 1)).is_none());
    }
}
