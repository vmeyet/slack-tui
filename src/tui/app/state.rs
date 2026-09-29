use super::{Action, Badge, ChannelRow, Focus, Kind, Live, MyMessage, Overlay, Screen, Thread, Toast, Typing};
use crate::api::{File, Message, SearchMatch};
use crate::firehose::{Highlighter, Line as LiveLine};
use crate::inbox::State;
use crate::resolve::NameBook;
use crate::tui::field::Field;
use crate::tui::images::Thumbs;
use crate::tui::inbox::Inbox;
use crate::tui::theme::Theme;
use crate::tui::ui::Bodies;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

/// What the app is started with; everything else it learns from `Incoming`.
pub struct Settings {
    pub theme: Theme,
    pub workspace: String,
    pub highlighter: Highlighter,
    pub thumbs: Thumbs,
    pub triage: bool,
}

#[derive(Debug)]
pub struct App {
    pub(in crate::tui) channels: Vec<ChannelRow>,
    pub(in crate::tui) filter: String,
    pub(in crate::tui) channel_selected: usize,
    pub(in crate::tui) current_channel: Option<String>,
    pub(in crate::tui) messages: Vec<Message>,
    pub(in crate::tui) message_selected: usize,
    /// Newest message the selection reached; anything newer arrived unseen.
    pub(in crate::tui) seen: Option<String>,
    /// Newest message of the open channel already marked read on Slack.
    pub(in crate::tui) marked: Option<String>,
    pub(in crate::tui) thread: Option<Thread>,
    /// Root of the thread open or on its way; replies for any other arrive too late and are dropped.
    pub(in crate::tui) wanted_thread: Option<String>,
    pub(in crate::tui) search: Option<Vec<SearchMatch>>,
    pub(in crate::tui) focus: Focus,
    pub(in crate::tui) overlay: Option<Overlay>,
    pub(in crate::tui) screen: Option<Screen>,
    pub(in crate::tui) buffer: Field,
    /// How often each emoji was put on a message, most used first in the picker.
    pub(in crate::tui) favorites: HashMap<String, u32>,
    pub(in crate::tui) custom_emoji: Vec<String>,
    /// Where the user is; what just happened goes in `toast`.
    pub(in crate::tui) toast: Option<Toast>,
    /// Who is typing in the open conversation, each until their own keystroke ages out.
    pub(in crate::tui) typing: Vec<Typing>,
    pub(in crate::tui) loading: bool,
    /// Newest commit of the repo, once the daily check answered.
    pub(in crate::tui) latest: Option<String>,
    pub(in crate::tui) names: NameBook,
    pub(in crate::tui) should_quit: bool,
    pub(in crate::tui) quitting: Option<(super::quit::QuitKey, Instant)>,
    pub(in crate::tui) live: Live,
    pub(in crate::tui) unread: HashSet<String>,
    pub(in crate::tui) badges: HashMap<String, Badge>,
    pub(in crate::tui) me: String,
    pub(in crate::tui) theme: Theme,
    pub(in crate::tui) started: Instant,
    /// Set by the event loop each time it wakes, so nothing below reads the clock.
    pub(in crate::tui) now: Instant,
    pub(in crate::tui) thumbs: Thumbs,
    pub(in crate::tui) workspace: String,
    pub(in crate::tui) people: Vec<(String, String)>,
    pub(in crate::tui) wall: VecDeque<LiveLine>,
    pub(in crate::tui) highlighter: Highlighter,
    /// Jev ranks the inbox and tags the firehose: on with `[typesafe] enabled`, off for good once it fails.
    pub(in crate::tui) triage: bool,
    /// Reading mode: only the conversation, centered, times shown on the selected row.
    pub(in crate::tui) zen: bool,
    pub(in crate::tui) palette_history: Vec<String>,
    /// Scroll offsets survive between frames so the viewport only moves when the selection leaves it.
    pub(in crate::tui) channels_view: ListState,
    pub(in crate::tui) messages_view: ListState,
    pub(in crate::tui) thread_view: ListState,
    pub(in crate::tui) message_bodies: Bodies,
    pub(in crate::tui) thread_bodies: Bodies,
}

