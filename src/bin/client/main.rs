//! Catan player (client) binary.
//!
//! Run with the server up in another terminal:
//! ```sh
//! cargo run --bin server
//! cargo run --bin client
//! # optional:
//! # SERVER_URL=http://127.0.0.1:3000 cargo run --bin client -- Blue
//! ```
//! The client checks `/health`, joins via `POST /join`, then prints `/state`.

use rust_catan::GameSession;

fn init_sentry() -> sentry::ClientInitGuard {
    sentry::init((
        "https://547bc132aefdf46bec7eeff82cfa026e@o4512078782136320.ingest.de.sentry.io/4512078786723920",
        sentry::ClientOptions::new()
            .maybe_release(sentry::release_name!())
            .send_default_pii(true),
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = init_sentry();

    let base_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
    // `cargo run --bin client -- Blue` picks the color to join with.
    let color = std::env::args().nth(1).unwrap_or_else(|| "Red".into());

    let http = reqwest::Client::new();

    let health_url = format!("{base_url}/health");
    let health: serde_json::Value = http.get(&health_url).send().await?.error_for_status()?.json().await?;
    println!("GET {health_url} -> {health}");

    let join_url = format!("{base_url}/join");
    let joined: GameSession = http
        .post(&join_url)
        .json(&serde_json::json!({ "color": color }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    println!("POST {join_url} {{\"color\": \"{color}\"}} -> {joined:?}");

    let state_url = format!("{base_url}/state");
    let state: GameSession = http.get(&state_url).send().await?.error_for_status()?.json().await?;
    println!("GET {state_url} -> turn={} players={:?}", state.turn, state.players);

    Ok(())
}
