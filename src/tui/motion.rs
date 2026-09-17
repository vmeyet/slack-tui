//! What moves on screen is drawn from the time elapsed since start, so nothing has to count frames.
use std::time::Duration;

/// One step of the empty-state animation.
pub const FRAME: Duration = Duration::from_millis(300);
pub const SPINNER_FRAME: Duration = Duration::from_millis(80);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn frame(elapsed: Duration) -> u32 {
    (elapsed.as_millis() / FRAME.as_millis()) as u32
}

pub fn spinner(elapsed: Duration) -> &'static str {
    SPINNER[(elapsed.as_millis() / SPINNER_FRAME.as_millis()) as usize % SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_steps_every_80ms_and_wraps_around() {
        assert_eq!(spinner(Duration::ZERO), "⠋");
        assert_eq!(spinner(Duration::from_millis(79)), "⠋");
        assert_eq!(spinner(Duration::from_millis(80)), "⠙");
        assert_eq!(spinner(Duration::from_millis(799)), "⠏");
        assert_eq!(spinner(Duration::from_millis(800)), "⠋");
    }

    #[test]
    fn frame_steps_every_300ms() {
        assert_eq!(frame(Duration::from_millis(299)), 0);
        assert_eq!(frame(Duration::from_millis(300)), 1);
        assert_eq!(frame(Duration::from_secs(3)), 10);
    }
}
