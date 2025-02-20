use anyhow::Result;
use metrics::{counter, describe_counter, Unit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::ingestion::LexiconIngestor;

pub async fn handle_message(
    message: Result<Message, Error>,
    ingestors: &HashMap<String, Box<dyn LexiconIngestor + Send + Sync>>,
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
        Ok(Message::Text(text)) => {
            counter!("jetstream.event").increment(1);
            println!("Received: {}", text);

            let nsid = extract_nsid(&text).unwrap_or("unknown".to_string());
            if let Some(fun) = ingestors.get(&nsid) {
                match fun.ingest(&text) {
                    Ok(_) => counter!("jetstream.event.parse").increment(1),
                    Err(_) => {
                        counter!("jetstream.error").increment(1);
                        counter!("jetstream.event.fail").increment(1);
                    }
                }
            }
            Ok(())
        }
        Ok(Message::Binary(_)) => {
            println!("Binary message received");
            Ok(())
        }
        Ok(Message::Close(_)) => {
            println!("Server closed connection");
            Err(Error::ConnectionClosed.into())
        }
        Err(e) => {
            counter!("jetstream.error").increment(1);
            eprintln!("Error: {}", e);
            Err(e.into())
        }
        _ => Ok(()),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageEnvelope {
    collection: String,
    #[serde(flatten)]
    extra: serde_json::Value, // Catches all other fields
}

fn extract_nsid(message: &str) -> anyhow::Result<String> {
    let envelope: MessageEnvelope = serde_json::from_str(message)
        .map_err(|e| anyhow::anyhow!("Failed to parse message: {}", e))?;

    Ok(envelope.collection)
}
