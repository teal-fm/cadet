// handler.rs
use anyhow::Result;
use metrics::{counter, describe_counter, Unit};
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::ingestion::LexiconIngestor;

pub async fn handle_message(
    message: Result<Message, Error>,
    ingestors: &Vec<Box<dyn LexiconIngestor + Send + Sync>>,
) -> Result<()> {
    describe_counter!("jetstream.read", Unit::Count, "events read");
    describe_counter!(
        "jetstream.read.fail",
        Unit::Count,
        "events that could not be read"
    );
    describe_counter!("jetstream.error", Unit::Count, "errors encountered");
    match message {
        Ok(Message::Text(text)) => {
            counter!("jetstream.event").increment(1);
            println!("Received: {}", text);

            // Example: process message with registered lexicon ingestors.

            for ingestor in ingestors {
                ingestor.ingest(&text)?;
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
