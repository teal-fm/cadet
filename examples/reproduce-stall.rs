use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use async_trait::async_trait;
use rocketman::{
    connection::JetstreamConnection,
    handler::{self, Ingestors},
    ingestion::LexiconIngestor,
    options::JetstreamOptions,
    types::event::Event,
};
use serde_json::Value;
use tracing::{error, info, warn};

/// This example tests the reconnection fix for idle connections.
///
/// When listening to a custom collection that gets no traffic,
/// the server closes the connection after timeout and
/// rocketman should reconnect automatically.
///
/// Run with: cargo run --example reproduce-stall
///
/// Expected behavior:
/// 1. Initial connection
/// 2. No messages for timeout period (20s)
/// 3. "No messages received in X seconds, reconnecting" or "Stream closed by server"
/// 4. Automatic reconnection (should repeat indefinitely)
/// 5. No infinite loops or hanging

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    start_idle_test().await
}

pub async fn start_idle_test() -> Result<()> {
    info!("Testing reconnection fix with idle connection...");

    let opts = JetstreamOptions::builder()
        .wanted_collections(vec![
            // This is a custom collection that will get zero traffic
            "moe.hayden.blogi.actor.profile".to_string(),
        ])
        .timeout_time_sec(20) // Shorter timeout for faster testing
        .bound(65536)
        .build();

    let jetstream = JetstreamConnection::new(opts);

    let mut ingestors: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>> = HashMap::new();
    ingestors.insert(
        "fm.teal.alpha.feed.play".to_string(),
        Box::new(ProfileIngestor),
    );

    let cursor: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));

    let msg_rx = jetstream.get_msg_rx();
    let reconnect_tx = jetstream.get_reconnect_tx();

    let monitor_rx = msg_rx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));

        loop {
            interval.tick().await;
            let queue_len = monitor_rx.len();
            if queue_len > 0 {
                warn!("Queue has {} messages pending", queue_len);
            }
        }
    });

    // Spawn task to process messages (there should be none)
    let c_cursor = cursor.clone();
    tokio::spawn(async move {
        info!("Message processing task started");
        let mut message_count = 0u64;
        let mut last_log = std::time::Instant::now();

        let ing = Ingestors {
            commits: ingestors,
            identity: None,
            account: None,
        };

        while let Ok(message) = msg_rx.recv_async().await {
            message_count += 1;

            // Log every 10 messages to see if we're actually processing
            if message_count % 10 == 0 || last_log.elapsed() > std::time::Duration::from_secs(5) {
                info!(
                    "Processing message #{} (queue len: {}/{})",
                    message_count,
                    msg_rx.len(),
                    msg_rx.capacity().unwrap_or(0)
                );
                last_log = std::time::Instant::now();
            }

            match handler::handle_message(message, &ing, reconnect_tx.clone(), c_cursor.clone())
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!("Error processing message #{}: {}", message_count, e);
                }
            }
        }

        error!("Message processing task ended unexpectedly!");
    });

    // Add a monitoring task to track connection health
    let start_time = std::time::Instant::now();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        let mut tick_count = 0;

        loop {
            interval.tick().await;
            tick_count += 1;
            let elapsed = start_time.elapsed().as_secs();
            info!(
                "Health check #{} - uptime: {}s (expecting reconnects every ~20s)",
                tick_count, elapsed
            );
        }
    });

    info!("Connecting to jetstream (testing reconnection fix)...");
    info!("Watch for 'Stream closed by server' or 'No messages received' followed by reconnection");

    // This should connect, sit idle, get disconnected, then reconnect automatically
    // The fix should prevent infinite loops when reconnecting
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
        // If we get messages, they're probably deletions with no record
        if let Some(ref commit) = message.commit {
            if commit.record.is_some() {
                info!("got message with record: {:?}", message);
            } else {
                info!(
                    "ProfileIngestor got deletion/empty record for DID: {}",
                    message.did
                );
            }
        } else {
            info!("ProfileIngestor got message with no commit: {:?}", message);
        }
        Ok(())
    }
}
