//! Catan game server binary.
//!
//! Run with:
//! ```sh
//! cargo run --bin server
//! # optional: SERVER_ADDR=127.0.0.1:3000 SERVER_SEED=42 cargo run --bin server
//! ```
//! Full v1 contract: `POST /games`, `GET /games/:id/state`, join/start,
//! setup, roll, discard, robber, build, trade, dev play, end-turn.
//! Legacy aliases `GET /state` and `POST /join` target the `default` game.

use std::net::SocketAddr;

use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::prelude::*;

use rust_catan::server::{router, AppState};

fn init_sentry() -> sentry::ClientInitGuard {
    let dsn = std::env::var("SENTRY_DSN").unwrap_or_else(|_| {
        "https://547bc132aefdf46bec7eeff82cfa026e@o4512078782136320.ingest.de.sentry.io/4512078786723920"
            .into()
    });
    sentry::init((
        dsn,
        sentry::ClientOptions::new()
            .maybe_release(sentry::release_name!())
            .send_default_pii(true),
    ))
}

#[tokio::main]
async fn main() {
    let _guard = init_sentry();
    // One subscriber, two layers: fmt (console) + Sentry.
    // A single `info!` in a handler goes to both — no duplication.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter))
        .with(sentry::integrations::tracing::layer())
        .init();

    let addr: SocketAddr = std::env::var("SERVER_ADDR")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 3000)));

    let state = AppState::new();
    let app = router(state).layer(TraceLayer::new_for_http());

    info!("Catan server listening on {addr}");
    println!("Catan server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind server address");
    axum::serve(listener, app).await.expect("server error");
}
