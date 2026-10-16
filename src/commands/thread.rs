//! `slack thread`: one thread in full.
use super::parse_ref;
use crate::cli::RefArgs;
use crate::ctx::Ctx;
use crate::{named, render};
use anyhow::Result;

/// Prints the whole thread the reference points to.
pub async fn run(ctx: &mut Ctx, args: RefArgs) -> Result<()> {
    let r = parse_ref(ctx, &args.reference).await?;
    let messages = ctx.slack.replies(&r.channel, r.thread_root()).await?;
    ctx.dir.learn_users(&messages).await?;
    let _ = ctx.dir.channels().await;
    let names = ctx.dir.names();
    if ctx.json {
        let messages = named::messages(&messages, &names);
        return ctx.emit(&serde_json::json!({"channel": r.channel, "thread_ts": r.thread_root(), "messages": messages}));
    }
    print!("{}", render::thread(&ctx.theme, &names, &names.channel_label(&r.channel), &messages));
    Ok(())
}
