use crate::api::rtm;
use crate::cli::FirehoseArgs;
use crate::ctx::Ctx;
use crate::firehose::{self, Highlighter, Line};
use crate::render;
use crate::resolve::NameBook;
use crate::typesafe::{TypeSafe, Unavailable};
use anyhow::{Result, bail};
use futures_util::StreamExt;
use futures_util::stream::FuturesOrdered;
use std::io::Write;
use tokio::sync::mpsc;

/// Jev and the handle it knows as `me`, while it keeps answering.
#[derive(Clone)]
struct Tagger {
    jev: TypeSafe,
    me: String,
}

pub async fn run(ctx: &mut Ctx, args: FirehoseArgs) -> Result<()> {
    let patterns: Vec<String> = ctx.config.firehose.highlight.iter().cloned().chain(args.highlight).collect();
    let hl = Highlighter::new(&patterns)?;
    let _ = ctx.dir.channels().await;
    let mut tagger = tagger(ctx).await?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(rtm::stream(ctx.slack.clone(), tx));
    eprintln!("{}", ctx.theme.dim("firehose… ctrl-c to stop"));
    let mut pending = FuturesOrdered::new();
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                match &event {
                    rtm::Event::Connected => eprintln!("{}", ctx.theme.ok("● live")),
                    rtm::Event::GaveUp(reason) => bail!("the live feed is unavailable: {reason}"),
                    _ => {}
                }
                let Some(line) = Line::from_event(&event) else { continue };
                if let Some(user) = &line.user {
                    ctx.dir.learn_ids(std::slice::from_ref(user)).await?;
                }
                pending.push_back(tagged(tagger.clone(), line, ctx.dir.names()));
            }
            Some((line, failure)) = pending.next() => {
                if let Some(unavailable) = failure.filter(|_| tagger.take().is_some()) {
                    eprintln!("{}", ctx.theme.dim(&unavailable.notice()));
                }
                if !(args.hide_noise && line.is_noise()) {
                    print_line(ctx, &line, &hl)?;
                }
            }
        }
    }
    Ok(())
}

/// Only when `[typesafe] enabled`; a failed connection is one notice, then the plain firehose.
async fn tagger(ctx: &mut Ctx) -> Result<Option<Tagger>> {
    if !ctx.config.typesafe.enabled {
        return Ok(None);
    }
    let jev = match TypeSafe::connect() {
        Ok(jev) => jev,
        Err(unavailable) => {
            eprintln!("{}", ctx.theme.dim(&unavailable.notice()));
            return Ok(None);
        }
    };
    let me = ctx.slack.auth_test().await?.user_id;
    ctx.dir.learn_ids(std::slice::from_ref(&me)).await?;
    Ok(Some(Tagger { jev, me: ctx.dir.names().user_label(&me) }))
}

/// Runs beside the stream: the loop keeps reading while Jev thinks, and lines still print in order.
async fn tagged(tagger: Option<Tagger>, line: Line, names: NameBook) -> (Line, Option<Unavailable>) {
    let Some(Tagger { jev, me }) = tagger else { return (line, None) };
    match firehose::classify(&jev, &line.tag_state(&names, &me)).await {
        Ok(tag) => (Line { tag: Some(tag), ..line }, None),
        Err(unavailable) => (line, Some(unavailable)),
    }
}

fn print_line(ctx: &Ctx, line: &Line, hl: &Highlighter) -> Result<()> {
    if ctx.json {
        println!("{}", serde_json::to_string(line)?);
    } else {
        print!("{}", render::firehose_line(&ctx.theme, &ctx.dir.names(), line, hl));
    }
    std::io::stdout().flush()?;
    Ok(())
}
