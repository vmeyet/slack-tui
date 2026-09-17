use super::App;
use crate::inbox::newer;
use crate::tui::motion::{FRAME, SPINNER_FRAME};
use std::time::{Duration, Instant};

const TOAST_LIFE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Until {
    Time(Instant),
    NextKey,
}

/// Something that just happened, shown over the status until it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    text: String,
    until: Until,
}

impl Toast {
    fn live(&self, now: Instant) -> bool {
        match self.until {
            Until::Time(end) => now < end,
            Until::NextKey => true,
        }
    }
}

impl App {
    pub(super) fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), until: Until::Time(self.now + TOAST_LIFE) });
    }

    pub(super) fn fail(&mut self, error: impl std::fmt::Display) {
        self.toast = Some(Toast { text: format!("✗ {error}"), until: Until::NextKey });
    }

    pub(super) fn dismiss_error(&mut self) {
        self.toast = self.toast.take().filter(|t| t.until != Until::NextKey);
    }

    fn live_toast(&self) -> Option<&Toast> {
        self.toast.as_ref().filter(|t| t.live(self.now))
    }

    pub fn status_line(&self) -> String {
        match self.live_toast() {
            Some(toast) => toast.text.clone(),
            None => self.location(),
        }
    }

    /// Where the user is, rebuilt on every frame so nothing can leave it stale.
    fn location(&self) -> String {
        if let Some(results) = &self.search {
            return format!("{} results · enter to jump · esc to close", results.len());
        }
        if self.current_channel.is_some() {
            return self.current_label();
        }
        if self.channels.is_empty() {
            return "loading channels…".into();
        }
        format!("{} conversations · ? for help", self.channels.len())
    }

    pub(super) fn toast_expires(&self) -> bool {
        self.live_toast().is_some_and(|t| t.until != Until::NextKey)
    }

    pub fn elapsed(&self) -> Duration {
        self.now.duration_since(self.started)
    }

    /// How long the event loop may sleep before the screen needs a new frame; `None` when nothing moves.
    pub fn redraw_in(&self) -> Option<Duration> {
        if self.spinning() {
            return Some(SPINNER_FRAME);
        }
        self.animating().then_some(FRAME)
    }

    fn spinning(&self) -> bool {
        self.loading || self.inbox.as_ref().is_some_and(|i| i.loading) || self.thumbs.loading()
    }

    /// Messages of the open conversation that arrived below the newest one the selection reached.
    pub fn new_below(&self) -> usize {
        let Some(seen) = self.seen.as_ref().filter(|_| self.search.is_none()) else { return 0 };
        self.messages.iter().filter(|m| newer(&m.ts, seen) && m.user.as_deref() != Some(&self.me)).count()
    }

    pub(super) fn mark_seen(&mut self) {
        let Some(reached) = self.messages.get(self.message_selected).filter(|_| self.search.is_none()) else { return };
        if self.seen.as_ref().is_none_or(|seen| newer(&reached.ts, seen)) {
            self.seen = Some(reached.ts.clone());
        }
    }
}
