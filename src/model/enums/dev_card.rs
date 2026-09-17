use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

/// Development card kinds. Deck composition per rulebook: 14 Knight,
/// 5 Victory Point, 2 Monopoly, 2 Road Building, 2 Invention (Year of Plenty).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DevCardKind {
    #[default]
    Knight,
    VictoryPoint,
    Monopoly,
    RoadBuilding,
    Invention,
}

impl DevCardKind {
    pub const ALL: [DevCardKind; 5] = [
        DevCardKind::Knight,
        DevCardKind::VictoryPoint,
        DevCardKind::Monopoly,
        DevCardKind::RoadBuilding,
        DevCardKind::Invention,
    ];

    /// Number of each kind in a fresh 25-card deck.
    #[must_use]
    pub const fn count_in_deck(self) -> usize {
        match self {
            DevCardKind::Knight => 14,
            DevCardKind::VictoryPoint => 5,
            DevCardKind::Monopoly => 2,
            DevCardKind::RoadBuilding => 2,
            DevCardKind::Invention => 2,
        }
    }

    /// Total cards in a fresh deck (25).
    #[must_use]
    pub const fn full_deck_size() -> usize {
        14 + 5 + 2 + 2 + 2
    }

    /// Build a fresh shuffled 25-card deck. Cards are drawn from the end.
    pub fn full_deck_shuffled(rng: &mut impl rand::Rng) -> Vec<DevCardKind> {
        let mut deck = Vec::with_capacity(Self::full_deck_size());
        for kind in Self::ALL {
            deck.extend(std::iter::repeat_n(kind, kind.count_in_deck()));
        }
        deck.shuffle(rng);
        deck
    }

    /// Whether a card bought on `built_turn` may be played on `current_turn`.
    ///
    /// At most one non-VP card per turn, never the turn it was bought.
    /// Victory Point cards are revealed (counted) even the turn bought.
    #[must_use]
    pub const fn playable_this_turn(self, built_turn: u32, current_turn: u32) -> bool {
        match self {
            DevCardKind::VictoryPoint => true,
            _ => built_turn != current_turn,
        }
    }
}

/// A development card held by a player. `built_turn` is the session
/// `turn_number` when drawn; it enforces the "not the turn bought" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DevCardInstance {
    pub kind: DevCardKind,
    pub built_turn: u32,
}

impl DevCardInstance {
    #[must_use]
    pub const fn new(kind: DevCardKind, built_turn: u32) -> Self {
        Self { kind, built_turn }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng as _;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn deck_composition_is_14_5_2_2_2() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let deck = DevCardKind::full_deck_shuffled(&mut rng);
        assert_eq!(deck.len(), 25);
        for kind in DevCardKind::ALL {
            assert_eq!(
                deck.iter().filter(|k| **k == kind).count(),
                kind.count_in_deck(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn play_restriction_same_turn_except_vp() {
        assert!(!DevCardKind::Knight.playable_this_turn(7, 7));
        assert!(DevCardKind::Knight.playable_this_turn(6, 7));
        assert!(DevCardKind::VictoryPoint.playable_this_turn(7, 7));
    }
}
