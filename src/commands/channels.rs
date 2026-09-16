use crate::api::ChannelKind;
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
    if has_dms && ctx.dir.users_snapshot().is_empty() {
        ctx.dir.users().await?;
    }
    let query = args.query.as_deref().map(|q| q.trim_start_matches(['#', '@']).to_lowercase()).unwrap_or_default();
    let mut listed: Vec<_> = ctx
        .dir
        .channels_snapshot()
        .iter()
        .filter(|c| args.all || c.is_member || c.is_im || c.is_mpim)
        .map(|c| (c, ctx.dir.display_channel(c)))
        .filter(|(_, label)| label.to_lowercase().contains(&query))
        .collect();
    listed.sort_by_key(|(c, label)| (kind_rank(c.kind()), label.to_lowercase()));
    if ctx.json {
        let channels: Vec<_> = listed.iter().map(|(c, _)| *c).collect();
        return ctx.emit(&channels);
    }
    print!("{}", render::channels(&ctx.theme, &listed));
    Ok(())
}

fn kind_rank(kind: ChannelKind) -> u8 {
    match kind {
        ChannelKind::Public => 0,
        ChannelKind::Private => 1,
        ChannelKind::GroupDm => 2,
        ChannelKind::Dm => 3,
    }
}
