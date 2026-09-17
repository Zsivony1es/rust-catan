//! Shared game state serialized as JSON between server and client.
//!
//! The server is authoritative: clients only send intents and render the
//! [`GameSession`] they get back. Everything here is serde-serializable
//! with no lifetimes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::{
    BoardState, BuildingKind, DevCardKind, EdgeId, IntersectionId, PendingTrade, Player,
    PlayerColor, Resource, ResourceBag,
};

/// Opaque game identifier (UUID v4 string).
pub type GameId = String;

/// Number of resource cards of each type in a fresh supply (19x5 = 95).
pub const SUPPLY_PER_RESOURCE: u8 = 19;

/// Points needed to win.
pub const POINTS_TO_WIN: usize = 10;
/// Knights needed for the first Largest Army award.
pub const KNIGHTS_FOR_ARMY: u8 = 3;
/// Roads needed for the first Longest Route award.
pub const ROADS_FOR_ROUTE: usize = 5;

/// Game phase. Setup runs two placement rounds (forward then reverse order);
/// each turn is Production (roll) then Action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Lobby,
    SetupRound1,
    SetupRound2,
    Production,
    Action,
    GameOver,
}

/// Bonus tile kinds (2 VP each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BonusKind {
    LargestArmy,
    LongestRoute,
}

/// Structured log event (also served via `GET /games/:id/log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEventKind {
    GameCreated,
    PlayerJoined {
        color: PlayerColor,
    },
    GameStarted {
        order: Vec<PlayerColor>,
    },
    SetupPlaced {
        color: PlayerColor,
        intersection: IntersectionId,
        edge: EdgeId,
    },
    StartingResources {
        color: PlayerColor,
        cards: ResourceBag,
    },
    DiceRolled {
        color: PlayerColor,
        die1: u8,
        die2: u8,
    },
    Produced {
        color: PlayerColor,
        cards: ResourceBag,
    },
    DiscardOwed {
        color: PlayerColor,
        amount: u8,
    },
    Discarded {
        color: PlayerColor,
        cards: ResourceBag,
    },
    RobberMoved {
        color: PlayerColor,
        hex: u32,
    },
    Stolen {
        from: PlayerColor,
        to: PlayerColor,
        resource: Option<Resource>,
    },
    BuiltRoad {
        color: PlayerColor,
        edge: EdgeId,
    },
    BuiltSettlement {
        color: PlayerColor,
        intersection: IntersectionId,
    },
    BuiltCity {
        color: PlayerColor,
        intersection: IntersectionId,
    },
    DevBought {
        color: PlayerColor,
    },
    Traded {
        color: PlayerColor,
        kind: crate::model::TradeKind,
        give: ResourceBag,
        want: ResourceBag,
    },
    TradeProposed {
        id: String,
        from: PlayerColor,
        to: PlayerColor,
    },
    TradeAccepted {
        id: String,
    },
    TradeDeclined {
        id: String,
    },
    DevPlayed {
        color: PlayerColor,
        kind: DevCardKind,
    },
    Bonus {
        kind: BonusKind,
        holder: Option<PlayerColor>,
    },
    TurnEnded {
        color: PlayerColor,
    },
    Won {
        color: PlayerColor,
        points: usize,
    },
}

/// One log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameEvent {
    pub turn_number: u32,
    pub kind: GameEventKind,
}

/// Full authoritative game state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSession {
    pub id: GameId,
    pub phase: Phase,
    /// Counts individual turns (incremented on every end-turn).
    pub turn_number: u32,
    /// Index into `order` of the active player.
    pub turn_index: usize,
    /// Seating order; the first entry moves first.
    pub order: Vec<PlayerColor>,
    pub players: Vec<Player>,
    pub board: BoardState,
    /// Bank supply piles (shortage rule applies to production/invention).
    pub supply: ResourceBag,
    /// Face-down development deck; drawn from the end.
    pub dev_deck: Vec<DevCardKind>,
    pub longest_route_holder: Option<PlayerColor>,
    pub largest_army_holder: Option<PlayerColor>,
    pub winner: Option<PlayerColor>,
    pub log: Vec<GameEvent>,
    /// After a 7: players that still owe discards and how many cards each.
    pub pending_discards: HashMap<PlayerColor, u8>,
    /// After a 7 (and discards): the active player must move the robber.
    pub robber_pending: bool,
    /// Last dice roll this turn, if any.
    pub dice: Option<(u8, u8)>,
    /// A non-VP dev card has already been played this turn.
    pub dev_played_this_turn: bool,
    /// Setup placements completed (2 per player when done).
    pub setup_done: usize,
    /// Proposed player-to-player trades awaiting a decision.
    pub pending_trades: Vec<PendingTrade>,
}

