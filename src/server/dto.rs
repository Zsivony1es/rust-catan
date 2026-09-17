//! REST request/response DTOs (v1, JSON). Shared by the server handlers and
//! the AI client wrapper so both sides agree on shapes.

use serde::{Deserialize, Serialize};

use crate::model::{EdgeId, IntersectionId};
use crate::{DevCardKind, GameEvent, GameSession, PlayerColor, Resource, ResourceBag, TradeKind};

// ---------------------------------------------------------------------------
// Lobby
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub color: PlayerColor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResponse {
    pub session: GameSession,
    pub token: String,
}

// ---------------------------------------------------------------------------
// Setup / turn engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupRequest {
    pub color: PlayerColor,
    pub intersection_id: IntersectionId,
    pub edge_id: EdgeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollResponse {
    pub dice: [u8; 2],
    pub session: GameSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscardRequest {
    pub color: PlayerColor,
    pub cards: ResourceBag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RobberRequest {
    pub hex_id: u32,
    pub victim: Option<PlayerColor>,
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRequest {
    pub kind: crate::model::BuildKind,
    pub edge_id: Option<EdgeId>,
    pub intersection_id: Option<IntersectionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRequest {
    pub kind: TradeKind,
    /// Bank/Port: exact give pile (4 / 3 / 2 of one resource).
    /// Player: offered cards.
    pub give: ResourceBag,
    /// Bank/Port: exactly 1 wanted card. Player: requested cards.
    pub want: ResourceBag,
    /// Player trades only: counterparty color.
    pub to: Option<PlayerColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResponse {
    pub session: GameSession,
    pub trade_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DevPlayRequest {
    pub kind: DevCardKind,
    pub hex_id: Option<u32>,
    pub victim: Option<PlayerColor>,
    pub resource: Option<Resource>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub edges: Vec<EdgeId>,
}

// ---------------------------------------------------------------------------
// Reads / errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogResponse {
    pub events: Vec<GameEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub code: crate::ErrorCode,
    pub message: String,
}
