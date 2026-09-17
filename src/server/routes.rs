//! HTTP layer: Axum handlers over the pure engine.
//!
//! Conventions: per-game mutex (never held across `.await`), `GameError`
//! maps to `{ "error": { "code", "message" } }` + 4xx, mutating routes need
//! `Authorization: Bearer <join-token>`.

use std::sync::{Arc, Mutex, MutexGuard};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::model::BuildKind;
use crate::server::dto::{
    BuildRequest, CreateGameRequest, DevPlayRequest, DiscardRequest, ErrorBody, ErrorDetail,
    HealthResponse, JoinRequest, JoinResponse, LogResponse, RobberRequest, RollResponse,
    SetupRequest, TradeRequest, TradeResponse,
};
use crate::server::engine::{self, BuildTarget, DevPlayParams};
use crate::server::state::{AppState, Game, DEFAULT_GAME_ID};
use crate::{GameError, PlayerColor, TradeKind};

impl IntoResponse for GameError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = Json(ErrorBody {
            error: ErrorDetail {
                code: self.code(),
                message: self.to_string(),
            },
        });
        (status, body).into_response()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/state", get(legacy_state))
        .route("/join", post(legacy_join))
        .route("/games", post(create_game))
        .route("/games/{id}/state", get(get_state))
        .route("/games/{id}/log", get(get_log))
        .route("/games/{id}/join", post(join))
        .route("/games/{id}/start", post(start))
        .route("/games/{id}/setup", post(setup))
        .route("/games/{id}/roll", post(roll))
        .route("/games/{id}/discard", post(discard))
        .route("/games/{id}/robber", post(robber))
        .route("/games/{id}/build", post(build))
        .route("/games/{id}/trade", post(trade))
        .route("/games/{id}/trade/{trade_id}/accept", post(accept_trade))
        .route("/games/{id}/trade/{trade_id}/decline", post(decline_trade))
        .route("/games/{id}/dev/play", post(dev_play))
        .route("/games/{id}/end-turn", post(end_turn))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn lock_game(game: &Arc<Mutex<Game>>) -> MutexGuard<'_, Game> {
    game.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn lookup(state: &AppState, id: &str) -> Result<Arc<Mutex<Game>>, GameError> {
    state
        .get(id)
        .await
        .ok_or_else(|| GameError::not_found(format!("game {id}")))
}

fn bearer_token(headers: &HeaderMap) -> Result<String, GameError> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| GameError::forbidden("missing Authorization token"))?;
    let raw = raw
        .to_str()
        .map_err(|_| GameError::forbidden("bad Authorization header"))?;
    raw.strip_prefix("Bearer ")
        .map(str::to_string)
        .ok_or_else(|| GameError::forbidden("use `Authorization: Bearer <token>`"))
}

/// Token owner inside this game.
fn auth_color(game: &Game, headers: &HeaderMap) -> Result<PlayerColor, GameError> {
    let token = bearer_token(headers)?;
    game.color_for_token(&token)
        .ok_or_else(|| GameError::forbidden("unknown token"))
}

