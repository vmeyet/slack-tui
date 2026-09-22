//! The interactive terminal client.
mod app;
mod complete;
mod compose;
mod field;
mod firehose;
mod images;
mod inbox;
mod jump;
mod motion;
mod palette;
mod theme;
mod ui;

use crate::api::{Message, rtm};
use crate::cache::Cache;
use crate::ctx::Ctx;
use crate::markdown;
use crate::resolve::{Directory, NameBook};
use crate::typesafe::{TypeSafe, Unavailable};
use anyhow::{Context, Result, bail};
use app::{Action, App, ChannelRow, Incoming, Kind, Settings};
use crossterm::event::{
    Event, EventStream, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, OnceCell, mpsc};

/// Opens the interactive client and returns when the user quits.
pub async fn run(ctx: Ctx) -> Result<()> {
    run_with(ctx, false).await
}

pub(crate) async fn run_inbox(ctx: Ctx) -> Result<()> {
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
    let triage = ctx.config.typesafe.enabled;
    let jev = triage.then(TypeSafe::connect);
    let backend = Backend {
        slack: ctx.slack.clone(),
        dir: Arc::new(Mutex::new(ctx.dir)),
        me: Arc::new(OnceCell::new()),
        cache: ctx.cache,
        jev: jev.clone().and_then(Result::ok),
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(Err(unavailable)) = jev {
        let _ = tx.send(Incoming::TriageUnavailable(unavailable));
    }
    let theme = theme_from(&ctx.config.tui)?;
    let highlighter = crate::firehose::Highlighter::new(&ctx.config.firehose.highlight)?;
    let mut terminal = ratatui::init();
    let enhanced = enable_modifier_keys();
    let thumbs = if ctx.config.tui.images.unwrap_or(true) { images::Thumbs::from_terminal() } else { images::Thumbs::off() };
    let mut app = App::with(Settings { theme, workspace, highlighter, thumbs, triage });
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
            () = tokio::time::sleep(redraw_in.unwrap_or_default()), if redraw_in.is_some() => Wake::Frame,
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
/// so ⌘K works where the terminal lets it through (Ghostty, Kitty, `WezTerm`, iTerm2 with the option on).
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

/// What the background tasks share: the API client, the directory, who is signed in, and Jev when it is on.
#[derive(Clone)]
struct Backend {
    slack: crate::api::Slack,
    dir: Arc<Mutex<Directory>>,
    me: Arc<OnceCell<String>>,
    cache: Cache,
    jev: Option<TypeSafe>,
}

impl Backend {
    async fn me(&self) -> Result<String> {
        self.me.get_or_try_init(|| async { Ok(self.slack.auth_test().await?.user_id) }).await.cloned()
    }

    /// What Jev needs besides the question: the client, the names, and the handle of whoever `me` is.
    async fn triage_context(&self) -> Result<(&TypeSafe, NameBook, String)> {
        let jev = self.jev.as_ref().ok_or_else(|| Unavailable("not connected".into()))?;
        let names = self.dir.lock().await.names();
        let me = names.user_label(&self.me().await?);
        Ok((jev, names, me))
    }
}

/// A failed judgment is no error for the app: it turns triage off with one notice.
fn judged<T>(outcome: std::result::Result<T, Unavailable>, arrived: impl FnOnce(T) -> Incoming) -> Incoming {
    outcome.map_or_else(Incoming::TriageUnavailable, arrived)
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
    let slack = &backend.slack;
    match action {
        Action::Compose { .. } => bail!("compose is run by the event loop, not here"),
        Action::LoadChannels => load_channels(backend).await,
        Action::CheckUpdate => Ok(Incoming::Latest(crate::update::latest_commit().await)),
        Action::LoadHistory(channel) => load_history(backend, channel).await,
        Action::LoadReplies { channel, ts } => load_replies(backend, channel, ts).await,
        Action::Send { channel, thread_ts, text } => send(backend, channel, thread_ts, &text).await,
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
        Action::Search(query) => Ok(Incoming::SearchResults(slack.search(&query, 50).await?.matches)),
        Action::Open { channel, ts } => {
            open(&slack.permalink(&channel, &ts).await?)?;
            Ok(Incoming::Toast("opened in Slack".into()))
        }
        Action::OpenUrl(url) => {
            open(&url)?;
            Ok(Incoming::Toast(format!("opened {url}")))
        }
        Action::LoadThreads => load_threads(backend).await,
        Action::LearnUsers(ids) => {
            let mut d = backend.dir.lock().await;
            d.learn_ids(&ids).await?;
            Ok(Incoming::Names(d.names()))
        }
        Action::Join(name) => join(backend, &name).await,
        Action::Leave(channel) => {
            slack.leave(&channel).await?;
            backend.dir.lock().await.refresh_channels().await?;
            Ok(Incoming::Left(channel))
        }
        Action::SendTo { target, text } => send_to(backend, &target, &text).await,
        Action::MarkChannelRead { channel, ts } => {
            slack.mark_read(&channel, &ts).await?;
            Ok(Incoming::Toast("marked read".into()))
        }
        Action::Export { path, label, messages, format } => export(backend, &path, &label, &messages, format).await,
        Action::OpenDm(user) => Ok(Incoming::DmOpened(slack.open_dm(&user).await?)),
        Action::LoadInbox => load_inbox(backend).await,
        Action::Prioritize(items) => {
            let (jev, names, me) = backend.triage_context().await?;
            let verdicts = crate::inbox::prioritize(jev, &backend.cache, &items, &names, &me).await;
            Ok(judged(verdicts, Incoming::Priorities))
        }
        Action::Classify(line) => {
            let (jev, names, me) = backend.triage_context().await?;
            let tag = crate::firehose::classify(jev, &line.tag_state(&names, &me)).await;
            Ok(judged(tag, |tag| Incoming::Tagged { channel: line.channel, ts: line.ts, tag }))
        }
        Action::LoadPromises => load_promises(backend).await,
        Action::MarkRead(item) => {
            crate::inbox::mark_read(slack, &item).await?;
            Ok(Incoming::Toast(String::new()))
        }
        Action::SaveInbox { workspace, state } => {
            state.save(&workspace).await?;
            Ok(Incoming::Toast(String::new()))
        }
        Action::LoadImage { id, url } => Ok(Incoming::Thumb { image: load_image(slack, &url).await, id }),
        Action::SaveSetting { key, value } => save_setting(key, value).await,
        Action::Yank { channel, ts } => yank(slack, &channel, &ts).await,
    }
}

async fn load_history(backend: &Backend, channel: String) -> Result<Incoming> {
    let messages = backend.slack.history(&channel, 100, None).await?;
    let mut d = backend.dir.lock().await;
    d.learn_users(&messages).await?;
    Ok(Incoming::History { channel, messages, names: d.names() })
}

async fn load_replies(backend: &Backend, channel: String, ts: String) -> Result<Incoming> {
    let messages = backend.slack.replies(&channel, &ts).await?;
    let mut d = backend.dir.lock().await;
    d.learn_users(&messages).await?;
    Ok(Incoming::Replies { channel, ts, messages, names: d.names() })
}

async fn send(backend: &Backend, channel: String, thread_ts: Option<String>, text: &str) -> Result<Incoming> {
    let names = backend.dir.lock().await.names();
    post_markdown(&backend.slack, &channel, text, &names, thread_ts.as_deref()).await?;
    Ok(Incoming::Sent { channel, thread_ts })
}

async fn post_markdown(slack: &crate::api::Slack, channel: &str, text: &str, names: &NameBook, thread_ts: Option<&str>) -> Result<()> {
    let rendered = markdown::to_blocks(text, names);
    slack.post_message(channel, &rendered.text, Some(&serde_json::Value::Array(rendered.blocks)), thread_ts, false).await?;
    Ok(())
}

fn open(url: &str) -> Result<()> {
    tokio::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

async fn load_threads(backend: &Backend) -> Result<Incoming> {
    let threads = backend.slack.thread_view(30).await.unwrap_or_default();
    let names = backend.dir.lock().await.names();
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

async fn join(backend: &Backend, name: &str) -> Result<Incoming> {
    let mut d = backend.dir.lock().await;
    let id = d.channel_id(name).await?;
    backend.slack.join(&id).await?;
    d.refresh_channels().await?;
    Ok(Incoming::Joined(id))
}

async fn send_to(backend: &Backend, target: &str, text: &str) -> Result<Incoming> {
    let mut d = backend.dir.lock().await;
    let channel = d.channel_id(target).await?;
    post_markdown(&backend.slack, &channel, text, &d.names(), None).await?;
    Ok(Incoming::Toast(format!("sent to {}", d.names().channel_label(&channel))))
}

async fn export(backend: &Backend, path: &Path, label: &str, messages: &[Message], format: palette::Format) -> Result<Incoming> {
    let names = backend.dir.lock().await.names();
    let body = match format {
        palette::Format::Json => serde_json::to_string_pretty(messages)?,
        palette::Format::Markdown => {
            let theme = crate::render::Theme::plain(100);
            crate::render::messages(&theme, &names, label, messages, &HashMap::new())
        }
    };
    tokio::fs::write(path, body).await?;
    Ok(Incoming::Toast(format!("saved {}", path.display())))
}

async fn load_inbox(backend: &Backend) -> Result<Incoming> {
    let me = backend.me().await?;
    let mut d = backend.dir.lock().await;
    let items = crate::inbox::fetch(&backend.slack, &mut d, &me).await?;
    Ok(Incoming::Inbox { items, names: d.names() })
}

async fn load_promises(backend: &Backend) -> Result<Incoming> {
    let (jev, names, me) = backend.triage_context().await?;
    let oldest = crate::render::time::since_to_ts(crate::promises::DEFAULT_SINCE).unwrap_or_default();
    let sent = crate::promises::sent_since(&backend.slack, &oldest).await?;
    let tracked = crate::promises::track(jev, &backend.cache, &sent, &names, &me).await.map_err(|u| anyhow::anyhow!(u.notice()))?;
    let open: Vec<_> = tracked.into_iter().filter(|p| !p.closed).map(|p| p.message).collect();
    Ok(if open.is_empty() { Incoming::Toast("no open promises ✓".into()) } else { Incoming::SearchResults(open) })
}

/// A picture that fails to download or decode is no error: it shows as unavailable.
async fn load_image(slack: &crate::api::Slack, url: &str) -> Option<image::DynamicImage> {
    let bytes = slack.download(url).await.ok()?;
    tokio::task::spawn_blocking(move || images::decode(&bytes)).await.unwrap_or(None)
}

async fn save_setting(key: String, value: String) -> Result<Incoming> {
    tokio::task::spawn_blocking(move || {
        let mut config = crate::config::Config::load()?;
        config.tui = config.tui.with(&key, &value);
        config.save()
    })
    .await??;
    Ok(Incoming::Toast(String::new()))
}

async fn yank(slack: &crate::api::Slack, channel: &str, ts: &str) -> Result<Incoming> {
    let url = slack.permalink(channel, ts).await?;
    let mut child = tokio::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
    child.stdin.take().context("piped stdin")?.write_all(url.as_bytes()).await?;
    child.wait().await?;
    Ok(Incoming::Toast("permalink copied".into()))
}
