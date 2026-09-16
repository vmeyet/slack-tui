use crate::cli::SearchArgs;
use crate::ctx::Ctx;
use crate::render;
use anyhow::Result;

pub fn build_query(args: &SearchArgs) -> String {
    let mut parts = vec![args.query.join(" ")];
    if let Some(c) = &args.channel {
        parts.push(format!("in:#{}", c.trim_start_matches('#')));
    }
    if let Some(u) = &args.from {
        parts.push(format!("from:@{}", u.trim_start_matches('@')));
    }
    if let Some(d) = &args.after {
        parts.push(format!("after:{d}"));
    }
    if let Some(d) = &args.before {
        parts.push(format!("before:{d}"));
    }
    parts.join(" ")
}

pub async fn run(ctx: &mut Ctx, args: SearchArgs) -> Result<()> {
    let query = build_query(&args);
    let result = ctx.slack.search(&query, args.limit).await?;
    if ctx.json {
        return ctx.emit(&result);
    }
    print!("{}", render::search(&ctx.theme, result.total, &result.matches));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_gets_modifiers() {
        let args = SearchArgs {
            query: vec!["deploy".into(), "failed".into()],
            channel: Some("#ops".into()),
            from: Some("vivien".into()),
            after: Some("2026-09-01".into()),
            before: None,
            limit: 5,
        };
        assert_eq!(build_query(&args), "deploy failed in:#ops from:@vivien after:2026-09-01");
    }
}
