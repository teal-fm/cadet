use anyhow::Result;
use metrics::{counter, describe_counter, Unit};
use serde_json::Value;
use std::collections::HashMap;
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::{ingestion::LexiconIngestor, types::event::Event};

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
            //println!("Received: {}", text);

            match extract_nsid(&text) {
                Ok((nsid, val)) => {
                    //println!("Detected nsid: {nsid}");
                    if let Some(fun) = ingestors.get(&nsid) {
                        //println!("Got ingestor");
                        match fun.ingest(val) {
                            Ok(_) => counter!("jetstream.event.parse").increment(1),
                            Err(e) => {
                                println!("{}", e);
                                counter!("jetstream.error").increment(1);
                                counter!("jetstream.event.fail").increment(1);
                            }
                        }
                    }
                }
                Err(e) => println!("{}", e),
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

fn extract_nsid(message: &str) -> anyhow::Result<(String, Event<Value>)> {
    let envelope: Event<Value> = serde_json::from_str(message).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse message: {} with json string {}",
            e,
            message
        )
    })?;

    Ok((envelope.commit.collection.clone(), envelope))
}
