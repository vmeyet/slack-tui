//! `slack send`: post a message, markdown by default.
use super::read_input;
use crate::cli::SendArgs;
use crate::ctx::Ctx;
use crate::markdown::{self, validate_blocks};
use crate::permalink;
use crate::render;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

struct Payload {
    text: String,
    blocks: Option<Vec<Value>>,
}

/// Posts the message, or the Block Kit payload, to the target.
pub async fn run(ctx: &mut Ctx, args: SendArgs) -> Result<()> {
    let (channel, thread_ts) = destination(ctx, &args).await?;
    let payload = compose(ctx, &args).await?;
    let request = json!({
        "channel": channel,
        "text": payload.text,
        "blocks": payload.blocks,
        "thread_ts": thread_ts,
        "reply_broadcast": args.broadcast.then_some(true),
    });
    if args.dry_run {
        return ctx.emit(&request);
    }
    let blocks = payload.blocks.map(Value::Array);
    let mut posted = ctx.slack.post_message(&channel, &payload.text, blocks.as_ref(), thread_ts.as_deref(), args.broadcast).await?;
    posted.permalink = ctx.slack.permalink(&posted.channel, &posted.ts).await.unwrap_or_default();
    if ctx.json {
        return ctx.emit(&posted);
    }
    let label = ctx.dir.names().channel_label(&channel);
    print!("{}", render::posted(&ctx.theme, &label, &posted, thread_ts.is_some()));
    Ok(())
}

async fn destination(ctx: &mut Ctx, args: &SendArgs) -> Result<(String, Option<String>)> {
    if permalink::is_permalink(&args.target) {
        let r = permalink::parse(&args.target)?;
        return Ok((r.channel.clone(), Some(r.thread_root().to_owned())));
    }
    let channel = ctx.dir.channel_id(&args.target).await?;
    let thread_ts = match args.thread.as_deref() {
        None => None,
        Some(t) if permalink::is_permalink(t) => {
            let r = permalink::parse(t)?;
            if r.channel != channel {
                bail!("the thread permalink points to another channel ({})", r.channel);
            }
            Some(r.thread_root().to_owned())
        }
        Some(t) if permalink::is_ts(t) => Some(t.to_owned()),
        Some(t) => bail!("`{t}` is neither a permalink nor a message ts"),
    };
    Ok((channel, thread_ts))
}

async fn compose(ctx: &mut Ctx, args: &SendArgs) -> Result<Payload> {
    if let Some(spec) = &args.blocks {
        let blocks = validate_blocks(&read_input(spec)?)?;
        let text = args.text.clone().unwrap_or_else(|| summary(&blocks));
        return Ok(Payload { text, blocks: Some(blocks) });
    }
    let text = if let Some(t) = &args.text {
        t.clone()
    } else {
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            eprintln!("{}", ctx.theme.dim("type your message, finish with ctrl-d"));
        }
        read_input("-").context("reading the message from stdin")?
    };
    let text = text.trim_end().to_owned();
    if text.is_empty() {
        bail!("nothing to send");
    }
    if args.raw {
        return Ok(Payload { text, blocks: None });
    }
    if text.contains('@') || text.contains('#') {
        let _ = ctx.dir.users().await;
        let _ = ctx.dir.channels().await;
    }
    let rendered = markdown::to_blocks(&text, &ctx.dir.names());
    Ok(Payload { text: rendered.text, blocks: Some(rendered.blocks) })
}

fn summary(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| b["text"]["text"].as_str().or_else(|| b["text"].as_str()))
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn summary_takes_block_texts() {
        let blocks = vec![
            json!({"type": "header", "text": {"type": "plain_text", "text": "Deploy"}}),
            json!({"type": "divider"}),
            json!({"type": "section", "text": {"type": "mrkdwn", "text": "done"}}),
        ];
        assert_eq!(summary(&blocks), "Deploy done");
        assert_eq!(summary(&[json!({"type": "divider"})]), "");
    }
}
