pub mod app;
pub mod compose;
pub mod firehose;
pub mod images;
pub mod inbox;
pub mod jump;
pub mod motion;
pub mod palette;
pub mod theme;
pub mod ui;

use crate::api::rtm;
use crate::ctx::Ctx;
use crate::markdown;
use crate::resolve::Directory;
use anyhow::Result;
use app::{Action, App, ChannelRow, Incoming, Kind, Settings};
use crossterm::event::{
    Event, EventStream, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OnceCell, mpsc};

pub async fn run(ctx: Ctx) -> Result<()> {
    run_with(ctx, false).await
}

pub async fn run_inbox(ctx: Ctx) -> Result<()> {
    run_with(ctx, true).await
}

/// `tui.theme` picks the palette, `tui.highlight` adds a fill under the selected row.
fn theme_from(config: &crate::config::Tui) -> Result<theme::Theme> {
    let mut theme = match config.theme.as_deref() {
        Some(name) => theme::Theme::named(name)
            .ok_or_else(|| anyhow::anyhow!("config `tui.theme = \"{name}\"` is not a theme (try {})", theme::Theme::NAMES.join(", ")))?,
        None => theme::Theme::default(),
    };
    if let Some(color) = config.highlight.as_deref().filter(|c| *c != "none") {
        let parsed = color.parse().map_err(|_| {
            anyhow::anyhow!("config `tui.highlight = \"{color}\"` is not a colour (try `darkgray`, `#2a2a2a`, `236` or `none`)")
        })?;
        theme.highlight = Some(parsed);
    }
    Ok(theme)
}

/// What ended the event loop's wait.
enum Wake {
    Incoming(Incoming),
    Event(std::io::Result<Event>),
    Frame,
}

async fn run_with(ctx: Ctx, open_inbox: bool) -> Result<()> {
    let workspace = ctx.workspace.clone().unwrap_or_else(|| "env".into());
    let backend = Backend { slack: ctx.slack.clone(), dir: Arc::new(Mutex::new(ctx.dir)), me: Arc::new(OnceCell::new()) };
    let (tx, mut rx) = mpsc::unbounded_channel();
    let theme = theme_from(&ctx.config.tui)?;
    let highlighter = crate::firehose::Highlighter::new(&ctx.config.firehose.highlight)?;
    let mut terminal = ratatui::init();
    let enhanced = enable_modifier_keys();
    let thumbs = if ctx.config.tui.images.unwrap_or(true) { images::Thumbs::from_terminal() } else { images::Thumbs::off() };
    let mut app = App::with(Settings { theme, workspace, highlighter, thumbs });
    spawn(Action::LoadChannels, backend.clone(), tx.clone());
    spawn(Action::CheckUpdate, backend.clone(), tx.clone());
    if open_inbox {
        for action in app.open_inbox() {
            spawn(action, backend.clone(), tx.clone());
        }
    }
    spawn_live(backend.slack.clone(), tx.clone());
    let mut events = EventStream::new();
    let result = loop {
        if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
            break Err(e.into());
        }
        let redraw_in = app.redraw_in();
        let wake = tokio::select! {
            Some(incoming) = rx.recv() => Wake::Incoming(incoming),
            _ = tokio::time::sleep(redraw_in.unwrap_or_default()), if redraw_in.is_some() => Wake::Frame,
            Some(event) = events.next() => Wake::Event(event),
        };
        app.now = Instant::now();
        let actions = match wake {
            Wake::Incoming(incoming) => app.apply(incoming),
            Wake::Event(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => app.handle_key(key),
            Wake::Event(Ok(_)) | Wake::Frame => vec![],
            Wake::Event(Err(e)) => break Err(e.into()),
        };
        for action in actions {
            match action {
                Action::Compose { channel, thread_ts, draft } => {
                    // The event stream reads stdin from its own thread; it steps aside so the editor gets the keys.
                    drop(events);
                    let composed = {
                        let _paused = Paused::start(&mut terminal, enhanced);
                        compose::edit(&draft)
                    };
                    events = EventStream::new();
                    let _ = tx.send(composed_outcome(channel, thread_ts, composed));
                }
                action => spawn(action, backend.clone(), tx.clone()),
            }
        }
        if app.should_quit {
            break Ok(());
        }
    };
    if enhanced {
        let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    result
}

/// Asks terminals speaking the kitty keyboard protocol to report ⌘ and other modifiers,
/// so ⌘K works where the terminal lets it through (Ghostty, Kitty, WezTerm, iTerm2 with the option on).
fn enable_modifier_keys() -> bool {
    if !crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        return false;
    }
    push_modifier_keys()
}

fn push_modifier_keys() -> bool {
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    crossterm::execute!(std::io::stdout(), PushKeyboardEnhancementFlags(flags)).is_ok()
}

/// The shell gets its terminal back — no raw mode, no alternate screen — for as long as this
/// lives, so a child process inherits a plain stdin and stdout. Dropping it takes the screen
/// back and repaints, whatever the child did.
struct Paused<'a> {
    terminal: &'a mut DefaultTerminal,
    enhanced: bool,
}

