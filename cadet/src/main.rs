use std::collections::HashMap;

use flume::unbounded;
use metrics_exporter_prometheus::PrometheusBuilder;
use rocketman::{
    connection::JetstreamConnection, handler, ingestion::LexiconIngestor,
    options::JetstreamOptions, types::event::Event,
};

use serde_json::Value;
use types::app::bsky::feed::Post;

fn setup_tracing() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
}

fn setup_metrics() {
    // Initialize metrics here
    if let Err(e) = PrometheusBuilder::new().install() {
        eprintln!(
            "Failed to install, program will run without Prometheus exporter: {}",
            e
        );
    }
}

#[tokio::main]
async fn main() {
    setup_tracing();
    setup_metrics();

    let (msg_tx, msg_rx) = unbounded();

    let opts = JetstreamOptions::builder()
        .wanted_collections(vec!["app.bsky.feed.post".to_string()])
        .build();

    let jetstream = JetstreamConnection::new(opts);

    let mut ingestors: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>> = HashMap::new();
    ingestors.insert("app.bsky.feed.post".to_string(), Box::new(PostPrinter));

    // Spawn a task to process messages from the queue.
    tokio::spawn(async move {
        while let Ok(message) = msg_rx.recv_async().await {
            if let Err(e) = handler::handle_message(message, &ingestors).await {
                eprintln!("Error processing message: {}", e);
            };
        }
    });

    if let Err(e) = jetstream.connect(msg_tx).await {
        eprintln!("Failed to connect to Jetstream: {}", e);
        std::process::exit(1);
    }
}

/// Example trait
pub struct PostPrinter;

#[async_trait::async_trait]
impl LexiconIngestor for PostPrinter {
    fn ingest(&self, message: Event<Value>) -> anyhow::Result<()> {
        if let Some(record) = message.commit.record {
            let json = serde_json::from_value::<types::app::bsky::feed::post::RecordData>(record)?;
            println!("{}: {:?}", message.did, json.text);
        } else {
            println!("{}: Message {} deleted", message.did, message.commit.rkey)
        }
        Ok(())
    }
}
