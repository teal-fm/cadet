use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use rocketman::{
    connection::JetstreamConnection, handler, ingestion::LexiconIngestor,
    options::JetstreamOptions, types::event::Event,
};
use serde_json::Value;
use tracing::info;

/// This example reproduces the idle connection issue.
///
/// When listening to a custom collection that gets no traffic,
/// the server closes the connection after ~2 minutes and
/// rocketman should reconnect automatically.
///
/// Run with: cargo run --example reproduce-stall
///
/// You should see:
/// 1. Initial connection
/// 2. No messages for ~2 minutes
/// 3. "Stream closed by server" message
/// 4. Automatic reconnection

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    start_idle_test().await
}

pub async fn start_idle_test() -> Result<()> {
    info!("Testing idle connection with custom collection...");

    let opts = JetstreamOptions::builder()
        .wanted_collections(vec![
            // This is a custom collection that will get zero traffic
            "moe.hayden.blogi.actor.profile".to_string(),
        ])
        .timeout_time_sec(40) // Default timeout
        .build();

    let jetstream = JetstreamConnection::new(opts);

    let mut ingestors: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>> = HashMap::new();
    ingestors.insert(
        "moe.hayden.blogi.actor.profile".to_string(),
        Box::new(ProfileIngestor),
    );

    let cursor: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));

    let msg_rx = jetstream.get_msg_rx();
    let reconnect_tx = jetstream.get_reconnect_tx();

    // Spawn task to process messages (there should be none)
    let c_cursor = cursor.clone();
    tokio::spawn(async move {
        info!("Message processing task started");
        let mut message_count = 0;

        while let Ok(message) = msg_rx.recv_async().await {
            message_count += 1;
            info!("Received message #{}: this is unexpected!", message_count);

            if let Err(e) =
                handler::handle_message(message, &ingestors, reconnect_tx.clone(), c_cursor.clone())
                    .await
            {
                eprintln!("Error processing message: {}", e);
            };
        }

        info!("Message processing task ended");
    });

    // Add a monitoring task to show we're alive
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut tick_count = 0;

        loop {
            interval.tick().await;
            tick_count += 1;
            info!(
                "Still alive... tick #{} (expecting no messages)",
                tick_count
            );

            if tick_count == 4 {
                info!("2 minutes elapsed - server should close connection soon");
            }
        }
    });

    info!("Connecting to jetstream (expecting idle connection)...");

    // This should connect, sit idle, get disconnected, then reconnect
    jetstream
        .connect(cursor.clone())
        .await
        .map_err(|e| anyhow::anyhow!("error running ingest: {}", e))
}

/// Simple ingestor that should never be called
pub struct ProfileIngestor;

#[async_trait]
impl LexiconIngestor for ProfileIngestor {
    async fn ingest(&self, message: Event<Value>) -> Result<()> {
        info!("ProfileIngestor received message: {:?}", message);
        info!("This should not happen with a custom collection!");
        Ok(())
    }
}