impl<'a> Paused<'a> {
    fn start(terminal: &'a mut DefaultTerminal, enhanced: bool) -> Self {
        if enhanced {
            let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
        }
        ratatui::restore();
        Self { terminal, enhanced }
    }
}

impl Drop for Paused<'_> {
    /// Everything `ratatui::init` does except installing a panic hook, which must not stack up
    /// nor be able to panic here, during an unwind.
    fn drop(&mut self) {
        let _ = enable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), EnterAlternateScreen);
        if self.enhanced {
            push_modifier_keys();
        }
        let _ = self.terminal.clear();
    }
}

/// A compose the user backed out of is no event for the app, only a quiet note.
fn composed_outcome(channel: String, thread_ts: Option<String>, composed: Result<Option<String>>) -> Incoming {
    match composed {
        Ok(Some(text)) => Incoming::Composed { channel, thread_ts, text },
        Ok(None) => Incoming::Toast("nothing sent".into()),
        Err(e) => Incoming::Error(e.to_string()),
    }
}

fn spawn_live(slack: crate::api::Slack, tx: mpsc::UnboundedSender<Incoming>) {
    let (live_tx, mut live_rx) = mpsc::unbounded_channel();
    tokio::spawn(rtm::stream(slack, live_tx));
    let forward = tx.clone();
    tokio::spawn(async move {
        while let Some(event) = live_rx.recv().await {
            if forward.send(Incoming::Live(Box::new(event))).is_err() {
                return;
            }
        }
    });
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        tick.tick().await;
        loop {
            tick.tick().await;
            if tx.send(Incoming::Tick).is_err() {
                return;
            }
        }
    });
}

/// What the background tasks share: the API client, the directory, and who is signed in.
#[derive(Clone)]
struct Backend {
    slack: crate::api::Slack,
    dir: Arc<Mutex<Directory>>,
    me: Arc<OnceCell<String>>,
}

impl Backend {
    async fn me(&self) -> Result<String> {
        self.me.get_or_try_init(|| async { Ok(self.slack.auth_test().await?.user_id) }).await.cloned()
    }
}

fn spawn(action: Action, backend: Backend, tx: mpsc::UnboundedSender<Incoming>) {
    tokio::spawn(async move {
        let outcome = perform(action, &backend).await;
        let _ = tx.send(outcome.unwrap_or_else(|e| Incoming::Error(e.to_string())));
    });
}

/// The directory lock is released before the sidebar extras are fetched, so history loads never wait on them.
async fn load_channels(backend: &Backend) -> Result<Incoming> {
    let (rows, people, names) = {
        let mut d = backend.dir.lock().await;
        d.channels().await?;
        let _ = d.users().await;
        let _ = d.learn_dm_users().await;
        let rows: Vec<ChannelRow> =
            d.conversations(false).into_iter().map(|(c, label)| ChannelRow::new(&c.id, &label, Kind::from(c.kind()))).collect();
        (rows, d.people(), d.names())
    };
    let slack = &backend.slack;
    let (sections, muted, me, counts) = tokio::join!(slack.sections(), slack.muted(), backend.me(), slack.counts());
    Ok(Incoming::Channels {
        rows: app::arrange(rows, &sections.unwrap_or_default(), &muted.unwrap_or_default()),
        people,
        names,
        badges: counts.map(|c| badges(&c)).unwrap_or_default(),
        me: me.unwrap_or_default(),
    })
}

fn badges(counts: &crate::api::Counts) -> HashMap<String, app::Badge> {
    counts
        .channels
        .iter()
        .chain(&counts.ims)
        .chain(&counts.mpims)
        .filter(|c| c.has_unreads || c.mention_count > 0)
        .map(|c| (c.id.clone(), app::Badge { unread: c.has_unreads, mentions: c.mention_count }))
        .collect()
}

