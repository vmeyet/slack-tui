//! `slack users`: the people of the workspace.
use crate::cli::UsersArgs;
use crate::ctx::Ctx;
use crate::render;
use anyhow::Result;

/// Prints people, filtered by the query when given.
pub async fn run(ctx: &mut Ctx, args: UsersArgs) -> Result<()> {
    if args.refresh {
        ctx.dir.refresh_users().await?;
    }
    ctx.dir.users().await?;
    let query = args.query.as_deref().map(|q| q.trim_start_matches('@').to_lowercase()).unwrap_or_default();
    let mut listed: Vec<_> = ctx
        .dir
        .users_snapshot()
        .iter()
        .filter(|u| !u.deleted)
        .filter(|u| {
            let hay = format!("{} {} {}", u.handle(), u.real_name, u.profile.real_name).to_lowercase();
            hay.contains(&query)
        })
        .cloned()
        .collect();
    listed.sort_by_key(|u| u.handle().to_lowercase());
    if ctx.json {
        return ctx.emit(&listed);
    }
    print!("{}", render::users(&ctx.theme, &listed));
    Ok(())
}
