//! Sparse resource-bag helpers.
//!
//! Hands, costs and the supply are all `HashMap<Resource, u8>` where a
//! missing entry means zero. These helpers keep that invariant in one place
//! so game logic never trips over missing keys.

use std::collections::HashMap;

use super::enums::Resource;

/// A multiset of resources. Missing keys count as zero.
pub type ResourceBag = HashMap<Resource, u8>;

/// How many `resource` are in `bag` (0 if absent).
#[must_use]
pub fn count(bag: &ResourceBag, resource: Resource) -> u8 {
    bag.get(&resource).copied().unwrap_or(0)
}

/// Total cards in `bag`.
#[must_use]
pub fn total(bag: &ResourceBag) -> u32 {
    bag.values().map(|v| u32::from(*v)).sum()
}

/// Add `amount` of `resource` to `bag`.
pub fn add(bag: &mut ResourceBag, resource: Resource, amount: u8) {
    if amount == 0 {
        return;
    }
    *bag.entry(resource).or_insert(0) += amount;
}

/// Add all entries of `other` into `bag`.
pub fn add_all(bag: &mut ResourceBag, other: &ResourceBag) {
    for (resource, amount) in other {
        add(bag, *resource, *amount);
    }
}

/// Remove `amount` of `resource`; returns `false` (no mutation) if insufficient.
pub fn take(bag: &mut ResourceBag, resource: Resource, amount: u8) -> bool {
    let entry = bag.entry(resource).or_insert(0);
    if *entry < amount {
        return false;
    }
    *entry -= amount;
    if *entry == 0 {
        bag.remove(&resource);
    }
    true
}

/// Remove all entries of `cost` from `bag`; returns `false` (no mutation) if
/// any resource is insufficient.
pub fn take_all(bag: &mut ResourceBag, cost: &ResourceBag) -> bool {
    for (resource, amount) in cost {
        if count(bag, *resource) < *amount {
            return false;
        }
    }
    for (resource, amount) in cost {
        let ok = take(bag, *resource, *amount);
        debug_assert!(ok);
    }
    true
}

/// Whether `bag` holds at least the amounts in `cost`.
#[must_use]
pub fn can_afford(bag: &ResourceBag, cost: &ResourceBag) -> bool {
    cost.iter()
        .all(|(resource, amount)| count(bag, *resource) >= *amount)
}

/// Build a bag from `(resource, amount)` pairs.
#[must_use]
pub fn bag(pairs: &[(Resource, u8)]) -> ResourceBag {
    let mut bag = ResourceBag::new();
    for (resource, amount) in pairs {
        add(&mut bag, *resource, *amount);
    }
    bag
}
