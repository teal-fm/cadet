# Rocketman

A modular(ish) jetstream consumer. Backed by Tungstenite.

## Installation
```toml
[dependencies]
rocketman = "0.2.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

## Usage
```rust
use rocketman::{
    connection::JetstreamConnection,
    handler,
    ingestion::LexiconIngestor,
    options::JetstreamOptions,
    types::event::{Event},
};
use serde_json::Value;
use std::{collections::HashMap, sync::{Arc, Mutex}};
use anyhow::Result;
use async_trait::async_trait;

#[tokio::main]
async fn main() {
    // init the builder
    let opts = JetstreamOptions::builder()
        // your EXACT nsids
        .wanted_collections(vec!["com.example.cool.nsid".to_string()])
        .build();
    // create the jetstream connector
    let jetstream = JetstreamConnection::new(opts);

    // create your ingestors
    let mut ingestors: HashMap<String, Box<dyn LexiconIngestor + Send + Sync>> = HashMap::new();
    ingestors.insert(
        // your EXACT nsid
        "com.example.cool.nsid".to_string(),
        Box::new(MyCoolIngestor),
    );

    // tracks the last message we've processed
    let cursor: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));

    // get channels
    let msg_rx = jetstream.get_msg_rx();
    let reconnect_tx = jetstream.get_reconnect_tx();

    // spawn a task to process messages from the queue.
    // this is a simple implementation, you can use a more complex one based on needs.
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
    // retries internally, but may fail if there is an extreme error.
    if let Err(e) = jetstream.connect(cursor.clone()).await {
        eprintln!("Failed to connect to Jetstream: {}", e);
        std::process::exit(1);
    }
}

pub struct MyCoolIngestor;

/// A cool ingestor implementation. Will just print the message. Does not do verification.
#[async_trait]
impl LexiconIngestor for MyCoolIngestor {
    async fn ingest(&self, message: Event<Value>) -> Result<()> {
        println!("{:?}", message);
        // Process message for default lexicon.
        Ok(())
    }
}
```

## Features

- **Modular Design**: Implement custom ingestors for different lexicon types
- **Automatic Reconnection**: Built-in reconnection logic with backoff
- **Compression Support**: Optional zstd compression support (enabled by default)
- **Async/Await**: Fully async implementation using tokio
- **Message Queuing**: Internal message queuing with flume channels

## Features Flags

- `zstd` (default): Enable zstd compression support

## Troubleshooting

### Common Stalling Issues

If rocketman stalls after a few minutes, the most likely causes are:

1. **Slow Ingestors** - Check if your ingestors are blocking or taking too long
2. **Queue Backpressure** - The message queue fills up faster than it's consumed
3. **Network Issues** - Connection problems not triggering reconnection

### Quick Fixes

- Keep ingestors fast (aim for <100ms per message)
- Use async I/O, avoid blocking operations
- Handle errors gracefully to prevent crashes
- Monitor for "Slow ingestor" warnings in logs

### Debug Logging

```rust
tracing_subscriber::fmt()
    .with_max_level(tracing::Level::WARN)
    .init();
```

## License

MIT

## Credits

Based heavily on [phil's jetstream consumer on atcosm constellation.](https://github.com/atcosm/links/blob/main/constellation/src/consumer/jetstream.rs)