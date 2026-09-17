//! Shared library for both binaries.
//!
//! ```text
//! rust-catan (lib)  <-  server (bin)  +  client/player (bin)
//! ```
//! Import shared types in either binary with e.g.
//! `use rust_catan::{GameSession, Player};`

pub mod client;
pub mod error;
pub mod game_session;
pub mod model;
pub mod server;

pub use error::{ErrorCode, GameError};
pub use game_session::{
    BonusKind, GameEvent, GameEventKind, GameId, GameSession, Phase, KNIGHTS_FOR_ARMY,
    POINTS_TO_WIN, ROADS_FOR_ROUTE, SUPPLY_PER_RESOURCE,
};
pub use model::{
    bag, BoardState, BuildKind, Building, BuildingKind, DevCardInstance, DevCardKind, Edge, EdgeId,
    FieldType, Hex, HexId, Intersection, IntersectionId, PendingTrade, Player, PlayerColor, Port,
    PortKind, Resource, ResourceBag, TradeKind, TradeOffer,
};
