//! `slack react`: an emoji reaction on a message.
use super::parse_ref;
use crate::cli::ReactArgs;
use crate::ctx::Ctx;
use anyhow::Result;

#[allow(clippy::expect_used)]
/// Adds the emoji reaction to the referenced message.
pub async fn run(ctx: &mut Ctx, args: ReactArgs) -> Result<()> {
    let (emoji, reference) = args.args.split_last().expect("clap enforces 2..=3 args");
    let r = parse_ref(ctx, reference).await?;
    let name = emoji.trim_matches(':');
    ctx.slack.react(&r.channel, &r.ts, name).await?;
    if ctx.json {
        return ctx.emit(&serde_json::json!({"channel": r.channel, "ts": r.ts, "name": name}));
    }
    println!("{} reacted :{name}:", ctx.theme.ok("✓"));
    Ok(())
}
