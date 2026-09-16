use crate::cli::ChannelsArgs;
use crate::ctx::Ctx;
use crate::render;
use anyhow::Result;

pub async fn run(ctx: &mut Ctx, args: ChannelsArgs) -> Result<()> {
    if args.refresh {
        ctx.dir.refresh_channels().await?;
    }
    ctx.dir.channels().await?;
    let has_dms = ctx.dir.channels_snapshot().iter().any(|c| c.is_im);
    if has_dms {
        ctx.dir.users().await?;
        ctx.dir.learn_dm_users().await?;
    }
    let query = args.query.as_deref().map(|q| q.trim_start_matches(['#', '@']).to_lowercase()).unwrap_or_default();
    let listed: Vec<_> = ctx.dir.conversations(args.all).into_iter().filter(|(_, label)| label.to_lowercase().contains(&query)).collect();
    if ctx.json {
        let channels: Vec<_> = listed.iter().map(|(c, _)| *c).collect();
        return ctx.emit(&channels);
    }
    print!("{}", render::channels(&ctx.theme, &listed));
    Ok(())
}
