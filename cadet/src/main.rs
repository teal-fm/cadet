use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cursor::load_cursor;
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::error;

use rocketman::{
    connection::JetstreamConnection, handler, ingestion::LexiconIngestor, options::JetstreamOptions,
};

mod cursor;
mod db;
mod ingestors;

fn setup_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
}

fn setup_metrics() {
    // Initialize metrics here
    if let Err(e) = PrometheusBuilder::new().install() {
        error!(
            "Failed to install, program will run without Prometheus exporter: {}",
            e
        );
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    setup_tracing();
    setup_metrics();

    let pool = db::init_pool()
        .await
        .expect("Could not get PostgreSQL pool");

    let opts = JetstreamOptions::builder()
        .wanted_collections(vec!["fm.teal.alpha.feed.play".to_string()])
        .build();

    let jetstream = JetstreamConnection::new(opts);

    let mut ingestors: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>> = HashMap::new();

    ingestors.insert(
        "fm.teal.alpha.feed.play".to_string(),
        Box::new(ingestors::teal::feed_play::PlayIngestor::new(pool)),
    );

    // tracks the last message we've processed
    // TODO: read from db/config so we can resume from where we left off in case of crash
    let cursor: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(load_cursor().await));

    // get channels
    let msg_rx = jetstream.get_msg_rx();
    let reconnect_tx = jetstream.get_reconnect_tx();

    // Spawn a task to process messages from the queue.
    // bleh at this clone
    let c_cursor = cursor.clone();
    tokio::spawn(async move {
        while let Ok(message) = msg_rx.recv_async().await {
            if let Err(e) =
                handler::handle_message(message, &ingestors, reconnect_tx.clone(), c_cursor.clone())
                    .await
            {
                error!("Error processing message: {}", e);
            };
        }
    });

    // store cursor every so often
    let c_cursor = cursor.clone();
    tokio::spawn(async move {
        let ceursor = c_cursor.clone();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let cursor_to_store: Option<String> = {
                let cursor_guard = ceursor.lock().unwrap();
                cursor_guard.clone()
            };
            if let Some(cursor) = cursor_to_store {
                if let Err(e) = cursor::store_cursor(&cursor).await {
                    error!("Error storing cursor: {}", e);
                }
            }
        }
    });

    if let Err(e) = jetstream.connect(cursor.clone()).await {
        error!("Failed to connect to Jetstream: {}", e);
        std::process::exit(1);
    }
}
