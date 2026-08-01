//! SSE plumbing shared by every streaming route.
//!
//! Two jobs:
//! 1. `event()` / `send()` build events whose data is ALWAYS a JSON object —
//!    the frontend's single SSE parser `JSON.parse`s every payload, so a bare
//!    string on the wire breaks every stream (masterplan.md §8.2 taxonomy).
//! 2. [`CancelOnDisconnect`] is the M2 "cancellation-on-disconnect" invention
//!    (masterplan §12 M2): when the client goes away axum drops the response
//!    stream, the wrapper's `Drop` fires the [`CancellationToken`], and every
//!    in-flight planner call / Tectonic compile bound to that token aborts.

use axum::response::sse::Event;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_util::sync::CancellationToken;

/// Build a taxonomy event with a JSON payload.
pub(crate) fn event(name: &str, data: &serde_json::Value) -> Event {
    Event::default().event(name).data(data.to_string())
}

/// Send an event, reporting whether the receiver is still listening. Callers
/// treat `false` as "client is gone, stop working".
pub(crate) async fn send(
    tx: &mpsc::Sender<Result<Event, Infallible>>,
    name: &str,
    data: &serde_json::Value,
) -> bool {
    tx.send(Ok(event(name, data))).await.is_ok()
}

/// Stream wrapper that cancels `token` when the HTTP response is dropped
/// (client disconnect, proxy timeout, tab closed).
pub(crate) struct CancelOnDisconnect<S> {
    inner: S,
    token: CancellationToken,
}

impl<S> CancelOnDisconnect<S> {
    pub(crate) fn new(inner: S, token: CancellationToken) -> Self {
        Self { inner, token }
    }
}

impl<S> Drop for CancelOnDisconnect<S> {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

impl<S: Stream + Unpin> Stream for CancelOnDisconnect<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::{StreamExt, wrappers::ReceiverStream};

    #[tokio::test]
    async fn dropping_the_stream_cancels_the_token() {
        let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(1);
        let token = CancellationToken::new();
        let stream = CancelOnDisconnect::new(ReceiverStream::new(rx), token.clone());

        assert!(!token.is_cancelled());
        drop(stream);
        assert!(token.is_cancelled());
        drop(tx);
    }

    #[tokio::test]
    async fn passes_items_through_untouched() {
        let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(4);
        let token = CancellationToken::new();
        let mut stream = CancelOnDisconnect::new(ReceiverStream::new(rx), token.clone());

        assert!(send(&tx, "progress", &serde_json::json!({"message": "hi"})).await);
        drop(tx);
        assert!(stream.next().await.is_some());
        assert!(stream.next().await.is_none());
    }
}
