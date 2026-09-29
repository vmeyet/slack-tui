pub mod client;
pub mod rtm;
pub mod types;

pub use client::{Params, Slack};
pub use types::*;

use futures_util::{StreamExt, TryStreamExt, stream};

/// Enough to overlap round trips, few enough that Slack rarely answers 429.
const IN_FLIGHT: usize = 8;

/// Runs `call` on every input with a few calls in flight at once; results keep the input order.
pub async fn in_parallel<I, T, F, Fut>(inputs: I, call: F) -> anyhow::Result<Vec<T>>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let calls: Vec<Fut> = inputs.into_iter().map(call).collect();
    stream::iter(calls).buffered(IN_FLIGHT).try_collect().await
}
