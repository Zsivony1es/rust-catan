//! Trade offers: bank (4:1), port (3:1 / 2:1) and player-to-player proposals.
//!
//! Rules enforced here: no like-for-like swaps, no empty sides (no free
//! gifts). Turn ownership, ratios and port access are checked by the engine.

use serde::{Deserialize, Serialize};

use super::bag::{self, ResourceBag};
use super::enums::{PlayerColor, Resource};

/// Which counterparty a trade executes against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeKind {
    Bank,
    Port,
    Player,
}

/// A validated give/want pair. Both sides must be non-empty and share no
/// resource (like-for-like laundering is forbidden).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffer {
    pub from: PlayerColor,
    pub give: ResourceBag,
    pub want: ResourceBag,
}

impl TradeOffer {
    /// Validate shape rules (hands/ratios are the engine's job).
    pub fn validate(
        from: PlayerColor,
        give: ResourceBag,
        want: ResourceBag,
    ) -> Result<Self, String> {
        if bag::total(&give) == 0 || bag::total(&want) == 0 {
            return Err("trade must give and receive at least one card (no gifts)".into());
        }
        for resource in Resource::ALL {
            if bag::count(&give, resource) > 0 && bag::count(&want, resource) > 0 {
                return Err("like-for-like trades are not allowed".into());
            }
        }
        Ok(Self { from, give, want })
    }

    /// Bank/port shape: give exactly `ratio` of a single resource, want
    /// exactly 1 card.
    pub fn validate_fixed_ratio(
        from: PlayerColor,
        give: ResourceBag,
        want: ResourceBag,
        ratio: u8,
    ) -> Result<Self, String> {
        let offer = Self::validate(from, give, want)?;
        if bag::total(&offer.give) != u32::from(ratio) || offer.give.len() != 1 {
            return Err(format!("must give exactly {ratio} of a single resource"));
        }
        if bag::total(&offer.want) != 1 {
            return Err("must take exactly 1 card".into());
        }
        Ok(offer)
    }
}

/// A proposed player-to-player trade awaiting the counterparty's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrade {
    pub id: String,
    pub from: PlayerColor,
    pub to: PlayerColor,
    pub give: ResourceBag,
    pub want: ResourceBag,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::bag;

    fn offer(give: &[(Resource, u8)], want: &[(Resource, u8)]) -> Result<TradeOffer, String> {
        TradeOffer::validate(PlayerColor::Red, bag::bag(give), bag::bag(want))
    }

    #[test]
    fn rejects_gifts_and_laundering() {
        assert!(offer(&[], &[(Resource::Ore, 1)]).is_err());
        assert!(offer(&[(Resource::Wood, 4)], &[]).is_err());
        assert!(offer(
            &[(Resource::Ore, 3)],
            &[(Resource::Ore, 1), (Resource::Wheat, 1)]
        )
        .is_err());
        assert!(offer(&[(Resource::Wood, 4)], &[(Resource::Ore, 1)]).is_ok());
    }
}
