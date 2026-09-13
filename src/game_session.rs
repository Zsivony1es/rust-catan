use serde::{Deserialize, Serialize};

use crate::model::Player;

/// Shared game state, serialized as JSON between server and client.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameSession {
    pub turn: u32,
    pub players: Vec<Player>,
}

impl GameSession {
    pub fn new() -> Self {
        Self {
            turn: 0,
            players: Vec::new(),
        }
    }
}
