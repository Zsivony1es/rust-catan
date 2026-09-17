//! Greedy AI strategy: pure, deterministic decisions over [`GameSession`].
//!
//! Priority: City > Settlement > Road > DevCard, bank/port trades to finish
//! the next build, opportunistic dev plays. The main loop maps
//! [`PlannedAction`] onto API calls. "Optimal" search (MCTS/EV) is post-MVP.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::{bag, costs, BuildKind, EdgeId, IntersectionId};
use crate::{DevCardKind, GameSession, Phase, PlayerColor, Resource, ResourceBag, TradeKind};

/// One concrete thing the strategy wants to do.
#[derive(Debug, Clone)]
pub enum PlannedAction {
    BuildRoad(EdgeId),
    BuildSettlement(IntersectionId),
    BuildCity(IntersectionId),
    BuyDev,
    BankTrade {
        give: ResourceBag,
        want: ResourceBag,
    },
    PortTrade {
        give: ResourceBag,
        want: ResourceBag,
    },
    ProposeTrade {
        to: PlayerColor,
        give: ResourceBag,
        want: ResourceBag,
    },
    PlayKnight {
        hex: u32,
        victim: Option<PlayerColor>,
    },
    PlayMonopoly {
        resource: Resource,
    },
    PlayRoadBuilding {
        edges: Vec<EdgeId>,
    },
    PlayInvention {
        resources: Vec<Resource>,
    },
    EndTurn,
}

pub struct GreedyStrategy;

impl GreedyStrategy {
    // -- setup ----------------------------------------------------------

    /// Best opening placement: pip-rich, resource-diverse intersection plus
    /// an adjacent empty edge reaching toward more pips.
    #[must_use]
    pub fn pick_setup(
        session: &GameSession,
        color: PlayerColor,
    ) -> Option<(IntersectionId, EdgeId)> {
        let _ = color;
        let mut spots: Vec<(u32, IntersectionId)> = session
            .board
            .intersections
            .iter()
            .filter(|i| session.board.distance_rule_ok(i.id))
            .map(|i| (setup_score(session, i.id), i.id))
            .collect();
        spots.sort();
        let (_, best) = spots.pop()?;
        // Adjacent empty edge toward the richest neighbor corner.
        let mut edges: Vec<(u32, EdgeId)> = session
            .board
            .incident_edges(best)
            .into_iter()
            .filter(|e| {
                session
                    .board
                    .edge(*e)
                    .is_some_and(|edge| edge.road.is_none())
            })
            .map(|e| {
                let edge = session.board.edge(e).expect("listed");
                let other = if edge.a == best { edge.b } else { edge.a };
                (intersection_pips(session, other), e)
            })
            .collect();
        edges.sort();
        let (_, edge) = edges.pop()?;
        Some((best, edge))
    }

    // -- 7 handling ------------------------------------------------------

    /// Discard exactly what is owed, starting with the most abundant piles.
    #[must_use]
    pub fn pick_discard(session: &GameSession, color: PlayerColor) -> ResourceBag {
        let owed = session.pending_discards.get(&color).copied().unwrap_or(0);
        let mut counts: Vec<(u8, Resource)> = Resource::ALL
            .iter()
            .map(|r| {
                (
                    session
                        .player(color)
                        .map(|p| p.count_resource(*r))
                        .unwrap_or(0),
                    *r,
                )
            })
            .collect();
        counts.sort_by_key(|(n, r)| (*n, *r as u8));
        let mut out: ResourceBag = HashMap::new();
        let mut left = owed;
        while left > 0 {
            let Some((n, r)) = counts.pop() else { break };
            if n == 0 {
                break;
            }
            let take = n.min(left);
            bag::add(&mut out, r, take);
            left -= take;
        }
        out
    }