impl Default for App {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            channels: vec![],
            filter: String::new(),
            channel_selected: 0,
            current_channel: None,
            messages: vec![],
            message_selected: 0,
            seen: None,
            marked: None,
            thread: None,
            wanted_thread: None,
            search: None,
            focus: Focus::default(),
            overlay: None,
            screen: None,
            buffer: Field::default(),
            favorites: HashMap::new(),
            custom_emoji: Vec::new(),
            toast: None,
            typing: vec![],
            loading: false,
            latest: None,
            names: NameBook::default(),
            should_quit: false,
            quitting: None,
            live: Live::default(),
            unread: HashSet::new(),
            badges: HashMap::new(),
            me: String::new(),
            theme: Theme::default(),
            started: now,
            now,
            thumbs: Thumbs::off(),
            workspace: "env".into(),
            people: vec![],
            wall: VecDeque::new(),
            highlighter: Highlighter::default(),
            triage: false,
            zen: false,
            palette_history: vec![],
            channels_view: ListState::default(),
            messages_view: ListState::default(),
            thread_view: ListState::default(),
            message_bodies: Bodies::default(),
            thread_bodies: Bodies::default(),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self { loading: true, ..Default::default() }
    }

    pub fn with(settings: Settings) -> Self {
        let Settings { theme, workspace, highlighter, thumbs, triage } = settings;
        Self { theme, thumbs, workspace, highlighter, triage, ..Self::new() }
    }

    pub fn visible_channels(&self) -> Vec<&ChannelRow> {
        if self.filter.is_empty() {
            return self.channels.iter().collect();
        }
        let filter = self.filter.to_lowercase();
        self.channels.iter().filter(|c| c.label.to_lowercase().contains(&filter)).collect()
    }

    pub fn current_kind(&self) -> Option<Kind> {
        let id = self.current_channel.as_deref()?;
        self.channels.iter().find(|c| c.id == id).map(|c| c.kind)
    }

    /// True while the screen changes with time alone, so the event loop only ticks frames when there is
    /// something to animate.
    pub fn animating(&self) -> bool {
        self.toast_expires() || self.quit_prompt().is_some() || self.typing_line().is_some() || self.empty_state_visible()
    }

    fn empty_state_visible(&self) -> bool {
        if matches!(self.overlay, Some(Overlay::Jump(_) | Overlay::Help)) {
            return false;
        }
        match &self.screen {
            Some(Screen::Firehose(_)) => false,
            Some(Screen::Inbox(inbox)) => inbox.items.is_empty() && !inbox.loading,
            None => {
                let empty_search = self.search.as_ref().is_some_and(Vec::is_empty);
                !self.loading && (empty_search || (self.search.is_none() && self.messages.is_empty()))
            }
        }
    }

    pub fn current_label(&self) -> String {
        self.current_channel.as_deref().map(|id| self.names.channel_label(id)).unwrap_or_default()
    }

    pub fn selected_message(&self) -> Option<&Message> {
        match self.focus {
            Focus::Thread => self.thread.as_ref().and_then(|t| t.messages.get(t.selected)),
            _ => self.messages.get(self.message_selected),
        }
    }

    pub(super) fn selected_ref(&self) -> Option<(String, String)> {
        if let Some(results) = &self.search
            && self.focus == Focus::Messages
        {
            return results.get(self.message_selected).map(|m| (m.channel.id.clone(), m.ts.clone()));
        }
        let channel = match self.focus {
            Focus::Thread => self.thread.as_ref()?.channel.clone(),
            _ => self.current_channel.clone()?,
        };
        Some((channel, self.selected_message()?.ts.clone()))
    }

    /// The selected message when it is the user's own; `verb` names what was refused otherwise.
    /// A search result is refused too: its text is not the one on screen until it is opened.
    pub(super) fn my_message(&self, verb: &str) -> Result<MyMessage, String> {
        let (channel, ts) = self.selected_ref().ok_or_else(|| format!("pick a message to {verb}"))?;
        let message = self.selected_message().filter(|m| m.ts == ts).ok_or_else(|| format!("open the message first, then :{verb}"))?;
        if message.user.as_deref() != Some(self.me.as_str()) {
            return Err(format!("you can only {verb} your own messages"));
        }
        Ok(MyMessage { channel, ts, text: message.text.clone() })
    }

    /// Case-insensitive, with or without the `#` / `🔒` mark.
    pub(super) fn channel_named(&self, name: &str) -> Option<&ChannelRow> {
        let bare = name.trim_start_matches('#');
        self.channels
            .iter()
            .find(|c| c.label.eq_ignore_ascii_case(name) || c.label.trim_start_matches(['#', '🔒']).eq_ignore_ascii_case(bare))
    }

    pub(super) fn open_channel(&mut self, id: String) -> Vec<Action> {
        let left = self.sync_read();
        self.marked = None;
        self.unread.remove(&id);
        self.badges.remove(&id);
        self.current_channel = Some(id.clone());
        self.messages.clear();
        self.typing.clear();
        self.seen = None;
        self.messages_view = ListState::default();
        self.thread_view = ListState::default();
        self.close_thread();
        self.focus = Focus::Messages;
        self.loading = true;
        left.into_iter().chain([Action::LoadHistory(id)]).collect()
    }

    pub(super) fn load_replies(&mut self, channel: String, ts: String) -> Action {
        self.wanted_thread = Some(ts.clone());
        Action::LoadReplies { channel, ts }
    }

    pub(super) fn close_thread(&mut self) {
        self.thread = None;
        self.wanted_thread = None;
    }

    /// Slack keeps the open channel read up to its newest message, so a restart shows no stale dot.
    pub(super) fn sync_read(&mut self) -> Option<Action> {
        let channel = self.current_channel.clone()?;
        let ts = self.messages.last()?.ts.clone();
        if self.marked.as_ref() == Some(&ts) {
            return None;
        }
        self.marked = Some(ts.clone());
        Some(Action::SyncRead { channel, ts })
    }

    pub fn open_inbox(&mut self) -> Vec<Action> {
        self.screen = Some(Screen::Inbox(Inbox::new(State::load(&self.workspace))));
        vec![Action::LoadInbox]
    }

    pub(super) fn persist_inbox(&self) -> Vec<Action> {
        match &self.screen {
            Some(Screen::Inbox(inbox)) => vec![Action::SaveInbox { workspace: self.workspace.clone(), state: inbox.state.clone() }],
            _ => vec![],
        }
    }

    pub(super) fn send(&mut self, channel: String, thread_ts: Option<String>, text: String) -> Vec<Action> {
        self.loading = true;
        self.toast("sending…");
        vec![Action::Send { channel, thread_ts, text }]
    }

    pub(super) fn search_for(&mut self, query: String) -> Vec<Action> {
        self.loading = true;
        self.toast("searching…");
        vec![Action::Search(query)]
    }

    /// Fetches thumbnails for the images now on screen and forgets the ones that left.
    pub(super) fn refresh_thumbs(&mut self) -> Vec<Action> {
        let thread = self.thread.iter().flat_map(|t| t.messages.iter());
        let files: Vec<File> = self.messages.iter().chain(thread).flat_map(|m| m.files.iter().cloned()).collect();
        self.thumbs.keep_only(files.iter().map(|f| f.id.clone()));
        self.thumbs.wanted(&files).into_iter().map(|(id, url)| Action::LoadImage { id, url }).collect()
    }
}
