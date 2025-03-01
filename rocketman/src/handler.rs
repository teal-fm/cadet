use anyhow::Result;
use flume::Sender;
use metrics::{counter, describe_counter, Unit};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio_tungstenite::tungstenite::{Error, Message};
use tracing::{debug, error};

use crate::{
    ingestion::LexiconIngestor,
    types::event::{Event, Kind},
};

pub async fn handle_message(
    message: Message,
    ingestors: &HashMap<String, Box<dyn LexiconIngestor + Send + Sync>>,
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

            debug!("envelope: {:?}", envelope);

            if let Some(ref time_us) = envelope.time_us {
                debug!("Time: {}", time_us);
                if let Some(cursor) = cursor.lock().unwrap().as_mut() {
                    debug!("Cursor: {}", cursor);
                    if time_us > cursor {
                        debug!("Cursor is behind, resetting");
                        *cursor = time_us.clone();
                    }
                }
            }

            match envelope.kind {
                Kind::Commit => match extract_nsid(&text) {
                    Ok((nsid, val)) => {
                        if let Some(fun) = ingestors.get(&nsid) {
                            match fun.ingest(val).await {
                                Ok(_) => counter!("jetstream.event.parse.commit", "nsid" => nsid)
                                    .increment(1),
                                Err(e) => {
                                    error!("Error parsing commit: {}", e);
                                    counter!("jetstream.error").increment(1);
                                    counter!("jetstream.event.fail").increment(1);
                                }
                            }
                        }
                    }
                    Err(e) => error!("Error parsing commit: {}", e),
                },
                Kind::Identity => {
                    counter!("jetstream.event.parse.identity").increment(1);
                }
                Kind::Account => {
                    counter!("jetstream.event.parse.account").increment(1);
                }
                Kind::Unknown(kind) => {
                    counter!("jetstream.event.parse.unknown", "kind" => kind).increment(1);
                }
            }
            Ok(())
        }
        Message::Binary(_) => {
            debug!("Binary message received");
            Ok(())
        }
        Message::Close(_) => {
            debug!("Server closed connection");
            if let Err(e) = reconnect_tx.send(()) {
                counter!("jetstream.event.parse.error", "error" => "failed_to_send_reconnect_signal").increment(1);
                error!("Failed to send reconnect signal: {}", e);
            }
            Err(Error::ConnectionClosed.into())
        }
        _ => Ok(()),
    }
}

fn extract_nsid(message: &str) -> anyhow::Result<(String, Event<Value>)> {
    let envelope: Event<Value> = serde_json::from_str(message).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse message: {} with json string {}",
            e,
            message
        )
    })?;

    // if the type is not a commit
    if envelope.kind != Kind::Commit || envelope.commit.is_none() {
        return Err(anyhow::anyhow!(
            "Message is not a commit, so there is no nsid attached."
        ));
    } else if let Some(ref commit) = envelope.commit {
        return Ok((commit.collection.clone(), envelope));
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
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };
    use tokio_tungstenite::tungstenite::Message;

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

        let mut ingestors: HashMap<
            String,
            Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>,
        > = HashMap::new();
        ingestors.insert(
            "ns1".to_string(),
            Box::new(DummyIngestor {
                called: called_flag.clone(),
            }),
        );

        let result = handle_message(
            Message::Text(event_json),
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

        let mut ingestors: HashMap<
            String,
            Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>,
        > = HashMap::new();
        ingestors.insert("ns_error".to_string(), Box::new(ErrorIngestor));

        // Even though ingestion fails, handle_message returns Ok(()).
        let result = handle_message(
            Message::Text(event_json),
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
        let ingestors: HashMap<String, Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>> =
            HashMap::new();

        let result =
            handle_message(Message::Text(event_json), &ingestors, reconnect_tx, cursor).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_binary_message() {
        let (reconnect_tx, _reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(None));
        let ingestors: HashMap<String, Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>> =
            HashMap::new();

        let result = handle_message(
            Message::Binary(vec![1, 2, 3]),
            &ingestors,
            reconnect_tx,
            cursor,
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_close_message() {
        let (reconnect_tx, reconnect_rx) = setup_reconnect_channel();
        let cursor = Arc::new(Mutex::new(None));
        let ingestors: HashMap<String, Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>> =
            HashMap::new();

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
        let ingestors: HashMap<String, Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>> =
            HashMap::new();

        let invalid_json = "this is not json".to_string();
        let result = handle_message(
            Message::Text(invalid_json),
            &ingestors,
            reconnect_tx,
            cursor,
        )
        .await;
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
        let mut ingestors: HashMap<
            String,
            Box<dyn crate::ingestion::LexiconIngestor + Send + Sync>,
        > = HashMap::new();
        ingestors.insert(
            "ns1".to_string(),
            Box::new(DummyIngestor {
                called: Arc::new(Mutex::new(false)),
            }),
        );

        let result = handle_message(
            Message::Text(event_json),
            &ingestors,
            reconnect_tx,
            cursor.clone(),
        )
        .await;
        assert!(result.is_ok());
        // Cursor should remain unchanged.
        assert_eq!(*cursor.lock().unwrap(), Some(300));
    }
}
