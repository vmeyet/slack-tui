//! What moves on screen is drawn from the time elapsed since start, so nothing has to count frames.
use std::time::Duration;

/// One step of the empty-state animation.
pub const FRAME: Duration = Duration::from_millis(300);
/// How often the loop wakes while something loads: one step of vtui's lively spinner.
pub const SPINNER_FRAME: Duration = Duration::from_millis(80);

pub fn frame(elapsed: Duration) -> u32 {
    (elapsed.as_millis() / FRAME.as_millis()) as u32
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use vtui::motion::Motion;

    #[test]
    fn the_spinner_steps_once_per_wake() {
        assert_ne!(Motion::Lively.spinner(Duration::ZERO), Motion::Lively.spinner(SPINNER_FRAME));
    }

    #[test]
    fn frame_steps_every_300ms() {
        assert_eq!(frame(Duration::from_millis(299)), 0);
        assert_eq!(frame(Duration::from_millis(300)), 1);
        assert_eq!(frame(Duration::from_secs(3)), 10);
    }
}
