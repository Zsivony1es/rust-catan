//! Authoritative game engine: pure functions over [`GameSession`].
//!
//! Every rule lives here so it is unit-testable without HTTP. Handlers in
//! `routes.rs` only do auth, locking and error mapping. All functions
//! validate-then-mutate and return [`GameError`] on illegal moves.

use std::collections::{HashMap, HashSet};

use rand::seq::SliceRandom as _;
use rand::RngExt as _;

use crate::model::{
    bag, costs, BuildKind, Building, BuildingKind, DevCardKind, EdgeId, IntersectionId,
    PlayerColor, Resource, ResourceBag, TradeOffer,
};
use crate::{
    BonusKind, GameError, GameEventKind, GameSession, Phase, KNIGHTS_FOR_ARMY, POINTS_TO_WIN,
    ROADS_FOR_ROUTE,
};

/// Parameters for development card effects.
#[derive(Debug, Clone, Default)]
pub struct DevPlayParams {
    /// Knight: hex to move the robber to (must differ from current).
    pub hex_id: Option<u32>,
    /// Knight / robber: victim to steal from when several are eligible.
    pub victim: Option<PlayerColor>,
    /// Monopoly: resource to take from everyone.
    pub resource: Option<Resource>,
    /// Invention: exactly 2 resources to take from the supply.
    pub resources: Vec<Resource>,
    /// Road Building: 1-2 edges for the free roads.
    pub edges: Vec<EdgeId>,
}

/// What to build (engine-level; JSON DTOs map onto this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    Road(EdgeId),
    Settlement(IntersectionId),
    City(IntersectionId),
    DevCard,
}

impl BuildTarget {
    #[must_use]
    pub const fn kind(self) -> BuildKind {
        match self {
            Self::Road(_) => BuildKind::Road,
            Self::Settlement(_) => BuildKind::Settlement,
            Self::City(_) => BuildKind::City,
            Self::DevCard => BuildKind::DevCard,
        }
    }
}

// ---------------------------------------------------------------------------
// Lobby
// ---------------------------------------------------------------------------

/// Seat `color` at a lobby game.
pub fn join(session: &mut GameSession, color: PlayerColor) -> Result<(), GameError> {
    if session.phase != Phase::Lobby {
        return Err(GameError::conflict("game already started"));
    }
    if session.has_player(color) {
        return Err(GameError::conflict(format!("{color:?} is already taken")));
    }
    if session.players.len() >= 4 {
        return Err(GameError::conflict("game is full (4 players)"));
    }
    session.players.push(crate::model::Player::new(color));
    session.push_log(GameEventKind::PlayerJoined { color });
    Ok(())
}

/// Roll 2d6 for turn order (highest first, ties re-rolled) and enter setup.
pub fn start(session: &mut GameSession, rng: &mut impl rand::Rng) -> Result<(), GameError> {
    if session.phase != Phase::Lobby {
        return Err(GameError::conflict("game already started"));
    }
    if session.players.len() < 2 {
        return Err(GameError::bad_request("need 2-4 players to start"));
    }
    let mut remaining: Vec<PlayerColor> = session.players.iter().map(|p| p.color).collect();
    let mut order = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let first = roll_off_winner(&remaining, rng);
        order.push(first);
        remaining.retain(|c| *c != first);
    }
    session.order = order.clone();
    session.turn_index = 0;
    session.turn_number = 0;
    session.phase = Phase::SetupRound1;
    session.push_log(GameEventKind::GameStarted { order });
    Ok(())
}

fn roll_die(rng: &mut impl rand::Rng) -> u8 {
    rng.random_range(1..=6)
}

/// Highest 2d6 roll wins; tied contenders re-roll until one leads.
/// (Rerolls are capped, then shuffled, to guarantee termination.)
fn roll_off_winner(group: &[PlayerColor], rng: &mut impl rand::Rng) -> PlayerColor {
    let mut contenders = group.to_vec();
    for _ in 0..100 {
        let mut best: u8 = 0;
        let mut tied: Vec<PlayerColor> = Vec::new();
        for color in &contenders {
            let total = roll_die(rng) + roll_die(rng);
            if total > best {
                best = total;
                tied.clear();
                tied.push(*color);
            } else if total == best {
                tied.push(*color);
            }
        }
        if tied.len() == 1 {
            return tied[0];
        }
        contenders = tied;
    }
    contenders.shuffle(rng);
    contenders[0]
}

