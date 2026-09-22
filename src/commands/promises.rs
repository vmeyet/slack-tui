//! `slack promises`: the follow-ups you promised and have not closed.
use crate::cli::PromisesArgs;
use crate::ctx::Ctx;
use crate::promises::{self, Promise};
use crate::render::{self, time};
use crate::typesafe::{TypeSafe, Unavailable};
use anyhow::{Result, anyhow};

/// Prints the follow-ups Jev found in your own messages.
pub async fn run(ctx: &mut Ctx, args: PromisesArgs) -> Result<()> {
    let oldest = time::since_to_ts(&args.since).ok_or_else(|| anyhow!("`{}` is not a duration like 3d, 2w or a date", args.since))?;
    let jev = match connect(ctx.config.typesafe.enabled) {
        Ok(jev) => jev,
        Err(unavailable) => return skip(ctx, &unavailable),
    };
    let me = ctx.slack.auth_test().await?.user_id;
    let sent = promises::sent_since(&ctx.slack, &oldest).await?;
    ctx.dir.channels().await?;
    ctx.dir.users().await?;
    let names = ctx.dir.names();
    let tracked = match promises::track(&jev, &ctx.cache, &sent, &names, &names.user_label(&me)).await {
        Ok(tracked) => tracked,
        Err(unavailable) => return skip(ctx, &unavailable),
    };
    let shown: Vec<Promise> = tracked.into_iter().filter(|p| args.all || !p.closed).collect();
    if ctx.json {
        return ctx.emit(&shown);
    }
    print!("{}", render::promises(&ctx.theme, &names, &shown));
    Ok(())
}

/// Only Jev can tell a promise from chatter, so it must be switched on.
fn connect(enabled: bool) -> Result<TypeSafe, Unavailable> {
    if !enabled {
        return Err(Unavailable("set `[typesafe] enabled = true` in the config".into()));
    }
    TypeSafe::connect()
}

#[allow(clippy::unnecessary_wraps)]
fn skip(ctx: &Ctx, unavailable: &Unavailable) -> Result<()> {
    eprintln!("{}", ctx.theme.dim(&unavailable.notice()));
    Ok(())
}