    /// Move the robber onto the richest opponent hex; steal from the leader.
    #[must_use]
    pub fn pick_robber(
        session: &GameSession,
        color: PlayerColor,
    ) -> Option<(u32, Option<PlayerColor>)> {
        let current = session.robber_hex()?;
        let mut hexes: Vec<(u32, u32)> = session
            .board
            .hexes
            .iter()
            .filter(|h| h.id != current)
            .map(|h| (robber_hex_score(session, color, h.id), h.id))
            .collect();
        hexes.sort();
        let (_, hex) = hexes.pop()?;
        let mut candidates: Vec<PlayerColor> = session
            .builders_on_hex(hex)
            .into_iter()
            .filter(|c| *c != color)
            .collect();
        if candidates.is_empty() {
            return Some((hex, None));
        }
        candidates.sort_by_key(|c| {
            let cards = session.player(*c).map(|p| p.resource_count()).unwrap_or(0);
            let points = session.victory_points(*c);
            (u32::MAX - cards, usize::MAX - points, *c as u8)
        });
        Some((hex, Some(candidates[0])))
    }

    // -- action phase -----------------------------------------------------

    /// Next build in priority order, if affordable now.
    #[must_use]
    pub fn next_build(session: &GameSession, color: PlayerColor) -> Option<PlannedAction> {
        let player = session.player(color)?;
        if player.cities_left() > 0
            && costs::can_afford(&player.resources, BuildKind::City)
            && !player.settlements.is_empty()
        {
            let mut cities: Vec<(u32, IntersectionId)> = player
                .settlements
                .iter()
                .map(|v| (intersection_pips(session, *v), *v))
                .collect();
            cities.sort();
            return cities.pop().map(|(_, v)| PlannedAction::BuildCity(v));
        }
        if player.settlements_left() > 0
            && costs::can_afford(&player.resources, BuildKind::Settlement)
        {
            let mut spots: Vec<(u32, IntersectionId)> = session
                .board
                .intersections
                .iter()
                .filter(|i| session.board.settlement_spot_ok(color, i.id, true))
                .map(|i| (setup_score(session, i.id), i.id))
                .collect();
            spots.sort();
            if let Some((_, v)) = spots.pop() {
                return Some(PlannedAction::BuildSettlement(v));
            }
        }
        if player.roads_left() > 0 && costs::can_afford(&player.resources, BuildKind::Road) {
            if let Some(edge) = best_road_edge(session, color) {
                return Some(PlannedAction::BuildRoad(edge));
            }
        }
        if !session.dev_deck.is_empty() && costs::can_afford(&player.resources, BuildKind::DevCard)
        {
            return Some(PlannedAction::BuyDev);
        }
        None
    }

    /// One bank/port trade toward the next unaffordable build, if possible.
    #[must_use]
    pub fn trade_to_afford(session: &GameSession, color: PlayerColor) -> Option<PlannedAction> {
        let player = session.player(color)?;
        // Cheapest-first build order we are saving toward.
        for kind in [
            BuildKind::City,
            BuildKind::Settlement,
            BuildKind::Road,
            BuildKind::DevCard,
        ] {
            if !piece_available(session, color, kind) {
                continue;
            }
            if let Some(missing) = missing_for(&player.resources, kind) {
                if let Some(action) = bank_or_port_offer(session, color, missing) {
                    return Some(action);
                }
            }
        }
        None
    }

