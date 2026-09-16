use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DevCard {
    Knight,
    VictoryPoint,
    Monopoly,
    RoadBuilding,
    Invention
}

impl DevCard {

    pub fn full_deck(self) -> Vec<DevCard> {

        const COMPOSITION: [(DevCard, usize); 5] = [
            (DevCard::Knight, 14),
            (DevCard::VictoryPoint, 5),
            (DevCard::Monopoly, 2),
            (DevCard::RoadBuilding, 2),
            (DevCard::Invention, 2),
        ];

        let deck: Vec<DevCard> = COMPOSITION
            .into_iter()
            .flat_map(|(kind, n)| std::iter::repeat(kind).take(n))
            .collect();
        return deck;
    }

    pub fn playable_this_turn(self, built_turn: u32, current_turn: u32) -> bool {
        return false;
    }
}