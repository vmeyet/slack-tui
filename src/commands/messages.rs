use crate::api::{Message, rtm};
use crate::cli::MessagesArgs;
use crate::ctx::Ctx;
use crate::render::{self, time};
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub async fn run(ctx: &mut Ctx, args: MessagesArgs) -> Result<()> {
    let oldest = match &args.since {
        Some(spec) => Some(time::since_to_ts(spec).ok_or_else(|| anyhow::anyhow!("`{spec}` is not a duration like 2h, 3d, 1w or a date"))?),
        None => None,
    };
    let channel = ctx.dir.channel_id(&args.channel).await?;
    let messages = ctx.slack.history(&channel, args.limit, oldest.as_deref()).await?;
    let replies = if args.threads { load_threads(ctx, &channel, &messages).await? } else { HashMap::new() };
    let all: Vec<Message> = messages.iter().cloned().chain(replies.values().flatten().cloned()).collect();
    ctx.dir.learn_users(&all).await?;
    if ctx.json {
        ctx.emit(&serde_json::json!({"channel": channel, "messages": messages, "replies": replies}))?;
    } else {
        if ctx.dir.channels_snapshot().is_empty() {
            let _ = ctx.dir.channels().await;
        }
        let names = ctx.dir.names();
        let label = names.channel_label(&channel);
        print!("{}", render::messages(&ctx.theme, &names, &label, &messages, &replies));
    }
    if let Some(since) = args.since.filter(|_| messages.is_empty()) {
        bail!("no messages since {since}");
    }
    if args.follow {
        let last = messages.last().map(|m| m.ts.clone()).unwrap_or_else(|| format!("{}.000000", chrono::Utc::now().timestamp()));
        return follow(ctx, &channel, last).await;
    }
    Ok(())
}

const POLL_EVERY: Duration = Duration::from_secs(5);

/// Prints new messages from the live feed, or by polling when the feed is refused.
async fn follow(ctx: &mut Ctx, channel: &str, mut last_ts: String) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(rtm::stream(ctx.slack.clone(), tx));
    eprintln!("{}", ctx.theme.dim("following… ctrl-c to stop"));
    while let Some(event) = rx.recv().await {
        match event {
            rtm::Event::Connected => eprintln!("{}", ctx.theme.ok("● live")),
            rtm::Event::Message { channel: c, message } if c == channel => {
                last_ts = message.ts.clone();
                print_live(ctx, std::slice::from_ref(&message)).await?;
            }
            rtm::Event::GaveUp(reason) => {
                eprintln!("{}", ctx.theme.dim(&format!("live feed unavailable ({reason}), polling every {}s", POLL_EVERY.as_secs())));
                break;
            }
            _ => {}
        }
    }
    loop {
        tokio::time::sleep(POLL_EVERY).await;
        let fresh: Vec<Message> = ctx.slack.history(channel, 100, Some(&last_ts)).await?.into_iter().filter(|m| m.ts > last_ts).collect();
        if let Some(m) = fresh.last() {
            last_ts = m.ts.clone();
        }
        print_live(ctx, &fresh).await?;
    }
}

async fn print_live(ctx: &mut Ctx, messages: &[Message]) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    ctx.dir.learn_users(messages).await?;
    let names = ctx.dir.names();
    for m in messages {
        if ctx.json {
            println!("{}", serde_json::to_string(m)?);
        } else {
            print!("{}", render::message(&ctx.theme, &names, m, 0));
        }
    }
    use std::io::Write;
    std::io::stdout().flush()?;
    Ok(())
}

async fn load_threads(ctx: &mut Ctx, channel: &str, messages: &[Message]) -> Result<HashMap<String, Vec<Message>>> {
    let mut replies = HashMap::new();
    for root in messages.iter().filter(|m| m.is_thread_root()) {
        replies.insert(root.ts.clone(), ctx.slack.replies(channel, &root.ts).await?);
    }
    Ok(replies)
}
