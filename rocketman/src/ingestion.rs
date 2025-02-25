use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::types::event::Event;

#[async_trait]
pub trait LexiconIngestor {
    async fn ingest(&self, message: Event<Value>) -> Result<()>;
}

pub struct DefaultLexiconIngestor;

#[async_trait]
impl LexiconIngestor for DefaultLexiconIngestor {
    async fn ingest(&self, message: Event<Value>) -> Result<()> {
        println!("Default lexicon processing: {:?}", message);
        // Process message for default lexicon.
        Ok(())
    }
}
