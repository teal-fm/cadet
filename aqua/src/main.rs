use axum::{Router, extract::Extension, routing::get};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use ctx::RawContext;
use repos::DataSource;
use repos::pg::PgDataSource;

mod api;
mod ctx;
mod db;
mod repos;
mod xrpc;

#[tokio::main]
async fn main() -> Result<(), String> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let db = db::init_pool().await.expect("failed to init db");
    let pgds = PgDataSource::new(db.clone()).boxed();
    let ctx = RawContext::new(pgds).build();

    let cors = CorsLayer::permissive();

    let app = Router::new()
        .route("/meta_info", get(api::get_meta_info))
        .nest("/xrpc/", xrpc::actor::actor_routes())
        .nest("/xrpc/", xrpc::feed::feed_routes())
        .nest("/xrpc/", xrpc::stats::stats_routes())
        .layer(Extension(ctx))
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