fn log_action(game_id: &str, game: &Game, action: &str) {
    info!(
        game_id = game_id,
        phase = ?game.session.phase,
        active = ?game.session.active_color(),
        turn = game.session.turn_number,
        "{action}"
    );
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

async fn health() -> Json<HealthResponse> {
    info!(
        endpoint.method = "GET",
        endpoint.route = "/health",
        "GET /health called"
    );
    Json(HealthResponse {
        status: "ok".into(),
    })
}

async fn get_state(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let session = lock_game(&game).session.clone();
    Ok(Json(session))
}

async fn get_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LogResponse>, GameError> {
    let game = lookup(&state, &id).await?;
    let events = lock_game(&game).session.log.clone();
    let response = LogResponse { events };
    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Lobby
// ---------------------------------------------------------------------------

async fn create_game(
    State(state): State<AppState>,
    body: Option<Json<CreateGameRequest>>,
) -> Json<crate::GameSession> {
    let seed = body.and_then(|b| b.seed);
    let game = state.create_game(seed).await;
    let session = lock_game(&game).session.clone();
    info!(game_id = %session.id, "game created");
    Json(session)
}

async fn join(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    engine::join(&mut guard.session, req.color)?;
    let token = uuid::Uuid::new_v4().to_string();
    guard.tokens.insert(req.color, token.clone());
    log_action(&id, &guard, "join");
    Ok(Json(JoinResponse {
        session: guard.session.clone(),
        token,
    }))
}

async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let game = &mut *guard;
    engine::start(&mut game.session, &mut game.rng)?;
    log_action(&id, &guard, "start");
    Ok(Json(guard.session.clone()))
}

// ---------------------------------------------------------------------------
// Setup + turn engine
// ---------------------------------------------------------------------------

async fn setup(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    if caller != req.color {
        return Err(GameError::forbidden("token does not match color"));
    }
    engine::setup_place(
        &mut guard.session,
        req.color,
        req.intersection_id,
        req.edge_id,
    )?;
    log_action(&id, &guard, "setup");
    Ok(Json(guard.session.clone()))
}

async fn roll(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RollResponse>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    let game = &mut *guard;
    let (die1, die2) = engine::roll(&mut game.session, &mut game.rng)?;
    log_action(&id, &guard, "roll");
    Ok(Json(RollResponse {
        dice: [die1, die2],
        session: guard.session.clone(),
    }))
}

async fn discard(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<DiscardRequest>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    if caller != req.color {
        return Err(GameError::forbidden("token does not match color"));
    }
    engine::discard(&mut guard.session, req.color, &req.cards)?;
    log_action(&id, &guard, "discard");
    Ok(Json(guard.session.clone()))
}

async fn robber(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RobberRequest>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    let game = &mut *guard;
    engine::robber(&mut game.session, &mut game.rng, req.hex_id, req.victim)?;
    log_action(&id, &guard, "robber");
    Ok(Json(guard.session.clone()))
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

async fn build(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<BuildRequest>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    let target = match req.kind {
        BuildKind::Road => BuildTarget::Road(
            req.edge_id
                .ok_or_else(|| GameError::bad_request("road needs edge_id"))?,
        ),
        BuildKind::Settlement => BuildTarget::Settlement(
            req.intersection_id
                .ok_or_else(|| GameError::bad_request("settlement needs intersection_id"))?,
        ),
        BuildKind::City => BuildTarget::City(
            req.intersection_id
                .ok_or_else(|| GameError::bad_request("city needs intersection_id"))?,
        ),
        BuildKind::DevCard => BuildTarget::DevCard,
    };
    engine::build(&mut guard.session, caller, target, false)?;
    log_action(&id, &guard, "build");
    Ok(Json(guard.session.clone()))
}

async fn trade(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<TradeRequest>,
) -> Result<Json<TradeResponse>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    let trade_id = match req.kind {
        TradeKind::Bank => {
            engine::trade_bank(&mut guard.session, caller, &req.give, &req.want)?;
            None
        }
        TradeKind::Port => {
            engine::trade_port(&mut guard.session, caller, &req.give, &req.want)?;
            None
        }
        TradeKind::Player => {
            let to = req
                .to
                .ok_or_else(|| GameError::bad_request("player trade needs `to`"))?;
            Some(engine::propose_trade(
                &mut guard.session,
                caller,
                to,
                &req.give,
                &req.want,
            )?)
        }
    };
    log_action(&id, &guard, "trade");
    Ok(Json(TradeResponse {
        session: guard.session.clone(),
        trade_id,
    }))
}

async fn accept_trade(
    State(state): State<AppState>,
    Path((id, trade_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    engine::accept_trade(&mut guard.session, caller, &trade_id)?;
    log_action(&id, &guard, "trade accept");
    Ok(Json(guard.session.clone()))
}

async fn decline_trade(
    State(state): State<AppState>,
    Path((id, trade_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    engine::decline_trade(&mut guard.session, caller, &trade_id)?;
    log_action(&id, &guard, "trade decline");
    Ok(Json(guard.session.clone()))
}

async fn dev_play(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<DevPlayRequest>,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    let params = DevPlayParams {
        hex_id: req.hex_id,
        victim: req.victim,
        resource: req.resource,
        resources: req.resources,
        edges: req.edges,
    };
    let game = &mut *guard;
    engine::play_dev(&mut game.session, &mut game.rng, caller, req.kind, &params)?;
    log_action(&id, &guard, "dev play");
    Ok(Json(guard.session.clone()))
}

async fn end_turn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<crate::GameSession>, GameError> {
    let game = lookup(&state, &id).await?;
    let mut guard = lock_game(&game);
    let caller = auth_color(&guard, &headers)?;
    let active = guard.session.active_color();
    if active != Some(caller) {
        return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
    }
    engine::end_turn(&mut guard.session)?;
    log_action(&id, &guard, "end turn");
    Ok(Json(guard.session.clone()))
}

// ---------------------------------------------------------------------------
// Legacy aliases (default game)
// ---------------------------------------------------------------------------

async fn legacy_state(State(state): State<AppState>) -> Json<crate::GameSession> {
    info!(
        endpoint.method = "GET",
        endpoint.route = "/state",
        "GET /state called"
    );
    let game = state.get_or_create_default().await;
    let session = lock_game(&game).session.clone();
    Json(session)
}

async fn legacy_join(
    State(state): State<AppState>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<JoinResponse>, GameError> {
    info!(
        endpoint.method = "POST",
        endpoint.route = "/join",
        color = ?req.color,
        "POST /join called"
    );
    let game = state.get_or_create_default().await;
    let mut guard = lock_game(&game);
    // Idempotent for the legacy polling client: re-join returns a fresh token.
    if !guard.session.has_player(req.color) {
        engine::join(&mut guard.session, req.color)?;
    }
    let token = uuid::Uuid::new_v4().to_string();
    guard.tokens.insert(req.color, token.clone());
    let _ = DEFAULT_GAME_ID;
    Ok(Json(JoinResponse {
        session: guard.session.clone(),
        token,
    }))
}
