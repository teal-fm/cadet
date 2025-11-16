use anyhow::Result;
use flume::Sender;
use metrics::{counter, describe_counter, Unit};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio_tungstenite::tungstenite::{Error, Message};
use tracing::{error, warn};

#[cfg(feature = "zstd")]
use std::io::Cursor as IoCursor;
#[cfg(feature = "zstd")]
use std::sync::LazyLock;
#[cfg(feature = "zstd")]
use zstd::dict::DecoderDictionary;

use crate::{
    ingestion::LexiconIngestor,
    types::event::{Event, Kind},
};

/// Container for different types of event ingestors.
pub struct Ingestors {
    /// Ingestors for commit events, keyed by collection/NSID (e.g., "app.bsky.feed.post")
    pub commits: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>>,
    /// Optional ingestor for identity events
    pub identity: Option<Box<dyn LexiconIngestor + Send + Sync>>,
    /// Optional ingestor for account events
    pub account: Option<Box<dyn LexiconIngestor + Send + Sync>>,
}

impl Ingestors {
    pub fn new() -> Self {
        Self {
            commits: HashMap::new(),
            identity: None,
            account: None,
        }
    }
}

impl Default for Ingestors {
    fn default() -> Self {
        Self::new()
    }
}

/// The custom `zstd` dictionary used for decoding compressed Jetstream messages.
///
/// Sourced from the [official Bluesky Jetstream repo.](https://github.com/bluesky-social/jetstream/tree/main/pkg/models)
#[cfg(feature = "zstd")]
static ZSTD_DICTIONARY: LazyLock<DecoderDictionary> =
    LazyLock::new(|| DecoderDictionary::copy(include_bytes!("../zstd/dictionary")));

pub async fn handle_message(
    message: Message,
    ingestors: &Ingestors,
    reconnect_tx: Sender<()>,
    cursor: Arc<Mutex<Option<u64>>>,
) -> Result<()> {
    describe_counter!(
        "jetstream.event",
        Unit::Count,
        "number of event ingest attempts"
    );
    describe_counter!(
        "jetstream.event.parse",
        Unit::Count,
        "events that were successfully processed"
    );
    describe_counter!(
        "jetstream.event.fail",
        Unit::Count,
        "events that could not be read"
    );
    describe_counter!("jetstream.error", Unit::Count, "errors encountered");

    match message {
        Message::Text(text) => {
            counter!("jetstream.event").increment(1);
            let envelope: Event<Value> = serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("Failed to parse message: {} with json string {}", e, text)
            })?;

            handle_envelope(envelope, cursor, ingestors).await?;
            Ok(())
        }
        #[cfg(feature = "zstd")]
        Message::Binary(bytes) => {
            counter!("jetstream.event").increment(1);
            let decoder = zstd::stream::Decoder::with_prepared_dictionary(
                IoCursor::new(bytes),
                &*ZSTD_DICTIONARY,
            )?;
            let envelope: Event<Value> = serde_json::from_reader(decoder)
                .map_err(|e| anyhow::anyhow!("Failed to parse binary message: {}", e))?;

            handle_envelope(envelope, cursor, ingestors).await?;
            Ok(())
        }
        #[cfg(not(feature = "zstd"))]
        Message::Binary(_) => Err(anyhow::anyhow!(
            "binary message received but zstd feature is not enabled"
        )),
        Message::Close(_) => {
            if let Err(e) = reconnect_tx.send(()) {
                counter!("jetstream.event.parse.error", "error" => "failed_to_send_reconnect_signal").increment(1);
                error!("Failed to send reconnect signal: {}", e);
            }
            Err(Error::ConnectionClosed.into())
        }
        _ => Ok(()),
    }
}

