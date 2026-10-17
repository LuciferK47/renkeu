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

/// Every event an agent stream may emit: the locked §8.2 taxonomy as a type.
///
/// Building events through this enum (instead of passing a name string and a
/// JSON blob) means an off-taxonomy name cannot be written, and the one place
/// that knows the payload shapes is here.
#[derive(Debug)]
pub(crate) enum SseEvent {
    Progress(serde_json::Value),
    Thinking { message: String, iteration: u32 },
    /// An intermediate attempt in a verify-first run: informational only,
    /// not something the client may apply.
    Step(serde_json::Value),
    Proposal(serde_json::Value),
    Applying { message: String },
    Compiled(serde_json::Value),
    AwaitingApproval(serde_json::Value),
    Error { message: String },
    Done(serde_json::Value),
}

impl SseEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::Progress(_) => "progress",
            Self::Thinking { .. } => "thinking",
            Self::Step(_) => "step",
            Self::Proposal(_) => "proposal",
            Self::Applying { .. } => "applying",
            Self::Compiled(_) => "compiled",
            Self::AwaitingApproval(_) => "awaiting_approval",
            Self::Error { .. } => "error",
            Self::Done(_) => "done",
        }
    }

    fn payload(&self) -> serde_json::Value {
        match self {
            Self::Progress(v)
            | Self::Step(v)
            | Self::Proposal(v)
            | Self::Compiled(v)
            | Self::AwaitingApproval(v)
            | Self::Done(v) => v.clone(),
            Self::Thinking { message, iteration } => {
                serde_json::json!({ "message": message, "iteration": iteration })
            }
            Self::Applying { message } | Self::Error { message } => {
                serde_json::json!({ "message": message })
            }
        }
    }
}

/// The sending half of an agent stream. It guarantees what clients rely on:
/// **exactly one `done`, and nothing after it.** A run that ends any other
/// way — an early return, a panic — still closes with `error` then `done`
/// when the sink is dropped.
pub(crate) struct EventSink {
    tx: mpsc::Sender<Result<Event, Infallible>>,
    finished: bool,
}

impl EventSink {
    pub(crate) fn new(tx: mpsc::Sender<Result<Event, Infallible>>) -> Self {
        Self { tx, finished: false }
    }

    /// Send an event. `false` means the client is gone; stop working. After
    /// `done` further events are dropped.
    pub(crate) async fn emit(&mut self, event: SseEvent) -> bool {
        if self.finished {
            return false;
        }
        if matches!(event, SseEvent::Done(_)) {
            self.finished = true;
        }
        self.tx
            .send(Ok(self::event(event.name(), &event.payload())))
            .await
            .is_ok()
    }

    /// End the run unsuccessfully: `error`, then the one `done`.
    pub(crate) async fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        let _ = self.emit(SseEvent::Error { message: message.clone() }).await;
        let _ = self
            .emit(SseEvent::Done(serde_json::json!({ "success": false, "message": message })))
            .await;
    }
}

impl Drop for EventSink {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // The run ended without saying so (a panic, a forgotten path). The
        // channel is not awaited here, so use the non-blocking send; a full
        // or closed channel just means nobody is listening.
        let message = "the run ended unexpectedly";
        for e in [
            SseEvent::Error { message: message.to_owned() },
            SseEvent::Done(serde_json::json!({ "success": false, "message": message })),
        ] {
            let _ = self.tx.try_send(Ok(self::event(e.name(), &e.payload())));
        }
    }
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

    async fn collect(rx: mpsc::Receiver<Result<Event, Infallible>>) -> String {
        let mut stream = ReceiverStream::new(rx);
        let mut out = String::new();
        while let Some(Ok(e)) = stream.next().await {
            out.push_str(&format!("{e:?}\n"));
        }
        out
    }

    #[tokio::test]
    async fn done_is_sent_once_and_nothing_follows_it() {
        let (tx, rx) = mpsc::channel(16);
        let mut sink = EventSink::new(tx);
        assert!(sink.emit(SseEvent::Thinking { message: "m".into(), iteration: 1 }).await);
        assert!(sink.emit(SseEvent::Done(serde_json::json!({"success": true}))).await);
        assert!(!sink.emit(SseEvent::Done(serde_json::json!({"success": false}))).await);
        assert!(!sink.emit(SseEvent::Error { message: "late".into() }).await);
        drop(sink);

        let text = collect(rx).await;
        assert_eq!(text.matches("event: done").count(), 1, "{text}");
        assert!(!text.contains("late"), "{text}");
    }

    #[tokio::test]
    async fn fail_is_error_then_one_done() {
        let (tx, rx) = mpsc::channel(16);
        let mut sink = EventSink::new(tx);
        sink.fail("boom").await;
        sink.fail("again").await; // a second failure adds nothing
        drop(sink);

        let text = collect(rx).await;
        let error = text.find("event: error").unwrap();
        let done = text.find("event: done").unwrap();
        assert!(error < done, "{text}");
        assert_eq!(text.matches("event: done").count(), 1, "{text}");
        assert!(!text.contains("again"), "{text}");
    }

    #[tokio::test]
    async fn a_sink_dropped_without_done_closes_the_stream_properly() {
        let (tx, rx) = mpsc::channel(16);
        let mut sink = EventSink::new(tx);
        assert!(sink.emit(SseEvent::Thinking { message: "m".into(), iteration: 1 }).await);
        drop(sink); // as after a panic

        let text = collect(rx).await;
        assert_eq!(text.matches("event: done").count(), 1, "{text}");
        assert!(text.contains("event: error"), "{text}");
    }

    #[test]
    fn every_event_has_a_taxonomy_name() {
        let names = [
            SseEvent::Progress(serde_json::json!({})).name(),
            SseEvent::Thinking { message: String::new(), iteration: 1 }.name(),
            SseEvent::Step(serde_json::json!({})).name(),
            SseEvent::Proposal(serde_json::json!({})).name(),
            SseEvent::Applying { message: String::new() }.name(),
            SseEvent::Compiled(serde_json::json!({})).name(),
            SseEvent::AwaitingApproval(serde_json::json!({})).name(),
            SseEvent::Error { message: String::new() }.name(),
            SseEvent::Done(serde_json::json!({})).name(),
        ];
        let allowed = ["progress", "step", "proposal", "applying", "compiled", "thinking", "done", "error", "awaiting_approval", "keepalive"];
        assert!(names.iter().all(|n| allowed.contains(n)), "{names:?}");
    }
}
