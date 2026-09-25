//! `q` and `ctrl-c` ask for a second press, so one stray key never closes the app.
use super::{Action, App};
use std::time::Duration;

/// How long the first press waits for the second.
pub const QUIT_WINDOW: Duration = Duration::from_millis(1500);

/// The key that started a quit: only the same key finishes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitKey {
    Q,
    CtrlC,
}

impl QuitKey {
    fn name(self) -> &'static str {
        match self {
            QuitKey::Q => "q",
            QuitKey::CtrlC => "ctrl-c",
        }
    }
}

impl App {
    /// Quits on the second press of the same key inside the window; the first press only asks.
    pub(super) fn quit_key(&mut self, key: QuitKey) -> Vec<Action> {
        let again = self.quitting.is_some_and(|(pending, at)| pending == key && self.now.duration_since(at) < QUIT_WINDOW);
        if again {
            self.should_quit = true;
            self.quitting = None;
        } else {
            self.quitting = Some((key, self.now));
        }
        vec![]
    }

    /// The status line while a quit waits for its second press.
    pub fn quit_prompt(&self) -> Option<String> {
        let (key, at) = self.quitting?;
        (self.now.duration_since(at) < QUIT_WINDOW).then(|| format!("press {} again to quit", key.name()))
    }
}
