use async_trait::async_trait;
use rocketman::{
    connection::JetstreamConnection,
    handler::{self, Ingestors},
    ingestion::LexiconIngestor,
    options::JetstreamOptions,
    types::event::{Account, Commit, Event, Identity},
};
use serde_json::Value;
use std::{sync::Arc, sync::Mutex};

#[tokio::main]
async fn main() {
    // set up logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // init the builder with wanted collections
    let opts = JetstreamOptions::builder()
        .wanted_collections(vec![
            "app.bsky.feed.post".to_string(),
            "xyz.statusphere.status".to_string(),
        ])
        .build();

    // create the jetstream connector
    let jetstream = JetstreamConnection::new(opts);

    // create your ingestors
    let mut ingestors = Ingestors::new();

    // register commit ingestors
    ingestors.commits.insert(
        "xyz.statusphere.status".to_string(),
        Box::new(StatusphereIngestor),
    );

    // register identity ingestor
    ingestors.identity = Some(Box::new(IdentityIngestor));

    // register account ingestor
    ingestors.account = Some(Box::new(AccountIngestor));

    // tracks the last message we've processed
    let cursor: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));

    // get channels
    let msg_rx = jetstream.get_msg_rx();
    let reconnect_tx = jetstream.get_reconnect_tx();

    // spawn a task to process messages from the queue.
    let c_cursor = cursor.clone();
    tokio::spawn(async move {
        while let Ok(message) = msg_rx.recv_async().await {
            if let Err(e) =
                handler::handle_message(message, &ingestors, reconnect_tx.clone(), c_cursor.clone())
                    .await
            {
                eprintln!("Error processing message: {}", e);
            };
        }
    });

    // connect to jetstream
    if let Err(e) = jetstream.connect(cursor.clone()).await {
        eprintln!("Failed to connect to Jetstream: {}", e);
        std::process::exit(1);
    }
}

/// Handles Statusphere status updates
pub struct StatusphereIngestor;

#[async_trait]
impl LexiconIngestor for StatusphereIngestor {
    async fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(Commit {
            record: Some(record),
            operation,
            ..
        }) = message.commit
        {
            if let Some(Value::String(status)) = record.get("status") {
                println!("[STATUSPHERE] [{operation:?}] {status:?}");
            }
        }
        Ok(())
    }
}

/// Handles identity events (handle changes, etc.)
pub struct IdentityIngestor;

#[async_trait]
impl LexiconIngestor for IdentityIngestor {
    async fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(Identity {
            did,
            handle,
            seq,
            time,
        }) = message.identity
        {
            println!("[IDENTITY] seq={seq} did={did} handle={handle:?} time={time}");
        }
        Ok(())
    }
}

/// Handles account events (active/inactive status)
pub struct AccountIngestor;

#[async_trait]
impl LexiconIngestor for AccountIngestor {
    async fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(Account {
            did,
            handle,
            seq,
            time,
            status,
        }) = message.account
        {
            println!("[ACCOUNT] seq={seq} did={did}{handle_str}{status_str} time={time}");
        }
        Ok(())
    }
}