impl GameSession {
    /// Fresh game in the lobby with a fixed board, full supply and a
    /// shuffled development deck.
    #[must_use]
    pub fn new(id: GameId, rng: &mut impl rand::Rng) -> Self {
        let mut supply = ResourceBag::new();
        for resource in Resource::ALL {
            supply.insert(resource, SUPPLY_PER_RESOURCE);
        }
        Self {
            id,
            phase: Phase::Lobby,
            turn_number: 0,
            turn_index: 0,
            order: Vec::new(),
            players: Vec::new(),
            board: BoardState::fixed_setup(),
            supply,
            dev_deck: DevCardKind::full_deck_shuffled(rng),
            longest_route_holder: None,
            largest_army_holder: None,
            winner: None,
            log: vec![GameEvent {
                turn_number: 0,
                kind: GameEventKind::GameCreated,
            }],
            pending_discards: HashMap::new(),
            robber_pending: false,
            dice: None,
            dev_played_this_turn: false,
            setup_done: 0,
            pending_trades: Vec::new(),
        }
    }

    /// Active player's color, if the game has started and is not over.
    #[must_use]
    pub fn active_color(&self) -> Option<PlayerColor> {
        match self.phase {
            Phase::Production | Phase::Action => self.order.get(self.turn_index).copied(),
            _ => None,
        }
    }

    /// Whose setup placement is expected next (forward then reverse order).
    #[must_use]
    pub fn setup_expected_color(&self) -> Option<PlayerColor> {
        let n = self.order.len();
        if n == 0 {
            return None;
        }
        match self.phase {
            Phase::SetupRound1 => self.order.get(self.setup_done).copied(),
            Phase::SetupRound2 => {
                let round2_done = self.setup_done.checked_sub(n)?;
                self.order.get(n - 1 - round2_done).copied()
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn player(&self, color: PlayerColor) -> Option<&Player> {
        self.players.iter().find(|p| p.color == color)
    }

    pub fn player_mut(&mut self, color: PlayerColor) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.color == color)
    }

    pub fn has_player(&self, color: PlayerColor) -> bool {
        self.players.iter().any(|p| p.color == color)
    }

    /// Victory points: settlements (1) + cities (2) + hidden VP cards +
    /// bonus tiles (2 each). VP cards count even the turn bought.
    #[must_use]
    pub fn victory_points(&self, color: PlayerColor) -> usize {
        let Some(player) = self.player(color) else {
            return 0;
        };
        let mut points = player.settlements.len() + 2 * player.cities.len() + player.vp_cards();
        if self.longest_route_holder == Some(color) {
            points += 2;
        }
        if self.largest_army_holder == Some(color) {
            points += 2;
        }
        points
    }

    /// Hex currently holding the robber, if any.
    #[must_use]
    pub fn robber_hex(&self) -> Option<u32> {
        self.board.hexes.iter().find(|h| h.has_robber).map(|h| h.id)
    }

    pub fn push_log(&mut self, kind: GameEventKind) {
        let turn_number = self.turn_number;
        self.log.push(GameEvent { turn_number, kind });
    }

    /// Colors with a settlement or city adjacent to `hex`.
    #[must_use]
    pub fn builders_on_hex(&self, hex: u32) -> Vec<PlayerColor> {
        let mut colors = Vec::new();
        for intersection in &self.board.intersections {
            if !intersection.hexes.contains(&hex) {
                continue;
            }
            if let Some(building) = intersection.building {
                if !colors.contains(&building.owner) {
                    colors.push(building.owner);
                }
            }
        }
        colors
    }

    /// Production value (pip sum) of every hex, for robber targeting.
    #[must_use]
    pub fn hex_pips(&self, hex: u32) -> u32 {
        self.board.hex(hex).map(|h| h.pips()).unwrap_or(0)
    }

    /// Whether any 7-resolution (discards / robber move) is still pending.
    #[must_use]
    pub fn seven_pending(&self) -> bool {
        !self.pending_discards.is_empty() || self.robber_pending
    }
}

/// Building VP split for tests/debug output.
#[must_use]
pub fn building_points(kind: BuildingKind) -> usize {
    match kind {
        BuildingKind::Settlement => 1,
        BuildingKind::City => 2,
    }
}

/// Discard owed for a hand of `total` cards (half rounded down past 7).
#[must_use]
pub const fn discard_for_total(total: u32) -> u8 {
    if total > 7 {
        (total / 2) as u8
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::bag;

    #[test]
    fn discard_rounding() {
        assert_eq!(discard_for_total(7), 0);
        assert_eq!(discard_for_total(8), 4);
        assert_eq!(discard_for_total(9), 4);
        assert_eq!(discard_for_total(19), 9);
    }

    #[test]
    fn vp_counts_everything() {
        use rand::SeedableRng as _;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let mut session = GameSession::new("test".into(), &mut rng);
        session.order = vec![PlayerColor::Red];
        let mut player = Player::new(PlayerColor::Red);
        player.settlements.push(1);
        player.cities.push(2);
        player.dev_hand.push(crate::model::DevCardInstance::new(
            DevCardKind::VictoryPoint,
            0,
        ));
        session.players.push(player);
        session.longest_route_holder = Some(PlayerColor::Red);
        // 1 + 2 + 1 + 2 = 6
        assert_eq!(session.victory_points(PlayerColor::Red), 6);
    }

    #[test]
    fn new_game_supply_and_deck() {
        use rand::SeedableRng as _;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let session = GameSession::new("test".into(), &mut rng);
        assert_eq!(bag::total(&session.supply), 95);
        assert_eq!(session.dev_deck.len(), 25);
        assert_eq!(session.phase, Phase::Lobby);
    }
}
