pub mod app;
pub mod firehose;
pub mod inbox;
pub mod jump;
pub mod palette;
pub mod theme;
pub mod ui;

use crate::api::rtm;
use crate::ctx::Ctx;
use crate::markdown;
use crate::resolve::Directory;
use anyhow::Result;
use app::{Action, App, ChannelRow, Incoming, Kind};
use crossterm::event::{
    Event, EventStream, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

pub async fn run(ctx: Ctx) -> Result<()> {
    run_with(ctx, false).await
}

pub async fn run_inbox(ctx: Ctx) -> Result<()> {
    run_with(ctx, true).await
}

/// `tui.theme` picks the palette, `tui.highlight` then overrides its selected-row surface.
fn theme_from(config: &crate::config::Tui) -> Result<theme::Theme> {
    let mut theme = match config.theme.as_deref() {
        Some(name) => theme::Theme::named(name)
            .ok_or_else(|| anyhow::anyhow!("config `tui.theme = \"{name}\"` is not a theme (try {})", theme::Theme::NAMES.join(", ")))?,
        None => theme::Theme::default(),
    };
    if let Some(color) = config.highlight.as_deref() {
        theme.surface = color
            .parse()
            .map_err(|_| anyhow::anyhow!("config `tui.highlight = \"{color}\"` is not a colour (try `darkgray`, `#2a2a2a` or `236`)"))?;
    }
    Ok(theme)
}

async fn run_with(ctx: Ctx, open_inbox: bool) -> Result<()> {
    let workspace = ctx.workspace.clone().unwrap_or_else(|| "env".into());
    let slack = ctx.slack.clone();
    let dir = Arc::new(Mutex::new(ctx.dir));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new();
    app.theme = theme_from(&ctx.config.tui)?;
    app.workspace = workspace;
    app.highlighter = crate::firehose::Highlighter::new(&ctx.config.firehose.highlight)?;
    let mut terminal = ratatui::init();
    let enhanced = enable_modifier_keys();
    spawn(Action::LoadChannels, slack.clone(), dir.clone(), tx.clone());
    if open_inbox {
        for action in app.open_inbox() {
            spawn(action, slack.clone(), dir.clone(), tx.clone());
        }
    }
    spawn_live(slack.clone(), tx.clone());
    let mut events = EventStream::new();
    let result = loop {
        if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
            break Err(e.into());
        }
        let actions = tokio::select! {
            Some(incoming) = rx.recv() => app.apply(incoming),
            Some(event) = events.next() => match event {
                Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => app.handle_key(key),
                Ok(_) => vec![],
                Err(e) => break Err(e.into()),
            },
        };
        for action in actions {
            spawn(action, slack.clone(), dir.clone(), tx.clone());
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
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    crossterm::execute!(std::io::stdout(), PushKeyboardEnhancementFlags(flags)).is_ok()
}

fn spawn_live(slack: crate::api::Slack, tx: mpsc::UnboundedSender<Incoming>) {
    let (live_tx, mut live_rx) = mpsc::unbounded_channel();
    tokio::spawn(rtm::stream(slack, live_tx));
    let forward = tx.clone();
    tokio::spawn(async move {
        while let Some(event) = live_rx.recv().await {
            if forward.send(Incoming::Live(event)).is_err() {
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

fn spawn(action: Action, slack: crate::api::Slack, dir: Arc<Mutex<Directory>>, tx: mpsc::UnboundedSender<Incoming>) {
    tokio::spawn(async move {
        let outcome = perform(action, &slack, &dir).await;
        let _ = tx.send(outcome.unwrap_or_else(|e| Incoming::Error(e.to_string())));
    });
}

async fn perform(action: Action, slack: &crate::api::Slack, dir: &Mutex<Directory>) -> Result<Incoming> {
    match action {
        Action::LoadChannels => {
            let mut d = dir.lock().await;
            d.channels().await?;
            let _ = d.users().await;
            let _ = d.learn_dm_users().await;
            let rows: Vec<ChannelRow> =
                d.conversations(false).into_iter().map(|(c, label)| ChannelRow::new(&c.id, &label, Kind::from(c.kind()))).collect();
            let sections = slack.sections().await.unwrap_or_default();
            let muted = slack.muted().await.unwrap_or_default();
            let me = slack.auth_test().await.map(|i| i.user_id).unwrap_or_default();
            let badges = match slack.counts().await {
                Ok(counts) => counts
                    .channels
                    .iter()
                    .chain(&counts.ims)
                    .chain(&counts.mpims)
                    .filter(|c| c.has_unreads || c.mention_count > 0)
                    .map(|c| (c.id.clone(), app::Badge { unread: c.has_unreads, mentions: c.mention_count }))
                    .collect(),
                Err(_) => std::collections::HashMap::new(),
            };
            Ok(Incoming::Channels { rows: app::arrange(rows, &sections, &muted), people: d.people(), names: d.names(), badges, me })
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
            Ok(Incoming::Status(format!("reacted :{name}:")))
        }
        Action::Search(query) => {
            let result = slack.search(&query, 50).await?;
            Ok(Incoming::SearchResults(result.matches))
        }
        Action::Open { channel, ts } => {
            let url = slack.permalink(&channel, &ts).await?;
            std::process::Command::new("open").arg(&url).spawn()?;
            Ok(Incoming::Status("opened in Slack".into()))
        }
        Action::OpenUrl(url) => {
            std::process::Command::new("open").arg(&url).spawn()?;
            Ok(Incoming::Status(format!("opened {url}")))
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
            Ok(Incoming::Status(format!("sent to {}", d.names().channel_label(&channel))))
        }
        Action::MarkChannelRead { channel, ts } => {
            slack.mark_read(&channel, &ts).await?;
            Ok(Incoming::Status("marked read".into()))
        }
        Action::Export { path, label, messages, format } => {
            let names = dir.lock().await.names();
            let body = match format {
                palette::Format::Json => serde_json::to_string_pretty(&messages)?,
                palette::Format::Markdown => {
                    let theme = crate::render::Theme::plain(100);
                    crate::render::messages(&theme, &names, &label, &messages, &std::collections::HashMap::new())
                }
            };
            std::fs::write(&path, body)?;
            Ok(Incoming::Status(format!("saved {}", path.display())))
        }
        Action::OpenDm(user) => {
            let channel = slack.open_dm(&user).await?;
            Ok(Incoming::DmOpened(channel))
        }
        Action::LoadInbox => {
            let me = slack.auth_test().await?.user_id;
            let mut d = dir.lock().await;
            let items = crate::inbox::fetch(slack, &mut d, &me).await?;
            Ok(Incoming::Inbox { items, names: d.names() })
        }
        Action::MarkRead(item) => {
            crate::inbox::mark_read(slack, &item).await?;
            Ok(Incoming::Status(String::new()))
        }
        Action::SaveInbox { workspace, state } => {
            state.save(&workspace)?;
            Ok(Incoming::Status(String::new()))
        }
        Action::Yank { channel, ts } => {
            let url = slack.permalink(&channel, &ts).await?;
            let mut child = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
            std::io::Write::write_all(child.stdin.as_mut().expect("piped"), url.as_bytes())?;
            child.wait()?;
            Ok(Incoming::Status("permalink copied".into()))
        }
    }
}
