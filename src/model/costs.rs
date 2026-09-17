//! Build costs per the rulebook player aid.
//!
//! - Road = 1 wood + 1 brick
//! - Settlement = 1 wood + 1 brick + 1 wool + 1 wheat
//! - City = 2 wheat + 3 ore (upgrades an existing settlement)
//! - Development card = 1 wool + 1 wheat + 1 ore

use super::bag::{self, ResourceBag};
use super::enums::Resource;

/// What can be built during the action phase (or granted for free by
/// the Road Building development card).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildKind {
    Road,
    Settlement,
    City,
    DevCard,
}

/// Resource cost of building `kind`.
#[must_use]
pub fn cost(kind: BuildKind) -> ResourceBag {
    match kind {
        BuildKind::Road => bag::bag(&[(Resource::Wood, 1), (Resource::Brick, 1)]),
        BuildKind::Settlement => bag::bag(&[
            (Resource::Wood, 1),
            (Resource::Brick, 1),
            (Resource::Wool, 1),
            (Resource::Wheat, 1),
        ]),
        BuildKind::City => bag::bag(&[(Resource::Wheat, 2), (Resource::Ore, 3)]),
        BuildKind::DevCard => bag::bag(&[
            (Resource::Wool, 1),
            (Resource::Wheat, 1),
            (Resource::Ore, 1),
        ]),
    }
}

/// Whether `hand` can pay for `kind`.
#[must_use]
pub fn can_afford(hand: &ResourceBag, kind: BuildKind) -> bool {
    bag::can_afford(hand, &cost(kind))
}

/// Pay for `kind` from `hand` into `supply`. Returns `false` (no mutation)
/// if the hand cannot afford it.
pub fn pay(hand: &mut ResourceBag, supply: &mut ResourceBag, kind: BuildKind) -> bool {
    let price = cost(kind);
    if !bag::take_all(hand, &price) {
        return false;
    }
    bag::add_all(supply, &price);
    true
}
