use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerColor {
    Red,
    Blue,
    Orange,
    White,
}

impl PlayerColor {
    pub const ALL: [PlayerColor; 4] = [
        PlayerColor::Red,
        PlayerColor::Blue,
        PlayerColor::Orange,
        PlayerColor::White,
    ];
}