async fn perform(action: Action, backend: &Backend) -> Result<Incoming> {
    let Backend { slack, dir, .. } = backend;
    match action {
        Action::Compose { .. } => unreachable!("the event loop runs the editor itself"),
        Action::LoadChannels => load_channels(backend).await,
        Action::CheckUpdate => {
            let latest = tokio::task::spawn_blocking(crate::update::latest_commit).await.unwrap_or(None);
            Ok(Incoming::Latest(latest))
        }
        Action::LoadHistory(channel) => {
            let messages = slack.history(&channel, 100, None).await?;
            let mut d = dir.lock().await;
            d.learn_users(&messages).await?;
            Ok(Incoming::History { channel, messages, names: d.names() })
        }
        Action::LoadReplies { channel, ts } => {
            let messages = slack.replies(&channel, &ts).await?;
            let mut d = dir.lock().await;
            d.learn_users(&messages).await?;
            Ok(Incoming::Replies { channel, ts, messages, names: d.names() })
        }
        Action::Send { channel, thread_ts, text } => {
            let names = dir.lock().await.names();
            let rendered = markdown::to_blocks(&text, &names);
            slack
                .post_message(&channel, &rendered.text, Some(&serde_json::Value::Array(rendered.blocks)), thread_ts.as_deref(), false)
                .await?;
            Ok(Incoming::Sent { channel, thread_ts })
        }
        Action::React { channel, ts, name } => {
            slack.react(&channel, &ts, &name).await?;
            Ok(Incoming::Toast(format!("reacted :{name}:")))
        }
        Action::Edit { channel, ts, text } => {
            slack.update_message(&channel, &ts, &text).await?;
            Ok(Incoming::Toast("edited ✓".into()))
        }
        Action::Delete { channel, ts } => {
            slack.delete_message(&channel, &ts).await?;
            Ok(Incoming::Toast("deleted ✓".into()))
        }
        Action::Search(query) => {
            let result = slack.search(&query, 50).await?;
            Ok(Incoming::SearchResults(result.matches))
        }
        Action::Open { channel, ts } => {
            let url = slack.permalink(&channel, &ts).await?;
            std::process::Command::new("open").arg(&url).spawn()?;
            Ok(Incoming::Toast("opened in Slack".into()))
        }
        Action::OpenUrl(url) => {
            std::process::Command::new("open").arg(&url).spawn()?;
            Ok(Incoming::Toast(format!("opened {url}")))
        }
        Action::LoadThreads => {
            let threads = slack.thread_view(30).await.unwrap_or_default();
            let names = dir.lock().await.names();
            let candidates = threads
                .into_iter()
                .map(|t| {
                    let preview: String = crate::mrkdwn::plain(&t.root_msg.text, &names).chars().take(60).collect();
                    jump::Candidate {
                        label: format!("{} · {}", names.channel_label(&t.root_msg.channel), preview.replace('\n', " ")),
                        target: jump::Target::Thread { channel: t.root_msg.channel, ts: t.root_msg.ts },
                    }
                })
                .collect();
            Ok(Incoming::Threads(candidates))
        }
        Action::LearnUsers(ids) => {
            let mut d = dir.lock().await;
            d.learn_ids(&ids).await?;
            Ok(Incoming::Names(d.names()))
        }
        Action::Join(name) => {
            let mut d = dir.lock().await;
            let id = d.channel_id(&name).await?;
            slack.join(&id).await?;
            d.refresh_channels().await?;
            Ok(Incoming::Joined(id))
        }
        Action::Leave(channel) => {
            slack.leave(&channel).await?;
            dir.lock().await.refresh_channels().await?;
            Ok(Incoming::Left(channel))
        }
        Action::SendTo { target, text } => {
            let mut d = dir.lock().await;
            let channel = d.channel_id(&target).await?;
            let rendered = markdown::to_blocks(&text, &d.names());
            slack.post_message(&channel, &rendered.text, Some(&serde_json::Value::Array(rendered.blocks)), None, false).await?;
            Ok(Incoming::Toast(format!("sent to {}", d.names().channel_label(&channel))))
        }
        Action::MarkChannelRead { channel, ts } => {
            slack.mark_read(&channel, &ts).await?;
            Ok(Incoming::Toast("marked read".into()))
        }
        Action::Export { path, label, messages, format } => {
            let names = dir.lock().await.names();
            let body = match format {
                palette::Format::Json => serde_json::to_string_pretty(&messages)?,
                palette::Format::Markdown => {
                    let theme = crate::render::Theme::plain(100);
                    crate::render::messages(&theme, &names, &label, &messages, &HashMap::new())
                }
            };
            std::fs::write(&path, body)?;
            Ok(Incoming::Toast(format!("saved {}", path.display())))
        }
        Action::OpenDm(user) => {
            let channel = slack.open_dm(&user).await?;
            Ok(Incoming::DmOpened(channel))
        }
        Action::LoadInbox => {
            let me = backend.me().await?;
            let mut d = dir.lock().await;
            let items = crate::inbox::fetch(slack, &mut d, &me).await?;
            Ok(Incoming::Inbox { items, names: d.names() })
        }
        Action::MarkRead(item) => {
            crate::inbox::mark_read(slack, &item).await?;
            Ok(Incoming::Toast(String::new()))
        }
        Action::SaveInbox { workspace, state } => {
            state.save(&workspace)?;
            Ok(Incoming::Toast(String::new()))
        }
        Action::LoadImage { id, url } => {
            let image = match slack.download(&url).await {
                Ok(bytes) => tokio::task::spawn_blocking(move || images::decode(&bytes)).await.unwrap_or(None),
                Err(_) => None,
            };
            Ok(Incoming::Thumb { id, image })
        }
        Action::SaveSetting { key, value } => {
            let mut config = crate::config::Config::load()?;
            config.tui = config.tui.with(&key, &value);
            config.save()?;
            Ok(Incoming::Toast(String::new()))
        }
        Action::Yank { channel, ts } => {
            let url = slack.permalink(&channel, &ts).await?;
            let mut child = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
            std::io::Write::write_all(child.stdin.as_mut().expect("piped"), url.as_bytes())?;
            child.wait()?;
            Ok(Incoming::Toast("permalink copied".into()))
        }
    }
}
