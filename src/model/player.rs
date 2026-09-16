use serde::{Deserialize, Serialize};

use super::enums::PlayerColor;

/// NOTE: the original sketch used `Vec<&RoadNode>` which would require
/// lifetimes and cannot be (de)serialized for the server API. We store
/// owning node ids instead; resolve them against `Board` when needed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Player {
    pub color: PlayerColor,
    pub settlements_on: Vec<u32>,
    pub cities_on: Vec<u32>,
}

impl Player {
    pub fn new(color: PlayerColor) -> Self {
        Self {
            color,
            settlements_on: Vec::new(),
            cities_on: Vec::new(),
        }
    }
}