async fn handle_envelope(
    envelope: Event<Value>,
    cursor: Arc<Mutex<Option<u64>>>,
    ingestors: &Ingestors,
) -> Result<()> {
    if let Some(ref time_us) = envelope.time_us {
        if let Some(cursor) = cursor.lock().unwrap().as_mut() {
            if time_us > cursor {
                *cursor = *time_us;
            }
        }
    }

    match envelope.kind {
        Kind::Commit => match extract_commit_nsid(&envelope) {
            Ok(nsid) => {
                if let Some(fun) = ingestors.commits.get(&nsid) {
                    let ingest_start = Instant::now();

                    match fun.ingest(envelope).await {
                        Ok(_) => {
                            let ingest_duration = ingest_start.elapsed();

                            if ingest_duration.as_secs() >= 5 {
                                warn!(
                                    "Slow ingestor: {} took {}s",
                                    nsid,
                                    ingest_duration.as_secs()
                                );
                            }

                            counter!("jetstream.event.parse.commit", "nsid" => nsid).increment(1);
                        }
                        Err(e) => {
                            error!("Error ingesting commit with nsid {}: {}", nsid, e);
                            counter!("jetstream.error").increment(1);
                            counter!("jetstream.event.fail").increment(1);
                        }
                    }
                }
            }
            Err(e) => error!("Error parsing commit: {}", e),
        },
        Kind::Identity => {
            if let Some(ref ingestor) = ingestors.identity {
                let ingest_start = Instant::now();

                match ingestor.ingest(envelope).await {
                    Ok(_) => {
                        let ingest_duration = ingest_start.elapsed();

                        if ingest_duration.as_secs() >= 5 {
                            warn!(
                                "Slow ingestor: identity took {}s",
                                ingest_duration.as_secs()
                            );
                        }

                        counter!("jetstream.event.parse.identity").increment(1);
                    }
                    Err(e) => {
                        error!("Error ingesting identity event: {}", e);
                        counter!("jetstream.error").increment(1);
                        counter!("jetstream.event.fail").increment(1);
                    }
                }
            } else {
                counter!("jetstream.event.parse.identity").increment(1);
            }
        }
        Kind::Account => {
            if let Some(ref ingestor) = ingestors.account {
                let ingest_start = Instant::now();

                match ingestor.ingest(envelope).await {
                    Ok(_) => {
                        let ingest_duration = ingest_start.elapsed();

                        if ingest_duration.as_secs() >= 5 {
                            warn!("Slow ingestor: account took {}s", ingest_duration.as_secs());
                        }

                        counter!("jetstream.event.parse.account").increment(1);
                    }
                    Err(e) => {
                        error!("Error ingesting account event: {}", e);
                        counter!("jetstream.error").increment(1);
                        counter!("jetstream.event.fail").increment(1);
                    }
                }
            } else {
                counter!("jetstream.event.parse.account").increment(1);
            }
        }
        Kind::Unknown(kind) => {
            counter!("jetstream.event.parse.unknown", "kind" => kind).increment(1);
        }
    }
    Ok(())
}

