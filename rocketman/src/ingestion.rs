use anyhow::Result;

pub trait LexiconIngestor {
    fn ingest(&self, message: &str) -> Result<()>;
}

pub struct DefaultLexiconIngestor;

impl LexiconIngestor for DefaultLexiconIngestor {
    fn ingest(&self, message: &str) -> Result<()> {
        println!("Default lexicon processing: {}", message);
        // Process message for default lexicon.
        Ok(())
    }
}
