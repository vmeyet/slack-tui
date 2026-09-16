pub mod app;
pub mod ui;

use crate::api::rtm;
use crate::ctx::Ctx;
use crate::markdown;
use crate::resolve::Directory;
use anyhow::Result;
use app::{Action, App, ChannelRow, Incoming, Kind};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

pub async fn run(ctx: Ctx) -> Result<()> {
    let slack = ctx.slack.clone();
    let dir = Arc::new(Mutex::new(ctx.dir));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = App::new();
    if let Some(color) = ctx.config.tui.highlight.as_deref() {
        app.highlight = color
            .parse()
            .map_err(|_| anyhow::anyhow!("config `tui.highlight = \"{color}\"` is not a colour (try `darkgray`, `#2a2a2a` or `236`)"))?;
    }
    let mut terminal = ratatui::init();
    spawn(Action::LoadChannels, slack.clone(), dir.clone(), tx.clone());
    spawn_live(slack.clone(), tx.clone());
    let mut events = EventStream::new();
    let result = loop {
        if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
            break Err(e.into());
        }
        let actions = tokio::select! {
            Some(incoming) = rx.recv() => app.apply(incoming),
            Some(event) = events.next() => match event {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => app.handle_key(key),
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
    ratatui::restore();
    result
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
            let rows = d
                .conversations(false)
                .into_iter()
                .map(|(c, label)| ChannelRow { id: c.id.clone(), label, kind: Kind::from(c.kind()) })
                .collect();
            Ok(Incoming::Channels(rows, d.names()))
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
        Action::Yank { channel, ts } => {
            let url = slack.permalink(&channel, &ts).await?;
            let mut child = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn()?;
            std::io::Write::write_all(child.stdin.as_mut().expect("piped"), url.as_bytes())?;
            child.wait()?;
            Ok(Incoming::Status("permalink copied".into()))
        }
    }
}
