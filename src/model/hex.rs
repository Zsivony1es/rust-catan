use serde::{Deserialize, Serialize};

use super::enums::FieldType;

/// A terrain hex on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hex {
    pub id: u32,
    /// Terrain type (desert produces nothing).
    pub field: FieldType,
    /// Number token; `None` for the desert (there is no 7 disc).
    pub number: Option<u8>,
    /// Whether the robber currently blocks this hex.
    pub has_robber: bool,
}

impl Hex {
    #[must_use]
    pub const fn new(id: u32, field: FieldType, number: Option<u8>) -> Self {
        Self {
            id,
            field,
            number,
            has_robber: matches!(field, FieldType::Desert),
        }
    }

    /// Dice pip value of the number token (desert / invalid -> 0).
    #[must_use]
    pub fn pips(self) -> u32 {
        match self.number {
            None | Some(7) => 0,
            Some(n) => 6 - u32::from(n.abs_diff(7)),
        }
    }
}
