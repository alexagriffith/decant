//! The SSE change-stream (spec §7): a `tokio::sync::broadcast` channel of small
//! [`ChangeEvent`]s and the `GET /api/v1/events` handler that fans them out to
//! connected clients (Phoenix).
//!
//! Events are deliberately tiny — a type, a timestamp, and a count — never
//! session rows. On receipt a client re-fetches the affected slice via the read
//! API. The ingest task holds a [`ChangeSender`] and emits one event after each
//! sync that ingested new data; the handler holds a [`broadcast::Receiver`] per
//! connection.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::http::AppState;

/// SSE event name for a successful sync that ingested new data. Clients listen
/// for this and re-fetch.
pub const ARCHIVE_UPDATED: &str = "archive_updated";

/// SSE event name asking the client to re-fetch from scratch because it fell
/// behind the broadcast buffer (a `Lagged` receiver) and may have missed events.
pub const RESYNC: &str = "resync";

/// Capacity of the change-event broadcast buffer. Small: events are coalesced
/// (one per sync) and clients only need the latest to know to re-fetch, so a
/// slow consumer that lags merely triggers a single `resync` rather than a
/// replay of every missed event.
const CHANNEL_CAPACITY: usize = 64;

/// Keep-alive ping interval. Idle connections (no sync for a while) stay open
/// through proxies and let the client notice a dead socket promptly.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// A compact archive-change notification. Serialized as the JSON `data` of an
/// `archive_updated` SSE event. Intentionally carries no session rows — just
/// enough for a client to decide to re-fetch (spec §7).
#[derive(Debug, Clone, Serialize)]
pub struct ChangeEvent {
    /// Discriminator; currently always `"archive_updated"`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// When the emitting sync completed, RFC3339 UTC.
    pub last_sync_at: String,
    /// Number of sessions ingested by that sync (always > 0; no-op syncs don't
    /// emit).
    pub ingested: usize,
}

impl ChangeEvent {
    /// Build an `archive_updated` event for a sync that ingested `ingested`
    /// sessions, completing at `last_sync_at` (RFC3339 UTC).
    pub fn archive_updated(ingested: usize, last_sync_at: impl Into<String>) -> Self {
        ChangeEvent {
            kind: ARCHIVE_UPDATED,
            last_sync_at: last_sync_at.into(),
            ingested,
        }
    }
}

/// Cloneable sender side of the change-event channel. The ingest task holds one;
/// each `Sender::send` reaches every live SSE subscriber.
pub type ChangeSender = broadcast::Sender<ChangeEvent>;

/// Create the change-event broadcast channel. Returns the [`ChangeSender`]
/// stored in `AppState` and cloned into the ingest task. The matching receiver
/// is created per-connection in the SSE handler via [`ChangeSender::subscribe`].
pub fn channel() -> ChangeSender {
    let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
    tx
}

/// Emit a change event, tolerating zero subscribers.
///
/// `broadcast::Sender::send` returns `Err` only when there are no receivers; for
/// a change-stream that is an ordinary idle state (no Phoenix connected), so we
/// drop the error rather than logging noise or panicking.
pub fn emit(tx: &ChangeSender, event: ChangeEvent) {
    let _ = tx.send(event);
}

/// `GET /api/v1/events` — Server-Sent Events change-stream.
///
/// Mounted behind the auth/Host/Origin guard (Phoenix sends the bearer token as
/// a normal `Authorization` header), so by the time we're here the request is
/// authorized. We subscribe a fresh receiver and forward each [`ChangeEvent`] as
/// an `archive_updated` SSE frame. A `Lagged` receiver (the client fell behind
/// the buffer) yields a single `resync` event telling it to re-fetch; a `Closed`
/// channel (daemon shutting down) ends the stream cleanly. A keep-alive ping
/// every ~15s holds idle connections open.
pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(change) => Some(to_sse_event(&change)),
        // The receiver lagged: it missed `n` events because the buffer wrapped.
        // Collapse that into one `resync` nudge — the client re-fetches the full
        // current state, so the exact missed events don't matter.
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
            Some(Ok(Event::default()
                .event(RESYNC)
                .data(r#"{"type":"resync"}"#)))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEPALIVE_INTERVAL))
}

/// Render a [`ChangeEvent`] as an SSE `Event` with the `archive_updated` name
/// and JSON data. Serialization of this tiny, fixed struct cannot fail; if it
/// somehow did we fall back to an empty object so the stream never breaks.
fn to_sse_event(change: &ChangeEvent) -> Result<Event, Infallible> {
    let data = serde_json::to_string(change).unwrap_or_else(|_| "{}".to_string());
    Ok(Event::default().event(ARCHIVE_UPDATED).data(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_event_serializes_with_type_key() {
        let ev = ChangeEvent::archive_updated(3, "2026-06-07T00:00:00Z");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "archive_updated");
        assert_eq!(v["ingested"], 3);
        assert_eq!(v["last_sync_at"], "2026-06-07T00:00:00Z");
    }

    #[test]
    fn emit_without_subscribers_is_ok() {
        // No receiver is kept alive, so `send` returns Err(NoSubscribers); `emit`
        // must swallow it rather than panic.
        let tx = channel();
        emit(&tx, ChangeEvent::archive_updated(1, "2026-06-07T00:00:00Z"));
    }

    #[tokio::test]
    async fn subscriber_receives_emitted_event() {
        let tx = channel();
        let mut rx = tx.subscribe();
        emit(&tx, ChangeEvent::archive_updated(2, "2026-06-07T00:00:00Z"));
        let got = rx.recv().await.unwrap();
        assert_eq!(got.kind, ARCHIVE_UPDATED);
        assert_eq!(got.ingested, 2);
    }
}
