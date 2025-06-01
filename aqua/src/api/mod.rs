use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use sys_info;

use crate::ctx::Context;

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaInfo {
    pub os_type: String,
    pub os_release: String,
    pub hostname: String,
}

pub async fn get_meta_info(
    Extension(_ctx): Extension<Context>,
) -> impl axum::response::IntoResponse {
    let os_type = sys_info::os_type().unwrap_or_else(|_| "Unknown".to_string());
    let os_release = sys_info::os_release().unwrap_or_else(|_| "Unknown".to_string());
    let hostname = sys_info::hostname().unwrap_or_else(|_| "Unknown".to_string());

    Json(MetaInfo {
        os_type,
        os_release,
        hostname,
    })
}
