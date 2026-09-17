use super::{Action, Badge, ChannelRow, Focus, Input, Kind, Live, Thread, Toast};
use crate::api::{File, Message, SearchMatch};
use crate::firehose::{Highlighter, Line as LiveLine};
use crate::inbox::State;
use crate::resolve::NameBook;
use crate::tui::firehose::Firehose;
use crate::tui::images::Thumbs;
use crate::tui::inbox::Inbox;
use crate::tui::jump::Jump;
use crate::tui::palette::Palette;
use crate::tui::theme::Theme;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

/// What the app is started with; everything else it learns from `Incoming`.
pub struct Settings {
    pub theme: Theme,
    pub workspace: String,
    pub highlighter: Highlighter,
    pub thumbs: Thumbs,
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
    pub(in crate::tui) thread: Option<Thread>,
    pub(in crate::tui) search: Option<Vec<SearchMatch>>,
    pub(in crate::tui) focus: Focus,
    pub(in crate::tui) input: Option<Input>,
    pub(in crate::tui) buffer: String,
    /// Where the user is; what just happened goes in `toast`.
    pub(in crate::tui) toast: Option<Toast>,
    pub(in crate::tui) loading: bool,
    pub(in crate::tui) names: NameBook,
    pub(in crate::tui) help: bool,
    pub(in crate::tui) should_quit: bool,
    pub(in crate::tui) live: Live,
    pub(in crate::tui) unread: HashSet<String>,
    pub(in crate::tui) badges: HashMap<String, Badge>,
    pub(in crate::tui) me: String,
    pub(in crate::tui) theme: Theme,
    pub(in crate::tui) started: Instant,
    /// Set by the event loop each time it wakes, so nothing below reads the clock.
    pub(in crate::tui) now: Instant,
    pub(in crate::tui) thumbs: Thumbs,
    pub(in crate::tui) inbox: Option<Inbox>,
    pub(in crate::tui) workspace: String,
    pub(in crate::tui) jump: Option<Jump>,
    pub(in crate::tui) people: Vec<(String, String)>,
    pub(in crate::tui) wall: VecDeque<LiveLine>,
    pub(in crate::tui) firehose: Option<Firehose>,
    pub(in crate::tui) highlighter: Highlighter,
    /// Reading mode: only the conversation, centered, times shown on the selected row.
    pub(in crate::tui) zen: bool,
    pub(in crate::tui) palette: Option<Palette>,
    pub(in crate::tui) palette_history: Vec<String>,
    /// Scroll offsets survive between frames so the viewport only moves when the selection leaves it.
    pub(in crate::tui) channels_view: ListState,
    pub(in crate::tui) messages_view: ListState,
    pub(in crate::tui) thread_view: ListState,
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
            thread: None,
            search: None,
            focus: Focus::default(),
            input: None,
            buffer: String::new(),
            toast: None,
            loading: false,
            names: NameBook::default(),
            help: false,
            should_quit: false,
            live: Live::default(),
            unread: HashSet::new(),
            badges: HashMap::new(),
            me: String::new(),
            theme: Theme::default(),
            started: now,
            now,
            thumbs: Thumbs::off(),
            inbox: None,
            workspace: "env".into(),
            jump: None,
            people: vec![],
            wall: VecDeque::new(),
            firehose: None,
            highlighter: Highlighter::default(),
            zen: false,
            palette: None,
            palette_history: vec![],
            channels_view: ListState::default(),
            messages_view: ListState::default(),
            thread_view: ListState::default(),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self { loading: true, ..Default::default() }
    }

    pub fn with(settings: Settings) -> Self {
        let Settings { theme, workspace, highlighter, thumbs } = settings;
        Self { theme, workspace, highlighter, thumbs, ..Self::new() }
    }

    pub fn visible_channels(&self) -> Vec<&ChannelRow> {
        let f = self.filter.to_lowercase();
        self.channels.iter().filter(|c| c.label.to_lowercase().contains(&f)).collect()
    }

    pub fn current_kind(&self) -> Option<Kind> {
        let id = self.current_channel.as_deref()?;
        self.channels.iter().find(|c| c.id == id).map(|c| c.kind)
    }

    /// True while the screen changes with time alone, so the event loop only ticks frames when there is
    /// something to animate.
    pub fn animating(&self) -> bool {
        self.toast_expires() || self.empty_state_visible()
    }

    fn empty_state_visible(&self) -> bool {
        if self.firehose.is_some() || self.jump.is_some() || self.help {
            return false;
        }
        if let Some(inbox) = &self.inbox {
            return inbox.items.is_empty() && !inbox.loading;
        }
        let empty_search = self.search.as_ref().is_some_and(Vec::is_empty);
        !self.loading && (empty_search || (self.search.is_none() && self.messages.is_empty()))
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

    /// Case-insensitive, with or without the `#` / `🔒` mark.
    pub(super) fn channel_named(&self, name: &str) -> Option<&ChannelRow> {
        let bare = name.trim_start_matches('#');
        self.channels
            .iter()
            .find(|c| c.label.eq_ignore_ascii_case(name) || c.label.trim_start_matches(['#', '🔒']).eq_ignore_ascii_case(bare))
    }

    pub(super) fn open_channel(&mut self, id: String) -> Vec<Action> {
        self.unread.remove(&id);
        self.badges.remove(&id);
        self.current_channel = Some(id.clone());
        self.messages.clear();
        self.seen = None;
        self.messages_view = ListState::default();
        self.thread_view = ListState::default();
        self.thread = None;
        self.focus = Focus::Messages;
        self.loading = true;
        vec![Action::LoadHistory(id)]
    }

    pub fn open_inbox(&mut self) -> Vec<Action> {
        self.inbox = Some(Inbox::new(State::load(&self.workspace)));
        vec![Action::LoadInbox]
    }

    pub(super) fn persist_inbox(&self) -> Vec<Action> {
        match &self.inbox {
            Some(inbox) => vec![Action::SaveInbox { workspace: self.workspace.clone(), state: inbox.state.clone() }],
            None => vec![],
        }
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
