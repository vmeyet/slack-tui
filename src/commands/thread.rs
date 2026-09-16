use super::parse_ref;
use crate::cli::RefArgs;
use crate::ctx::Ctx;
use crate::render;
use anyhow::Result;

pub async fn run(ctx: &mut Ctx, args: RefArgs) -> Result<()> {
    let r = parse_ref(ctx, &args.reference).await?;
    let messages = ctx.slack.replies(&r.channel, r.thread_root()).await?;
    ctx.dir.learn_users(&messages).await?;
    if ctx.json {
        return ctx.emit(&serde_json::json!({"channel": r.channel, "thread_ts": r.thread_root(), "messages": messages}));
    }
    if ctx.dir.channels_snapshot().is_empty() {
        let _ = ctx.dir.channels().await;
    }
    let names = ctx.dir.names();
    print!("{}", render::thread(&ctx.theme, &names, &names.channel_label(&r.channel), &messages));
    Ok(())
}
