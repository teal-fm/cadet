// connection.rs
use futures_util::{SinkExt, StreamExt};
use metrics::{counter, describe_counter, describe_histogram, histogram, Unit};
use std::cmp::min;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message},
};
use url::Url;

use crate::options::JetstreamOptions;

pub struct JetstreamConnection {
    pub opts: JetstreamOptions,
}

impl JetstreamConnection {
    pub fn new(opts: JetstreamOptions) -> Self {
        Self { opts }
    }

    pub async fn connect(
        &self,
        msg_sender: flume::Sender<Result<Message, Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        describe_counter!(
            "jetstream.connection.attempt",
            Unit::Count,
            "attempts to connect to jetstream service"
        );
        describe_counter!(
            "jetstream.connection.error",
            Unit::Count,
            "errors connecting to jetstream service"
        );
        describe_histogram!(
            "jetstream.connection.duration",
            Unit::Seconds,
            "Time connected to jetstream service"
        );
        let mut retry_interval = 1;

        loop {
            counter!("jetstream.connection.attempt");
            println!("Connecting to {}", self.opts.ws_url);
            let start = Instant::now();

            let mut url = Url::parse(&self.opts.ws_url.to_string()).unwrap();

            // Append query params
            if let Some(ref cols) = self.opts.wanted_collections {
                for col in cols {
                    url.query_pairs_mut().append_pair("wantedCollections", col);
                }
            }
            if let Some(ref dids) = self.opts.wanted_dids {
                for did in dids {
                    url.query_pairs_mut().append_pair("wantedDids", did);
                }
            }
            if let Some(ref cursor) = self.opts.cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            }

            match connect_async(url).await {
                Ok((ws_stream, response)) => {
                    let elapsed = start.elapsed();
                    println!("Connected. HTTP status: {}", response.status());

                    let (_, mut read) = ws_stream.split();
                    retry_interval = 1;

                    while let Some(message) = read.next().await {
                        histogram!("jetstream.connection.duration").record(elapsed.as_secs_f64());
                        if let Err(err) = msg_sender.send_async(message).await {
                            eprintln!("Failed to queue message: {}", err);
                        }
                    }
                }
                Err(e) => {
                    counter!("jetstream.connection.error");
                    eprintln!("Connection error: {}", e);
                }
            }

            let sleep_time = min(
                self.opts.max_retry_interval,
                retry_interval + rand::random::<u64>() % retry_interval,
            );
            println!("Reconnecting in {} seconds...", sleep_time);
            sleep(Duration::from_secs(sleep_time)).await;
            retry_interval *= 2;
        }
    }
}
