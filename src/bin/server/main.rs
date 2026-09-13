//! Catan game server binary.
//!
//! Run with:
//! ```sh
//! cargo run --bin server
//! # optional: SERVER_ADDR=127.0.0.1:3000 cargo run --bin server
//! ```
//! Endpoints:
//! - `GET /health` -> `{ "status": "ok" }`
//! - `GET /state`  -> current `GameSession` as JSON
//! - `POST /join` with `{ "color": "Red" }` -> joins game, returns updated `GameSession`

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::{get, post}};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::prelude::*;

use rust_catan::{GameSession, Player, PlayerColor};

#[derive(Clone)]
struct AppState {
    session: Arc<Mutex<GameSession>>,
}

#[derive(Debug, Deserialize)]
struct JoinRequest {
    color: PlayerColor,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    // Single macro -> console (fmt layer) + Sentry (sentry tracing layer).
    // Fields become Sentry log attributes, queryable in the Logs explorer.
    info!(
        endpoint.method = "GET",
        endpoint.route = "/health",
        "GET /health called"
    );
    Json(Health { status: "ok" })
}

async fn get_state(State(state): State<AppState>) -> Json<GameSession> {
    info!(
        endpoint.method = "GET",
        endpoint.route = "/state",
        "GET /state called"
    );
    let session = state.session.lock().expect("session lock").clone();
    Json(session)
}

async fn join(
    State(state): State<AppState>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<GameSession>, StatusCode> {
    info!(
        endpoint.method = "POST",
        endpoint.route = "/join",
        color = ?req.color,
        "POST /join called"
    );
    let mut session = state.session.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Avoid duplicate colors for now.
    if !session.players.iter().any(|p| p.color == req.color) {
        session.players.push(Player::new(req.color));
        session.turn += 1;
    }
    Ok(Json(session.clone()))
}

fn init_sentry() -> sentry::ClientInitGuard {
    sentry::init((
        "https://547bc132aefdf46bec7eeff82cfa026e@o4512078782136320.ingest.de.sentry.io/4512078786723920",
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

    let state = AppState {
        session: Arc::new(Mutex::new(GameSession::new())),
    };

    let app: Router = Router::new()
        .route("/health", get(health))
        .route("/state", get(get_state))
        .route("/join", post(join))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    info!("Catan server listening on {addr}");
    println!("Catan server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind server address");
    axum::serve(listener, app)
        .await
        .expect("server error");
}
