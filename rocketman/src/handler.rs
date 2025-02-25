use anyhow::Result;
use metrics::{counter, describe_counter, Unit};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::{
    ingestion::LexiconIngestor,
    types::event::{Event, Kind},
};

pub async fn handle_message(
    message: Result<Message, Error>,
    ingestors: &HashMap<String, Box<dyn LexiconIngestor + Send + Sync>>,
    cursor: Arc<Mutex<Option<String>>>,
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

            let envelope: Event<Value> = serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("Failed to parse message: {} with json string {}", e, text)
            })?;

            counter!("jetstream.event.parse").increment(1);

            if let Some(ref time_us) = envelope.time_us {
                if let Some(cursor) = cursor.lock().unwrap().as_mut() {
                    if time_us > cursor {
                        println!("Cursor is behind, resetting");
                        *cursor = time_us.clone();
                    }
                }
            }

            match envelope.kind {
                Kind::Commit => match extract_nsid(&text) {
                    Ok((nsid, val)) => {
                        if let Some(fun) = ingestors.get(&nsid) {
                            match fun.ingest(val).await {
                                Ok(_) => counter!("jetstream.event.parse.commit").increment(1),
                                Err(e) => {
                                    println!("{}", e);
                                    counter!("jetstream.error").increment(1);
                                    counter!("jetstream.event.fail").increment(1);
                                }
                            }
                        }
                    }
                    Err(e) => println!("{}", e),
                },
                Kind::Identity => {
                    counter!("jetstream.event.parse.identity").increment(1);
                }
                Kind::Account => {
                    counter!("jetstream.event.parse.account").increment(1);
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
