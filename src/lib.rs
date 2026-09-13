//! Shared library for both binaries.
//!
//! ```text
//! rust-catan (lib)  <-  server (bin)  +  client/player (bin)
//! ```
//! Import shared types in either binary with e.g.
//! `use rust_catan::{GameSession, Player};`

pub mod game_session;
pub mod model;

pub use game_session::GameSession;
pub use model::{Board, FieldType, Player, PlayerColor, Resource, RoadNode, RoadNodeType};
