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

// You can add additional implementations for different lexicons.
pub struct CustomLexiconIngestor;

impl LexiconIngestor for CustomLexiconIngestor {
    fn ingest(&self, message: &str) -> Result<()> {
        println!("Custom lexicon processing: {}", message);
        // Process message for a custom lexicon.
        Ok(())
    }
}
