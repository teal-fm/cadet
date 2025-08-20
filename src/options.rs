use bon::Builder;

use crate::endpoints::JetstreamEndpoints;

#[derive(Builder, Debug)]
pub struct JetstreamOptions {
    #[builder(default)]
    pub ws_url: JetstreamEndpoints,
    #[builder(default = 120)]
    pub max_retry_interval_seconds: u64,
    #[builder(default = 60)]
    pub connection_success_time_seconds: u64,
    #[builder(default = 65535)]
    pub bound: usize,
    #[builder(default = 40)]
    pub timeout_time_sec: usize,
    #[cfg(feature = "zstd")]
    #[builder(default = true)]
    pub compress: bool,
    pub wanted_collections: Option<Vec<String>>,
    pub wanted_dids: Option<Vec<String>>,
    pub cursor: Option<String>,
}

impl Default for JetstreamOptions {
    fn default() -> Self {
        Self {
            ws_url: JetstreamEndpoints::default(),
            max_retry_interval_seconds: 120,
            connection_success_time_seconds: 60,
            bound: 65535,
            timeout_time_sec: 40,
            #[cfg(feature = "zstd")]
            compress: true,
            wanted_collections: None,
            wanted_dids: None,
            cursor: None,
        }
    }
}
