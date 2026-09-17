use serde::{Deserialize, Serialize};

use super::bag::{self, ResourceBag};
use super::board::{EdgeId, IntersectionId};
use super::enums::{DevCardInstance, DevCardKind, PlayerColor, Resource};

/// Piece limits per color (rulebook components).
pub const MAX_ROADS: usize = 15;
pub const MAX_SETTLEMENTS: usize = 5;
pub const MAX_CITIES: usize = 4;

/// A player's private + public state. Intersections/edges are stored as ids
/// (no lifetimes, JSON-friendly); resolve them against `BoardState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub color: PlayerColor,
    /// Resource hand (sparse: missing = 0).
    pub resources: ResourceBag,
    /// Development cards in hand (hidden until played).
    pub dev_hand: Vec<DevCardInstance>,
    /// Knights played (face-up pile; counts toward Largest Army).
    pub played_knights: u8,
    /// Intersections holding this player's settlements.
    pub settlements: Vec<IntersectionId>,
    /// Intersections holding this player's cities.
    pub cities: Vec<IntersectionId>,
    /// Edges holding this player's roads.
    pub roads: Vec<EdgeId>,
}

impl Player {
    #[must_use]
    pub fn new(color: PlayerColor) -> Self {
        Self {
            color,
            resources: ResourceBag::new(),
            dev_hand: Vec::new(),
            played_knights: 0,
            settlements: Vec::new(),
            cities: Vec::new(),
            roads: Vec::new(),
        }
    }

    /// Total resource cards in hand.
    #[must_use]
    pub fn resource_count(&self) -> u32 {
        bag::total(&self.resources)
    }

    /// Cards that must be discarded on a 7 (half rounded down, 0 if <= 7).
    #[must_use]
    pub fn discard_owed(&self) -> u8 {
        let total = self.resource_count();
        if total > 7 {
            (total / 2) as u8
        } else {
            0
        }
    }

    #[must_use]
    pub fn roads_left(&self) -> usize {
        MAX_ROADS.saturating_sub(self.roads.len())
    }

    #[must_use]
    pub fn settlements_left(&self) -> usize {
        MAX_SETTLEMENTS.saturating_sub(self.settlements.len())
    }

    #[must_use]
    pub fn cities_left(&self) -> usize {
        MAX_CITIES.saturating_sub(self.cities.len())
    }

    /// Hidden Victory Point cards in hand (counted at win check).
    #[must_use]
    pub fn vp_cards(&self) -> usize {
        self.dev_hand
            .iter()
            .filter(|c| c.kind == DevCardKind::VictoryPoint)
            .count()
    }

    /// How many cards of `kind` are playable on `current_turn`.
    #[must_use]
    pub fn playable(&self, kind: DevCardKind, current_turn: u32) -> usize {
        self.dev_hand
            .iter()
            .filter(|c| c.kind == kind && kind.playable_this_turn(c.built_turn, current_turn))
            .count()
    }

    #[must_use]
    pub fn count_resource(&self, resource: Resource) -> u8 {
        bag::count(&self.resources, resource)
    }
}