/// Index of `color` in `session.players`, if seated.
fn player_index(session: &GameSession, color: PlayerColor) -> Option<usize> {
    session.players.iter().position(|p| p.color == color)
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// Place one settlement + its adjacent road during a setup round.
pub fn setup_place(
    session: &mut GameSession,
    color: PlayerColor,
    intersection: IntersectionId,
    edge: EdgeId,
) -> Result<(), GameError> {
    if !matches!(session.phase, Phase::SetupRound1 | Phase::SetupRound2) {
        return Err(GameError::bad_request("not in setup phase"));
    }
    let expected = session.setup_expected_color();
    if expected != Some(color) {
        return Err(GameError::forbidden(format!(
            "expected {expected:?}, got {color:?}"
        )));
    }
    if session.board.intersection(intersection).is_none() {
        return Err(GameError::not_found(format!("intersection {intersection}")));
    }
    let board_edge = session
        .board
        .edge(edge)
        .ok_or_else(|| GameError::not_found(format!("edge {edge}")))?;
    if board_edge.road.is_some() {
        return Err(GameError::bad_request("edge already has a road"));
    }
    if board_edge.a != intersection && board_edge.b != intersection {
        return Err(GameError::bad_request(
            "setup road must touch the placed settlement",
        ));
    }
    if !session.board.distance_rule_ok(intersection) {
        return Err(GameError::bad_request(
            "distance rule: too close to another building",
        ));
    }
    let idx = player_index(session, color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;

    session
        .board
        .intersection_mut(intersection)
        .expect("checked")
        .building = Some(Building {
        owner: color,
        kind: BuildingKind::Settlement,
    });
    session.board.edge_mut(edge).expect("checked").road = Some(color);
    session.players[idx].settlements.push(intersection);
    session.players[idx].roads.push(edge);
    session.setup_done += 1;
    session.push_log(GameEventKind::SetupPlaced {
        color,
        intersection,
        edge,
    });

    let n = session.order.len();
    if session.setup_done == n {
        session.phase = Phase::SetupRound2;
    } else if session.setup_done == 2 * n {
        grant_starting_resources(session);
        session.phase = Phase::Production;
        session.turn_index = 0;
        session.turn_number = 1;
    }
    Ok(())
}

/// Each player collects 1 resource per hex adjacent to their 2nd settlement.
fn grant_starting_resources(session: &mut GameSession) {
    for color in session.order.clone() {
        let second = session
            .player(color)
            .and_then(|p| p.settlements.get(1).copied());
        let Some(home) = second else { continue };
        let hexes = session.board.hexes_adjacent_to(home);
        let mut gained: ResourceBag = HashMap::new();
        for hex_id in hexes {
            let produces = session.board.hex(hex_id).and_then(|h| h.field.produces());
            if let Some(resource) = produces {
                if bag::take(&mut session.supply, resource, 1) {
                    bag::add(&mut gained, resource, 1);
                }
            }
        }
        if let Some(player) = session.player_mut(color) {
            bag::add_all(&mut player.resources, &gained);
        }
        session.push_log(GameEventKind::StartingResources {
            color,
            cards: gained,
        });
    }
}

// ---------------------------------------------------------------------------
// Turn loop: roll / production / 7 / discard / robber
// ---------------------------------------------------------------------------

/// Roll 2d6: pay out production (2-12) or open 7-resolution.
pub fn roll(session: &mut GameSession, rng: &mut impl rand::Rng) -> Result<(u8, u8), GameError> {
    require_active(session, None)?;
    if session.phase != Phase::Production {
        return Err(GameError::bad_request("already rolled this turn"));
    }
    if session.seven_pending() {
        return Err(GameError::conflict("resolve the robber first"));
    }
    let (die1, die2) = (roll_die(rng), roll_die(rng));
    let total = die1 + die2;
    let active = session.active_color().expect("checked");
    session.dice = Some((die1, die2));
    session.push_log(GameEventKind::DiceRolled {
        color: active,
        die1,
        die2,
    });
    if total == 7 {
        open_seven(session);
    } else {
        apply_production(session, total);
        session.phase = Phase::Action;
    }
    Ok((die1, die2))
}

/// Everyone past 7 cards owes half (rounded down); then the robber moves.
fn open_seven(session: &mut GameSession) {
    let owed: Vec<(PlayerColor, u8)> = session
        .players
        .iter()
        .filter_map(|player| {
            let amount = player.discard_owed();
            (amount > 0).then_some((player.color, amount))
        })
        .collect();
    for (color, amount) in owed {
        session.pending_discards.insert(color, amount);
        session.push_log(GameEventKind::DiscardOwed { color, amount });
    }
    session.robber_pending = true;
}

/// Pay production for `total`: settlement = 1x, city = 2x per adjacent
/// matching hex. Robber hex is blocked. Shortage rule: if the supply cannot
/// cover everyone owed a resource, nobody gets that resource; a lone
/// affected player takes what remains.
fn apply_production(session: &mut GameSession, total: u8) {
    // (resource -> [(color, amount)])
    let mut owed: HashMap<Resource, Vec<(PlayerColor, u8)>> = HashMap::new();
    for hex in session.board.hexes.clone() {
        if hex.number != Some(total) || hex.has_robber {
            continue;
        }
        let Some(resource) = hex.field.produces() else {
            continue;
        };
        for intersection in &session.board.intersections {
            if !intersection.hexes.contains(&hex.id) {
                continue;
            }
            if let Some(building) = intersection.building {
                let amount = match building.kind {
                    BuildingKind::Settlement => 1,
                    BuildingKind::City => 2,
                };
                owed.entry(resource)
                    .or_default()
                    .push((building.owner, amount));
            }
        }
    }
    let mut paid: HashMap<PlayerColor, ResourceBag> = HashMap::new();
    for (resource, claims) in &owed {
        let needed: u32 = claims.iter().map(|(_, a)| u32::from(*a)).sum();
        let available = u32::from(bag::count(&session.supply, *resource));
        if available >= needed {
            for (color, amount) in claims {
                bag::add(paid.entry(*color).or_default(), *resource, *amount);
                let ok = bag::take(&mut session.supply, *resource, *amount);
                debug_assert!(ok);
            }
        } else {
            let parties: HashSet<PlayerColor> = claims.iter().map(|(c, _)| *c).collect();
            if parties.len() == 1 {
                let color = claims[0].0;
                let got = available.min(needed) as u8;
                if got > 0 {
                    bag::add(paid.entry(color).or_default(), *resource, got);
                    let ok = bag::take(&mut session.supply, *resource, got);
                    debug_assert!(ok);
                }
            }
            // Else: shortage — nobody gets this resource.
        }
    }
    for (color, cards) in paid {
        if let Some(player) = session.player_mut(color) {
            bag::add_all(&mut player.resources, &cards);
        }
        session.push_log(GameEventKind::Produced { color, cards });
    }
}

/// Discard `cards` toward a player's 7-penalty.
pub fn discard(
    session: &mut GameSession,
    color: PlayerColor,
    cards: &ResourceBag,
) -> Result<(), GameError> {
    let owed = session
        .pending_discards
        .get(&color)
        .copied()
        .ok_or_else(|| GameError::bad_request(format!("{color:?} owes no discard")))?;
    if bag::total(cards) != u32::from(owed) {
        return Err(GameError::bad_request(format!(
            "must discard exactly {owed} cards"
        )));
    }
    let player = session
        .player_mut(color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
    if !bag::can_afford(&player.resources, cards) {
        return Err(GameError::bad_request("discard exceeds hand"));
    }
    let ok = bag::take_all(&mut player.resources, cards);
    debug_assert!(ok);
    bag::add_all(&mut session.supply, cards);
    session.pending_discards.remove(&color);
    session.push_log(GameEventKind::Discarded {
        color,
        cards: cards.clone(),
    });
    Ok(())
}

/// Move the robber after a 7 (discards must be done) and steal.
pub fn robber(
    session: &mut GameSession,
    rng: &mut impl rand::Rng,
    hex_id: u32,
    victim: Option<PlayerColor>,
) -> Result<(), GameError> {
    let active = require_active(session, None)?;
    if !session.robber_pending {
        return Err(GameError::bad_request("no robber move pending"));
    }
    if !session.pending_discards.is_empty() {
        return Err(GameError::conflict("discards still pending"));
    }
    move_robber_and_steal(session, rng, active, hex_id, victim)?;
    session.robber_pending = false;
    session.phase = Phase::Action;
    Ok(())
}

/// Shared robber move + steal used by dice-7 and Knight cards.
fn move_robber_and_steal(
    session: &mut GameSession,
    rng: &mut impl rand::Rng,
    active: PlayerColor,
    hex_id: u32,
    victim: Option<PlayerColor>,
) -> Result<(), GameError> {
    if session.board.hex(hex_id).is_none() {
        return Err(GameError::not_found(format!("hex {hex_id}")));
    }
    if session.robber_hex() == Some(hex_id) {
        return Err(GameError::bad_request("robber must move to a new hex"));
    }
    for hex in &mut session.board.hexes {
        hex.has_robber = hex.id == hex_id;
    }
    session.push_log(GameEventKind::RobberMoved {
        color: active,
        hex: hex_id,
    });

    let mut candidates: Vec<PlayerColor> = session
        .builders_on_hex(hex_id)
        .into_iter()
        .filter(|c| *c != active)
        .collect();
    candidates.sort_by_key(|c| *c as u8);
    if candidates.is_empty() {
        session.push_log(GameEventKind::Stolen {
            from: active,
            to: active,
            resource: None,
        });
        return Ok(());
    }
    let mark = match victim {
        Some(v) => {
            if !candidates.contains(&v) {
                return Err(GameError::bad_request(format!(
                    "{v:?} has no building on hex {hex_id}"
                )));
            }
            v
        }
        None => {
            if candidates.len() > 1 {
                return Err(GameError::bad_request("choose a victim to steal from"));
            }
            candidates[0]
        }
    };
    let stolen = steal_random_card(session, rng, active, mark);
    session.push_log(GameEventKind::Stolen {
        from: mark,
        to: active,
        resource: stolen,
    });
    Ok(())
}

/// Steal one random resource card from `mark` for `active` (none if empty).
fn steal_random_card(
    session: &mut GameSession,
    rng: &mut impl rand::Rng,
    active: PlayerColor,
    mark: PlayerColor,
) -> Option<Resource> {
    let hand: Vec<Resource> = {
        let player = session.player(mark)?;
        let mut cards = Vec::new();
        for resource in Resource::ALL {
            cards.extend(std::iter::repeat_n(
                resource,
                usize::from(player.count_resource(resource)),
            ));
        }
        cards
    };
    if hand.is_empty() {
        return None;
    }
    let resource = hand[rng.random_range(0..hand.len())];
    let ok = bag::take(&mut session.player_mut(mark)?.resources, resource, 1);
    debug_assert!(ok);
    bag::add(&mut session.player_mut(active)?.resources, resource, 1);
    Some(resource)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// Build (or place for free via Road Building) during the action phase.
pub fn build(
    session: &mut GameSession,
    color: PlayerColor,
    target: BuildTarget,
    free: bool,
) -> Result<(), GameError> {
    if session.winner.is_some() {
        return Err(GameError::conflict("game is over"));
    }
    // Free placements (Road Building) may also land pre-roll, but always on
    // the active player's own turn.
    require_active(session, Some(color))?;
    if free {
        if !matches!(session.phase, Phase::Production | Phase::Action) {
            return Err(GameError::bad_request("cannot build now"));
        }
    } else if session.phase != Phase::Action {
        return Err(GameError::bad_request("build during the action phase"));
    }
    match target {
        BuildTarget::Road(edge) => build_road(session, color, edge, free),
        BuildTarget::Settlement(v) => build_settlement(session, color, v),
        BuildTarget::City(v) => build_city(session, color, v),
        BuildTarget::DevCard => buy_dev_card(session, color),
    }?;
    update_longest_route(session);
    check_winner(session);
    Ok(())
}

fn build_road(
    session: &mut GameSession,
    color: PlayerColor,
    edge: EdgeId,
    free: bool,
) -> Result<(), GameError> {
    if session.board.edge(edge).is_none() {
        return Err(GameError::not_found(format!("edge {edge}")));
    }
    if !session.board.road_spot_ok(color, edge) {
        return Err(GameError::bad_request(
            "road must connect to your network on an empty edge",
        ));
    }
    let player = session
        .player(color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
    if player.roads_left() == 0 {
        return Err(GameError::conflict("no road pieces left"));
    }
    if !free && !bag::can_afford(&player.resources, &costs::cost(BuildKind::Road)) {
        return Err(GameError::bad_request("cannot afford a road"));
    }
    let idx = player_index(session, color).expect("player checked");
    if !free {
        let player = &mut session.players[idx];
        let ok = costs::pay(&mut player.resources, &mut session.supply, BuildKind::Road);
        debug_assert!(ok);
    }
    session.players[idx].roads.push(edge);
    session.board.edge_mut(edge).expect("checked").road = Some(color);
    session.push_log(GameEventKind::BuiltRoad { color, edge });
    Ok(())
}

fn build_settlement(
    session: &mut GameSession,
    color: PlayerColor,
    v: IntersectionId,
) -> Result<(), GameError> {
    if session.board.intersection(v).is_none() {
        return Err(GameError::not_found(format!("intersection {v}")));
    }
    if !session.board.distance_rule_ok(v) {
        return Err(GameError::bad_request(
            "distance rule: too close to another building",
        ));
    }
    if !session.board.settlement_spot_ok(color, v, true) {
        return Err(GameError::bad_request(
            "settlement must touch one of your roads",
        ));
    }
    let player = session
        .player(color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
    if player.settlements_left() == 0 {
        return Err(GameError::conflict(
            "no settlement pieces left (upgrade to a city first)",
        ));
    }
    if !bag::can_afford(&player.resources, &costs::cost(BuildKind::Settlement)) {
        return Err(GameError::bad_request("cannot afford a settlement"));
    }
    let idx = player_index(session, color).expect("player checked");
    {
        let player = &mut session.players[idx];
        let ok = costs::pay(
            &mut player.resources,
            &mut session.supply,
            BuildKind::Settlement,
        );
        debug_assert!(ok);
        player.settlements.push(v);
    }
    session.board.intersection_mut(v).expect("checked").building = Some(Building {
        owner: color,
        kind: BuildingKind::Settlement,
    });
    session.push_log(GameEventKind::BuiltSettlement {
        color,
        intersection: v,
    });
    Ok(())
}

fn build_city(
    session: &mut GameSession,
    color: PlayerColor,
    v: IntersectionId,
) -> Result<(), GameError> {
    let node = session
        .board
        .intersection(v)
        .ok_or_else(|| GameError::not_found(format!("intersection {v}")))?;
    if node.building
        != Some(Building {
            owner: color,
            kind: BuildingKind::Settlement,
        })
    {
        return Err(GameError::bad_request(
            "cities upgrade one of your settlements",
        ));
    }
    let player = session
        .player(color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
    if player.cities_left() == 0 {
        return Err(GameError::conflict("no city pieces left"));
    }
    if !bag::can_afford(&player.resources, &costs::cost(BuildKind::City)) {
        return Err(GameError::bad_request("cannot afford a city"));
    }
    let idx = player_index(session, color).expect("player checked");
    {
        let player = &mut session.players[idx];
        let ok = costs::pay(&mut player.resources, &mut session.supply, BuildKind::City);
        debug_assert!(ok);
        player.settlements.retain(|s| *s != v);
        player.cities.push(v);
    }
    session.board.intersection_mut(v).expect("checked").building = Some(Building {
        owner: color,
        kind: BuildingKind::City,
    });
    session.push_log(GameEventKind::BuiltCity {
        color,
        intersection: v,
    });
    Ok(())
}

fn buy_dev_card(session: &mut GameSession, color: PlayerColor) -> Result<(), GameError> {
    if session.dev_deck.is_empty() {
        return Err(GameError::conflict("development deck is empty"));
    }
    {
        let player = session
            .player(color)
            .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
        if !bag::can_afford(&player.resources, &costs::cost(BuildKind::DevCard)) {
            return Err(GameError::bad_request("cannot afford a development card"));
        }
    }
    let turn_number = session.turn_number;
    let kind = session.dev_deck.pop().expect("checked");
    let idx = player_index(session, color).expect("player checked");
    {
        let player = &mut session.players[idx];
        let ok = costs::pay(
            &mut player.resources,
            &mut session.supply,
            BuildKind::DevCard,
        );
        debug_assert!(ok);
        player
            .dev_hand
            .push(crate::model::DevCardInstance::new(kind, turn_number));
    }
    session.push_log(GameEventKind::DevBought { color });
    Ok(())
}

// ---------------------------------------------------------------------------
// Trading
// ---------------------------------------------------------------------------

/// Bank trade at 4:1.
pub fn trade_bank(
    session: &mut GameSession,
    color: PlayerColor,
    give: &ResourceBag,
    want: &ResourceBag,
) -> Result<(), GameError> {
    require_active(session, Some(color))?;
    require_action_phase(session)?;
    let offer = TradeOffer::validate_fixed_ratio(color, give.clone(), want.clone(), 4)
        .map_err(GameError::bad_request)?;
    execute_fixed_trade(session, color, &offer, crate::model::TradeKind::Bank)
}

/// Port trade at 3:1 (generic) or 2:1 (matching specific port).
pub fn trade_port(
    session: &mut GameSession,
    color: PlayerColor,
    give: &ResourceBag,
    want: &ResourceBag,
) -> Result<(), GameError> {
    require_active(session, Some(color))?;
    require_action_phase(session)?;
    let player = session
        .player(color)
        .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
    let ports = session.board.ports_for(&player.settlements, &player.cities);
    if give.len() != 1 {
        return Err(GameError::bad_request("port trade gives one resource type"));
    }
    let (resource, _) = give.iter().next().expect("checked");
    let mut ratio: Option<u8> = None;
    for port in &ports {
        match port.kind {
            crate::model::PortKind::Specific(r) if r == *resource => {
                ratio = Some(2);
                break;
            }
            crate::model::PortKind::Generic => {
                ratio = Some(ratio.unwrap_or(3));
            }
            _ => {}
        }
    }
    let Some(ratio) = ratio else {
        return Err(GameError::forbidden("no port available for this trade"));
    };
    let offer = TradeOffer::validate_fixed_ratio(color, give.clone(), want.clone(), ratio)
        .map_err(GameError::bad_request)?;
    execute_fixed_trade(session, color, &offer, crate::model::TradeKind::Port)
}

fn execute_fixed_trade(
    session: &mut GameSession,
    color: PlayerColor,
    offer: &TradeOffer,
    kind: crate::model::TradeKind,
) -> Result<(), GameError> {
    {
        let player = session
            .player(color)
            .ok_or_else(|| GameError::not_found(format!("player {color:?}")))?;
        if !bag::can_afford(&player.resources, &offer.give) {
            return Err(GameError::bad_request("trade exceeds hand"));
        }
    }
    if !bag::can_afford(&session.supply, &offer.want) {
        return Err(GameError::conflict("supply lacks the wanted card"));
    }
    let idx = player_index(session, color).expect("player checked");
    {
        let player = &mut session.players[idx];
        let ok = bag::take_all(&mut player.resources, &offer.give);
        debug_assert!(ok);
    }
    bag::add_all(&mut session.supply, &offer.give);
    {
        let ok = bag::take_all(&mut session.supply, &offer.want);
        debug_assert!(ok);
    }
    bag::add_all(&mut session.players[idx].resources, &offer.want);
    session.push_log(GameEventKind::Traded {
        color,
        kind,
        give: offer.give.clone(),
        want: offer.want.clone(),
    });
    Ok(())
}

/// Propose a player-to-player trade; returns the offer id for accept/decline.
pub fn propose_trade(
    session: &mut GameSession,
    from: PlayerColor,
    to: PlayerColor,
    give: &ResourceBag,
    want: &ResourceBag,
) -> Result<String, GameError> {
    require_active(session, Some(from))?;
    require_action_phase(session)?;
    if from == to {
        return Err(GameError::bad_request("cannot trade with yourself"));
    }
    if session.player(to).is_none() {
        return Err(GameError::not_found(format!("player {to:?}")));
    }
    let offer =
        TradeOffer::validate(from, give.clone(), want.clone()).map_err(GameError::bad_request)?;
    let proposer = session.player(from).expect("checked");
    if !bag::can_afford(&proposer.resources, &offer.give) {
        return Err(GameError::bad_request("trade exceeds hand"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    session.pending_trades.push(crate::model::PendingTrade {
        id: id.clone(),
        from,
        to,
        give: offer.give,
        want: offer.want,
    });
    session.push_log(GameEventKind::TradeProposed {
        id: id.clone(),
        from,
        to,
    });
    Ok(id)
}

/// Accept a pending trade as the counterparty; cards swap atomically.
pub fn accept_trade(
    session: &mut GameSession,
    color: PlayerColor,
    trade_id: &str,
) -> Result<(), GameError> {
    let index = session
        .pending_trades
        .iter()
        .position(|t| t.id == trade_id)
        .ok_or_else(|| GameError::not_found(format!("trade {trade_id}")))?;
    let trade = session.pending_trades[index].clone();
    if trade.to != color {
        return Err(GameError::forbidden("only the counterparty can accept"));
    }
    // Offers die when the proposer's turn ends.
    if session.active_color() != Some(trade.from) || session.phase != Phase::Action {
        session.pending_trades.remove(index);
        return Err(GameError::conflict("offer expired"));
    }
    let proposer_has = session
        .player(trade.from)
        .is_some_and(|p| bag::can_afford(&p.resources, &trade.give));
    let counter_has = session
        .player(trade.to)
        .is_some_and(|p| bag::can_afford(&p.resources, &trade.want));
    if !proposer_has || !counter_has {
        session.pending_trades.remove(index);
        return Err(GameError::conflict("hands changed; offer withdrawn"));
    }
    for (owner, take_cards, give_cards) in [
        (trade.from, &trade.give, &trade.want),
        (trade.to, &trade.want, &trade.give),
    ] {
        let player = session.player_mut(owner).expect("checked");
        let ok = bag::take_all(&mut player.resources, take_cards);
        debug_assert!(ok);
        bag::add_all(&mut player.resources, give_cards);
    }
    session.pending_trades.remove(index);
    session.push_log(GameEventKind::TradeAccepted {
        id: trade.id.clone(),
    });
    session.push_log(GameEventKind::Traded {
        color: trade.from,
        kind: crate::model::TradeKind::Player,
        give: trade.give,
        want: trade.want,
    });
    Ok(())
}

/// Decline (or withdraw) a pending trade.
pub fn decline_trade(
    session: &mut GameSession,
    color: PlayerColor,
    trade_id: &str,
) -> Result<(), GameError> {
    let index = session
        .pending_trades
        .iter()
        .position(|t| t.id == trade_id)
        .ok_or_else(|| GameError::not_found(format!("trade {trade_id}")))?;
    let trade = session.pending_trades[index].clone();
    if color != trade.from && color != trade.to {
        return Err(GameError::forbidden("not a party to this trade"));
    }
    session.pending_trades.remove(index);
    session.push_log(GameEventKind::TradeDeclined { id: trade.id });
    Ok(())
}

// ---------------------------------------------------------------------------
// Development cards
// ---------------------------------------------------------------------------

/// Play a development card (pre-roll or in the action phase).
pub fn play_dev(
    session: &mut GameSession,
    rng: &mut impl rand::Rng,
    color: PlayerColor,
    kind: DevCardKind,
    params: &DevPlayParams,
) -> Result<(), GameError> {
    if session.winner.is_some() {
        return Err(GameError::conflict("game is over"));
    }
    let active = require_active(session, Some(color))?;
    if !matches!(session.phase, Phase::Production | Phase::Action) {
        return Err(GameError::bad_request("cannot play cards now"));
    }
    if session.phase == Phase::Production && session.seven_pending() {
        return Err(GameError::conflict("resolve the robber first"));
    }
    let hand_index = session
        .player(color)
        .and_then(|p| {
            p.dev_hand.iter().position(|c| {
                c.kind == kind && kind.playable_this_turn(c.built_turn, session.turn_number)
            })
        })
        .ok_or_else(|| {
            if session
                .player(color)
                .is_some_and(|p| p.dev_hand.iter().any(|c| c.kind == kind))
            {
                GameError::bad_request("card was bought this turn or already played one")
            } else {
                GameError::bad_request(format!("no playable {kind:?} card"))
            }
        })?;
    if kind != DevCardKind::VictoryPoint {
        if session.dev_played_this_turn {
            return Err(GameError::bad_request("already played a card this turn"));
        }
        // Remove up front so effects see the post-play hand.
        session
            .player_mut(color)
            .expect("checked")
            .dev_hand
            .remove(hand_index);
    }

    match kind {
        DevCardKind::Knight => {
            let hex_id = params
                .hex_id
                .ok_or_else(|| GameError::bad_request("knight needs hex_id"))?;
            move_robber_and_steal(session, rng, active, hex_id, params.victim)?;
            if let Some(player) = session.player_mut(color) {
                player.played_knights += 1;
            }
            update_largest_army(session);
        }
        DevCardKind::VictoryPoint => {
            // Counted automatically in victory_points; reveal for the log.
        }
        DevCardKind::Monopoly => {
            let resource = params
                .resource
                .ok_or_else(|| GameError::bad_request("monopoly needs resource"))?;
            let mut taken: ResourceBag = HashMap::new();
            for player in &mut session.players {
                if player.color == color {
                    continue;
                }
                let amount = player.count_resource(resource);
                if amount > 0 {
                    let ok = bag::take(&mut player.resources, resource, amount);
                    debug_assert!(ok);
                    bag::add(&mut taken, resource, amount);
                }
            }
            if let Some(player) = session.player_mut(color) {
                bag::add_all(&mut player.resources, &taken);
            }
            if !taken.is_empty() {
                session.push_log(GameEventKind::Produced {
                    color,
                    cards: taken,
                });
            }
        }
        DevCardKind::RoadBuilding => {
            if params.edges.is_empty() || params.edges.len() > 2 {
                // The card was removed up front; put it back.
                restore_card(session, color, hand_index, kind);
                return Err(GameError::bad_request("road building places 1-2 roads"));
            }
            // Validate on a scratch board so failures leave no half-built roads.
            let mut scratch = session.board.clone();
            let roads_built = session.player(color).map(|p| p.roads.len()).unwrap_or(0);
            if roads_built + params.edges.len() > crate::model::MAX_ROADS {
                restore_card(session, color, hand_index, kind);
                return Err(GameError::conflict("not enough road pieces left"));
            }
            for edge in &params.edges {
                if scratch.edge(*edge).is_none() {
                    restore_card(session, color, hand_index, kind);
                    return Err(GameError::not_found(format!("edge {edge}")));
                }
                if !scratch.road_spot_ok(color, *edge) {
                    restore_card(session, color, hand_index, kind);
                    return Err(GameError::bad_request(format!(
                        "road on edge {edge} is not legal"
                    )));
                }
                scratch.edge_mut(*edge).expect("checked").road = Some(color);
            }
            for edge in &params.edges {
                session.board.edge_mut(*edge).expect("checked").road = Some(color);
                if let Some(player) = session.player_mut(color) {
                    player.roads.push(*edge);
                }
                session.push_log(GameEventKind::BuiltRoad { color, edge: *edge });
            }
            update_longest_route(session);
        }
        DevCardKind::Invention => {
            if params.resources.len() != 2 {
                restore_card(session, color, hand_index, kind);
                return Err(GameError::bad_request(
                    "invention takes exactly 2 resources",
                ));
            }
            let mut taken: ResourceBag = HashMap::new();
            for resource in &params.resources {
                if bag::take(&mut session.supply, *resource, 1) {
                    bag::add(&mut taken, *resource, 1);
                }
            }
            if let Some(player) = session.player_mut(color) {
                bag::add_all(&mut player.resources, &taken);
            }
            if !taken.is_empty() {
                session.push_log(GameEventKind::Produced {
                    color,
                    cards: taken,
                });
            }
        }
    }

    session.push_log(GameEventKind::DevPlayed { color, kind });
    if kind != DevCardKind::VictoryPoint {
        session.dev_played_this_turn = true;
    }
    check_winner(session);
    Ok(())
}

/// Put back a dev card removed up front when its effect fails validation.
fn restore_card(session: &mut GameSession, color: PlayerColor, index: usize, kind: DevCardKind) {
    let turn_number = session.turn_number;
    if let Some(player) = session.player_mut(color) {
        let at = index.min(player.dev_hand.len());
        player
            .dev_hand
            .insert(at, crate::model::DevCardInstance::new(kind, turn_number));
    }
}

// ---------------------------------------------------------------------------
// Bonuses, winning, turn end
// ---------------------------------------------------------------------------

/// Re-award the Longest Route tile (first to 5+, stolen on strictly more,
/// back to the supply below 5 or on a broken tie).
fn update_longest_route(session: &mut GameSession) {
    let mut lengths: HashMap<PlayerColor, usize> = HashMap::new();
    for player in &session.players {
        lengths.insert(player.color, session.board.longest_road(player.color));
    }
    let best = lengths.values().copied().max().unwrap_or(0);
    let leaders: Vec<PlayerColor> = lengths
        .iter()
        .filter(|(_, len)| **len == best)
        .map(|(c, _)| *c)
        .collect();
    let next = match session.longest_route_holder {
        Some(holder) => {
            let held = lengths.get(&holder).copied().unwrap_or(0);
            if held >= ROADS_FOR_ROUTE && held >= best {
                Some(holder) // ties keep the tile
            } else if best >= ROADS_FOR_ROUTE && leaders.len() == 1 {
                Some(leaders[0])
            } else {
                None
            }
        }
        None => {
            if best >= ROADS_FOR_ROUTE && leaders.len() == 1 {
                Some(leaders[0])
            } else {
                None
            }
        }
    };
    if next != session.longest_route_holder {
        session.longest_route_holder = next;
        session.push_log(GameEventKind::Bonus {
            kind: BonusKind::LongestRoute,
            holder: next,
        });
    }
}

/// Re-award the Largest Army tile (first to 3 knights, stolen on strictly more).
fn update_largest_army(session: &mut GameSession) {
    let mut best_color: Option<PlayerColor> = None;
    let mut best_count: u8 = 0;
    for player in &session.players {
        if player.played_knights > best_count {
            best_count = player.played_knights;
            best_color = Some(player.color);
        }
    }
    let next = match session.largest_army_holder {
        Some(holder) => {
            let held = session
                .player(holder)
                .map(|p| p.played_knights)
                .unwrap_or(0);
            if held >= best_count && held >= KNIGHTS_FOR_ARMY {
                Some(holder) // ties keep the tile
            } else {
                best_color.filter(|_| best_count >= KNIGHTS_FOR_ARMY)
            }
        }
        None => best_color.filter(|_| best_count >= KNIGHTS_FOR_ARMY),
    };
    if next != session.largest_army_holder {
        session.largest_army_holder = next;
        session.push_log(GameEventKind::Bonus {
            kind: BonusKind::LargestArmy,
            holder: next,
        });
    }
}

/// First to 10+ VPs on their own turn wins immediately.
fn check_winner(session: &mut GameSession) {
    if session.winner.is_some() {
        return;
    }
    if !matches!(session.phase, Phase::Production | Phase::Action) {
        return;
    }
    let Some(active) = session.active_color() else {
        return;
    };
    let points = session.victory_points(active);
    if points >= POINTS_TO_WIN {
        session.winner = Some(active);
        session.phase = Phase::GameOver;
        session.push_log(GameEventKind::Won {
            color: active,
            points,
        });
    }
}

/// Pass the dice left.
pub fn end_turn(session: &mut GameSession) -> Result<(), GameError> {
    let active = require_active(session, None)?;
    if session.phase != Phase::Action {
        return Err(GameError::bad_request("roll before ending your turn"));
    }
    if session.seven_pending() {
        return Err(GameError::conflict("resolve the robber first"));
    }
    // Pending player-trade offers die with the turn.
    for trade in std::mem::take(&mut session.pending_trades) {
        session.push_log(GameEventKind::TradeDeclined { id: trade.id });
    }
    let n = session.order.len();
    session.turn_index = (session.turn_index + 1) % n.max(1);
    session.turn_number += 1;
    session.dice = None;
    session.dev_played_this_turn = false;
    session.phase = Phase::Production;
    session.push_log(GameEventKind::TurnEnded { color: active });
    Ok(())
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// Game must be running; returns the active color. When `color` is given it
/// must be the active player.
fn require_active(
    session: &GameSession,
    color: Option<PlayerColor>,
) -> Result<PlayerColor, GameError> {
    if session.winner.is_some() || session.phase == Phase::GameOver {
        return Err(GameError::conflict("game is over"));
    }
    let active = session
        .active_color()
        .ok_or_else(|| GameError::bad_request("game is not in a turn phase"))?;
    if let Some(color) = color {
        if color != active {
            return Err(GameError::forbidden(format!("it is {active:?}'s turn")));
        }
    }
    Ok(active)
}

fn require_action_phase(session: &GameSession) -> Result<(), GameError> {
    if session.phase != Phase::Action {
        return Err(GameError::bad_request("act during the action phase"));
    }
    if session.seven_pending() {
        return Err(GameError::conflict("resolve the robber first"));
    }
    Ok(())
}
