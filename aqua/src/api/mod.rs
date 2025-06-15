use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use sys_info;

use crate::ctx::Context;

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaOsInfo {
    os_type: String,
    release: String,
    hostname: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaAppInfo {
    git_hash: String,
    git_date: String,
    build_time: String,
    rustc_ver: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetaInfo {
    os: MetaOsInfo,
    app: MetaAppInfo,
}

pub async fn get_meta_info(
    Extension(_ctx): Extension<Context>,
) -> impl axum::response::IntoResponse {
    // Retrieve system information
    let git_hash = env!("VERGEN_GIT_DESCRIBE");
    let git_date = env!("VERGEN_GIT_COMMIT_DATE");
    let build_time = env!("VERGEN_BUILD_TIMESTAMP");
    let rustc_ver = env!("VERGEN_RUSTC_SEMVER");

    let os_type = sys_info::os_type().unwrap_or_else(|_| "Unknown".to_string());
    let os_release = sys_info::os_release().unwrap_or_else(|_| "Unknown".to_string());
    let hostname = sys_info::hostname().unwrap_or_else(|_| "Unknown".to_string());

    Json(MetaInfo {
        os: MetaOsInfo {
            os_type,
            release: os_release,
            hostname,
        },
        app: MetaAppInfo {
            git_hash: git_hash.to_string(),
            git_date: git_date.to_string(),
            build_time: build_time.to_string(),
            rustc_ver: rustc_ver.to_string(),
        },
    })
}