fn extract_commit_nsid(envelope: &Event<Value>) -> anyhow::Result<String> {
    // if the type is not a commit
    if envelope.commit.is_none() {
        return Err(anyhow::anyhow!(
            "Message has no commit, so there is no nsid attached."
        ));
    } else if let Some(ref commit) = envelope.commit {
        return Ok(commit.collection.clone());
    }

    Err(anyhow::anyhow!("Failed to extract nsid: unknown error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::event::Event;
    use anyhow::Result;
    use async_trait::async_trait;
    use flume::{Receiver, Sender};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

    // Dummy ingestor that records if it was called.
    struct DummyIngestor {
        pub called: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl crate::ingestion::LexiconIngestor for DummyIngestor {
        async fn ingest(&self, _event: Event<serde_json::Value>) -> Result<(), anyhow::Error> {
            let mut called = self.called.lock().unwrap();
            *called = true;
            Ok(())
        }
    }

    // Dummy ingestor that always returns an error.
    struct ErrorIngestor;

    #[async_trait]
    impl crate::ingestion::LexiconIngestor for ErrorIngestor {
        async fn ingest(&self, _event: Event<serde_json::Value>) -> Result<(), anyhow::Error> {
            Err(anyhow::anyhow!("Ingest error"))
        }
    }

    // Helper to create a reconnect channel.
    fn setup_reconnect_channel() -> (Sender<()>, Receiver<()>) {
        flume::unbounded()
    }

    #[tokio::test]
    async fn test_valid_commit_success() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));
        let called_flag = Arc::new(Mutex::new(false));

        // Create a valid commit event JSON.
        let event_json = json!({
            "did": "did:example:123",
            "time_us": 200,
            "kind": "commit",
            "commit": {
                "rev": "1",
                "operation": "create",
                "collection": "ns1",
                "rkey": "rkey1",
                "record": { "foo": "bar" },
                "cid": "cid123"
            },
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors.commits.insert(
            "ns1".to_string(),
            Box::new(DummyIngestor {
                called: called_flag.clone(),
            }),
        );

        let utf8bytes = Utf8Bytes::from(event_json);

        let result = handle_message(
            Message::Text(utf8bytes),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        // Check that the ingestor was called.
        assert!(*called_flag.lock().unwrap());
        // Verify that the cursor got updated.
        assert_eq!(*cursor.lock().unwrap(), Some(200));
    }

    #[cfg(feature = "zstd")]
    #[tokio::test]
    async fn test_binary_valid_commit() {
        use tokio_tungstenite::tungstenite::Bytes;

        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));
        let called_flag = Arc::new(Mutex::new(false));

        let uncompressed_json = json!({
            "did": "did:example:123",
            "time_us": 200,
            "kind": "commit",
            "commit": {
                "rev": "1",
                "operation": "create",
                "collection": "ns1",
                "rkey": "rkey1",
                "record": { "foo": "bar" },
                "cid": "cid123"
            },
        })
        .to_string();

        let compressed_dest: IoCursor<Vec<u8>> = IoCursor::new(vec![]);
        let mut encoder = zstd::Encoder::with_prepared_dictionary(
            compressed_dest,
            &zstd::dict::EncoderDictionary::copy(include_bytes!("../zstd/dictionary"), 0),
        )
        .unwrap();
        std::io::copy(
            &mut IoCursor::new(uncompressed_json.as_bytes()),
            &mut encoder,
        )
        .unwrap();
        let compressed_dest = encoder.finish().unwrap();

        let mut ingestors = Ingestors::new();
        ingestors.commits.insert(
            "ns1".to_string(),
            Box::new(DummyIngestor {
                called: called_flag.clone(),
            }),
        );

        let binary = Bytes::from(compressed_dest.into_inner());

        let result = handle_message(
            Message::Binary(binary),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;

        assert!(result.is_ok());
        // Check that the ingestor was called.
        assert!(*called_flag.lock().unwrap());
        // Verify that the cursor got updated.
        assert_eq!(*cursor.lock().unwrap(), Some(200));
    }

    #[tokio::test]
    async fn test_commit_ingest_failure() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));

        // Valid commit event with an ingestor that fails.
        let event_json = json!({
            "did": "did:example:123",
            "time_us": 300,
            "kind": "commit",
            "commit": {
                "rev": "1",
                "operation": "create",
                "collection": "ns_error",
                "rkey": "rkey1",
                "record": { "foo": "bar" },
                "cid": "cid123"
            },
            "identity": null
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors
            .commits
            .insert("ns_error".to_string(), Box::new(ErrorIngestor));

        // Even though ingestion fails, handle_message returns Ok(()).
        let utf8 = Utf8Bytes::from(event_json);
        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        // Cursor should still update because it comes before the ingest call.
        assert_eq!(*cursor.lock().unwrap(), Some(300));
    }

    #[tokio::test]
    async fn test_identity_message() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(None));
        // Valid identity event.
        let event_json = json!({
            "did": "did:example:123",
            "time_us": 150,
            "kind": "identity",
            "commit": null,
            "identity": {
                "did": "did:example:123",
                "handle": "user",
                "seq": 1,
                "time": "2025-01-01T00:00:00Z"
            }
        })
        .to_string();
        let ingestors = Ingestors::new();

        let utf8 = Utf8Bytes::from(event_json);

        let result = handle_message(Message::Text(utf8), &ingestors, reconnect_tx, cursor).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_close_message() {
        let (reconnect_tx, reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(None));
        let ingestors = Ingestors::new();

        let result = handle_message(Message::Close(None), &ingestors, reconnect_tx, cursor).await;
        // Should return an error due to connection close.
        assert!(result.is_err());
        // Verify that a reconnect signal was sent.
        let signal = reconnect_rx.recv_async().await;
        assert!(signal.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_json() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(None));
        let ingestors = Ingestors::new();

        let invalid_json = "this is not json".to_string();
        let utf8 = Utf8Bytes::from(invalid_json);
        let result = handle_message(Message::Text(utf8), &ingestors, reconnect_tx, cursor).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cursor_not_updated_if_lower() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        // Set an initial cursor value.
        let cursor = Arc::new(Mutex::new(Some(300)));
        let event_json = json!({
            "did": "did:example:123",
            "time_us": 200,
            "kind": "commit",
            "commit": {
                "rev": "1",
                "operation": "create",
                "collection": "ns1",
                "rkey": "rkey1",
                "record": { "foo": "bar" },
                "cid": "cid123"
            },
            "identity": null
        })
        .to_string();

        // Use a dummy ingestor that does nothing.
        let mut ingestors = Ingestors::new();
        ingestors.commits.insert(
            "ns1".to_string(),
            Box::new(DummyIngestor {
                called: Arc::new(Mutex::new(false)),
            }),
        );
        let utf8 = Utf8Bytes::from(event_json);
        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        // Cursor should remain unchanged.
        assert_eq!(*cursor.lock().unwrap(), Some(300));
    }

    #[tokio::test]
    async fn test_identity_with_ingestor() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));
        let called_flag = Arc::new(Mutex::new(false));

        let event_json = json!({
            "did": "did:example:123",
            "time_us": 200,
            "kind": "identity",
            "commit": null,
            "identity": {
                "did": "did:example:123",
                "handle": "user.bsky.social",
                "seq": 42,
                "time": "2025-01-01T00:00:00Z"
            }
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors.identity = Some(Box::new(DummyIngestor {
            called: called_flag.clone(),
        }));

        let utf8 = Utf8Bytes::from(event_json);

        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        assert!(*called_flag.lock().unwrap());
        assert_eq!(*cursor.lock().unwrap(), Some(200));
    }

    #[tokio::test]
    async fn test_identity_ingest_failure() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));

        let event_json = json!({
            "did": "did:example:123",
            "time_us": 300,
            "kind": "identity",
            "commit": null,
            "identity": {
                "did": "did:example:123",
                "handle": "user.bsky.social",
                "seq": 42,
                "time": "2025-01-01T00:00:00Z"
            }
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors.identity = Some(Box::new(ErrorIngestor));

        let utf8 = Utf8Bytes::from(event_json);
        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(*cursor.lock().unwrap(), Some(300));
    }

    #[tokio::test]
    async fn test_account_with_ingestor() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));
        let called_flag = Arc::new(Mutex::new(false));

        let event_json = json!({
            "did": "did:example:123",
            "time_us": 200,
            "kind": "account",
            "commit": null,
            "account": {
                "active": true,
                "did": "did:example:123",
                "seq": 42,
                "time": "2025-01-01T00:00:00Z"
            }
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors.account = Some(Box::new(DummyIngestor {
            called: called_flag.clone(),
        }));

        let utf8 = Utf8Bytes::from(event_json);

        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        assert!(*called_flag.lock().unwrap());
        assert_eq!(*cursor.lock().unwrap(), Some(200));
    }

    #[tokio::test]
    async fn test_account_ingest_failure() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(Some(100)));

        let event_json = json!({
            "did": "did:example:123",
            "time_us": 300,
            "kind": "account",
            "commit": null,
            "account": {
                "active": false,
                "did": "did:example:123",
                "seq": 42,
                "time": "2025-01-01T00:00:00Z"
            }
        })
        .to_string();

        let mut ingestors = Ingestors::new();
        ingestors.account = Some(Box::new(ErrorIngestor));

        let utf8 = Utf8Bytes::from(event_json);
        let result = handle_message(
            Message::Text(utf8),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(*cursor.lock().unwrap(), Some(300));
    }
}
