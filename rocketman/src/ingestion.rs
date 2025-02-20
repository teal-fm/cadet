use anyhow::Result;
use serde_json::Value;

use crate::types::event::Event;

pub trait LexiconIngestor {
    fn ingest(&self, message: Event<Value>) -> Result<()>;
}

pub struct DefaultLexiconIngestor;

impl LexiconIngestor for DefaultLexiconIngestor {
    fn ingest(&self, message: Event<Value>) -> Result<()> {
        println!("Default lexicon processing: {:?}", message);
        // Process message for default lexicon.
        Ok(())
    }
}