    /// Opportunistic dev play (at most one non-VP per turn is enforced server-side).
    #[must_use]
    pub fn dev_to_play(session: &GameSession, color: PlayerColor) -> Option<PlannedAction> {
        if !matches!(session.phase, Phase::Production | Phase::Action) {
            return None;
        }
        if session.dev_played_this_turn {
            return None;
        }
        let player = session.player(color)?;
        let turn = session.turn_number;
        let playable = |kind: DevCardKind| player.playable(kind, turn) > 0;

        if playable(DevCardKind::Monopoly) {
            // Take a resource someone hoards that we need next.
            for kind in [
                BuildKind::City,
                BuildKind::Settlement,
                BuildKind::Road,
                BuildKind::DevCard,
            ] {
                if let Some(missing) = missing_for(&player.resources, kind) {
                    let mut holders: Vec<(u8, Resource)> = Resource::ALL
                        .iter()
                        .filter(|r| bag::count(&missing, **r) > 0)
                        .map(|r| (hoarded_by_others(session, color, *r), *r))
                        .collect();
                    holders.sort();
                    if let Some((held, resource)) = holders.pop() {
                        if held >= 2 {
                            return Some(PlannedAction::PlayMonopoly { resource });
                        }
                    }
                }
            }
        }
        if playable(DevCardKind::RoadBuilding) && player.roads_left() > 0 {
            let mut edges = Vec::new();
            if let Some(first) = best_road_edge(session, color) {
                edges.push(first);
                // Simulate the first road for the second pick.
                let mut scratch = session.clone();
                if let Some(p) = scratch.player_mut(color) {
                    p.roads.push(first);
                }
                if let Some(edge) = scratch.board.edge_mut(first) {
                    edge.road = Some(color);
                }
                if player.roads_left() > 1 {
                    if let Some(second) = best_road_edge(&scratch, color) {
                        edges.push(second);
                    }
                }
            }
            if !edges.is_empty() {
                return Some(PlannedAction::PlayRoadBuilding { edges });
            }
        }
        if playable(DevCardKind::Invention) {
            for kind in [
                BuildKind::City,
                BuildKind::Settlement,
                BuildKind::Road,
                BuildKind::DevCard,
            ] {
                if let Some(missing) = missing_for(&player.resources, kind) {
                    let mut want: Vec<Resource> = Vec::new();
                    for resource in Resource::ALL {
                        for _ in 0..bag::count(&missing, resource) {
                            if want.len() == 2 {
                                break;
                            }
                            if bag::count(&session.supply, resource) > 0 {
                                want.push(resource);
                            }
                        }
                    }
                    if !want.is_empty() {
                        while want.len() < 2 {
                            // Pad with anything the supply has.
                            if let Some(fill) = Resource::ALL
                                .iter()
                                .find(|r| bag::count(&session.supply, **r) > 0)
                            {
                                want.push(*fill);
                            } else {
                                break;
                            }
                        }
                        if want.len() == 2 {
                            return Some(PlannedAction::PlayInvention { resources: want });
                        }
                    }
                    break;
                }
            }
        }
        if playable(DevCardKind::Knight) {
            let richest = session
                .players
                .iter()
                .filter(|p| p.color != color)
                .map(|p| p.resource_count())
                .max()
                .unwrap_or(0);
            if richest >= 3 {
                if let Some((hex, victim)) = Self::pick_robber(session, color) {
                    return Some(PlannedAction::PlayKnight { hex, victim });
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Scoring helpers
// ---------------------------------------------------------------------------

/// Pip sum of hexes around an intersection.
fn intersection_pips(session: &GameSession, v: IntersectionId) -> u32 {
    session
        .board
        .hexes_adjacent_to(v)
        .iter()
        .map(|h| session.hex_pips(*h))
        .sum()
}

/// Setup desirability: production value + diversity bonus.
fn setup_score(session: &GameSession, v: IntersectionId) -> u32 {
    let hexes = session.board.hexes_adjacent_to(v);
    let pips: u32 = hexes.iter().map(|h| session.hex_pips(*h)).sum();
    let mut kinds = HashSet::new();
    for h in hexes {
        if let Some(hex) = session.board.hex(h) {
            kinds.insert(hex.field);
        }
    }
    pips + 2 * kinds.len() as u32
}

/// Robber target value: opponent production on the hex, weighted by cities
/// and the current leader.
fn robber_hex_score(session: &GameSession, me: PlayerColor, hex: u32) -> u32 {
    let pips = session.hex_pips(hex);
    let mut score: u32 = 0;
    for intersection in &session.board.intersections {
        if !intersection.hexes.contains(&hex) {
            continue;
        }
        if let Some(building) = intersection.building {
            if building.owner == me {
                score = score.saturating_sub(pips);
            } else {
                let weight = match building.kind {
                    crate::BuildingKind::Settlement => 1,
                    crate::BuildingKind::City => 2,
                };
                let leader_bonus = if is_leader(session, building.owner) {
                    2
                } else {
                    1
                };
                score += pips * weight * leader_bonus;
            }
        }
    }
    score
}

fn is_leader(session: &GameSession, color: PlayerColor) -> bool {
    let mine = session.victory_points(color);
    session
        .players
        .iter()
        .all(|p| session.victory_points(p.color) <= mine)
}

/// Legal road edge reaching toward the best future settlement spot.
fn best_road_edge(session: &GameSession, color: PlayerColor) -> Option<EdgeId> {
    let legal: Vec<EdgeId> = session
        .board
        .edges
        .iter()
        .filter(|e| session.board.road_spot_ok(color, e.id))
        .map(|e| e.id)
        .collect();
    if legal.is_empty() {
        return None;
    }
    // Best not-yet-reachable settlement target.
    let target = session
        .board
        .intersections
        .iter()
        .filter(|i| session.board.distance_rule_ok(i.id))
        .map(|i| (setup_score(session, i.id), i.id))
        .max();
    let Some((_, goal)) = target else {
        return legal.into_iter().min();
    };
    // BFS distances from the goal over intersections.
    let dist = bfs_distances(session, goal);
    legal
        .into_iter()
        .map(|e| {
            let edge = session.board.edge(e).expect("listed");
            let d = dist
                .get(&edge.a)
                .copied()
                .unwrap_or(u32::MAX)
                .min(dist.get(&edge.b).copied().unwrap_or(u32::MAX));
            (d, u32::MAX - e, e)
        })
        .min()
        .map(|(_, _, e)| e)
}

/// BFS hop distances from `goal` over the intersection graph.
fn bfs_distances(session: &GameSession, goal: IntersectionId) -> HashMap<IntersectionId, u32> {
    let mut dist = HashMap::new();
    let mut queue = VecDeque::from([goal]);
    dist.insert(goal, 0);
    while let Some(current) = queue.pop_front() {
        let here = dist[&current];
        for next in session.board.neighbors(current) {
            if dist.insert(next, here + 1).is_none() {
                queue.push_back(next);
            }
        }
    }
    dist
}

fn piece_available(session: &GameSession, color: PlayerColor, kind: BuildKind) -> bool {
    session.player(color).is_some_and(|p| match kind {
        BuildKind::Road => p.roads_left() > 0,
        BuildKind::Settlement => p.settlements_left() > 0,
        BuildKind::City => p.cities_left() > 0 && !p.settlements.is_empty(),
        BuildKind::DevCard => !session.dev_deck.is_empty(),
    })
}

/// First missing resource pile for `kind`, or `None` if affordable.
fn missing_for(hand: &ResourceBag, kind: BuildKind) -> Option<ResourceBag> {
    let price = costs::cost(kind);
    if bag::can_afford(hand, &price) {
        return None;
    }
    let mut missing = ResourceBag::new();
    for resource in Resource::ALL {
        let need = bag::count(&price, resource).saturating_sub(bag::count(hand, resource));
        if need > 0 {
            bag::add(&mut missing, resource, need);
        }
    }
    Some(missing)
}

/// One bank/port trade covering a missing card, if a ratio is available.
fn bank_or_port_offer(
    session: &GameSession,
    color: PlayerColor,
    missing: ResourceBag,
) -> Option<PlannedAction> {
    let player = session.player(color)?;
    let want_resource = Resource::ALL
        .iter()
        .find(|r| bag::count(&missing, **r) > 0)?;
    let want = bag::bag(&[(*want_resource, 1)]);
    let ports = session.board.ports_for(&player.settlements, &player.cities);
    let has_generic = ports.iter().any(|p| p.kind == crate::PortKind::Generic);
    // Prefer the cheapest ratio available.
    let mut options: Vec<(u8, TradeKind, Resource)> = Vec::new();
    for resource in Resource::ALL {
        let held = bag::count(&player.resources, resource);
        let specific = ports
            .iter()
            .any(|p| p.kind == crate::PortKind::Specific(resource));
        if specific && held >= 2 {
            options.push((2, TradeKind::Port, resource));
        } else if has_generic && held >= 3 {
            options.push((3, TradeKind::Port, resource));
        } else if held >= 4 {
            options.push((4, TradeKind::Bank, resource));
        }
    }
    options.sort();
    let (ratio, kind, resource) = options.into_iter().next()?;
    let give = bag::bag(&[(resource, ratio)]);
    // Never trade away the last card of something we also miss.
    if bag::count(&missing, resource) > 0 {
        return None;
    }
    match kind {
        TradeKind::Port => Some(PlannedAction::PortTrade { give, want }),
        TradeKind::Bank => Some(PlannedAction::BankTrade { give, want }),
        TradeKind::Player => None,
    }
}

/// Most cards of `resource` held by any single opponent.
fn hoarded_by_others(session: &GameSession, me: PlayerColor, resource: Resource) -> u8 {
    session
        .players
        .iter()
        .filter(|p| p.color != me)
        .map(|p| p.count_resource(resource))
        .max()
        .unwrap_or(0)
}
