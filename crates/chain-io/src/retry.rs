//! Retrying reads that fail with something other than the node's answer, up to a limit, so a
//! dead endpoint stops the run instead of hanging it (D88).

use std::time::Duration;

use futures::future::LocalBoxFuture;
use tokio::time::Instant;

use crate::ChainError;

/// After a failed read, wait this long before retrying, doubling up to `MAX_BACKOFF`.
const FIRST_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(8);

/// How long the check before a run keeps retrying an endpoint that doesn't answer. Short, so a
/// dead endpoint stops the run within seconds.
pub(crate) const CHECK_LIMIT: Duration = Duration::from_secs(5);

/// How long a read may go unanswered during a run before chain I/O gives up on its endpoint.
/// Long enough to ride out a rate-limit window (Base's is 30s, D82) or a brief outage.
pub(crate) const ANSWER_LIMIT: Duration = Duration::from_secs(60);

/// Retries `call` with backoff until it succeeds, or until it has been failing for `limit`.
pub(crate) async fn retry_for<'a, T>(
    what: &'static str,
    limit: Duration,
    mut call: impl FnMut() -> LocalBoxFuture<'a, Result<T, ChainError>>,
) -> Result<T, ChainError> {
    let first = Instant::now();
    let mut backoff = FIRST_BACKOFF;
    loop {
        match call().await {
            Ok(value) => return Ok(value),
            Err(error) if first.elapsed() >= limit => {
                return Err(ChainError::Unanswered {
                    what,
                    waited: first.elapsed(),
                    last: error.to_string(),
                });
            }
            Err(error) => {
                tracing::warn!(%error, ?backoff, "{what} failed; retrying");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}
