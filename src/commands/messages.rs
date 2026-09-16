use crate::api::Message;
use crate::cli::MessagesArgs;
use crate::ctx::Ctx;
use crate::render::{self, time};
use anyhow::{Result, bail};
use std::collections::HashMap;

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
        return ctx.emit(&serde_json::json!({"channel": channel, "messages": messages, "replies": replies}));
    }
    if ctx.dir.channels_snapshot().is_empty() {
        let _ = ctx.dir.channels().await;
    }
    let names = ctx.dir.names();
    let label = names.channel_label(&channel);
    print!("{}", render::messages(&ctx.theme, &names, &label, &messages, &replies));
    if let Some(since) = args.since.filter(|_| messages.is_empty()) {
        bail!("no messages since {since}");
    }
    Ok(())
}

async fn load_threads(ctx: &mut Ctx, channel: &str, messages: &[Message]) -> Result<HashMap<String, Vec<Message>>> {
    let mut replies = HashMap::new();
    for root in messages.iter().filter(|m| m.is_thread_root()) {
        replies.insert(root.ts.clone(), ctx.slack.replies(channel, &root.ts).await?);
    }
    Ok(replies)
}
