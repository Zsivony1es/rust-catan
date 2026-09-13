use serde::{Deserialize, Serialize};

use super::player_color::PlayerColor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoadNodeType {
    HasVP,
    HasVPWithSettler(PlayerColor),
    HasTown(PlayerColor),
    Empty,
}
