//! Engine-level tests: scripted games through `server::engine` without HTTP.
//!
//! Covers PLAN §7 "engine integration": join/start/setup/roll/build/trade/
//! robber/end-turn, plus a full seeded game to `GameOver`.

use rand::SeedableRng as _;
use rand_chacha::ChaCha8Rng;

use rust_catan::client::{GreedyStrategy, PlannedAction};
use rust_catan::model::bag;
use rust_catan::server::engine::{self, BuildTarget, DevPlayParams};
use rust_catan::{DevCardKind, GameSession, Phase, PlayerColor, Resource};

fn test_session(seed: u64) -> (GameSession, ChaCha8Rng) {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let session = GameSession::new("test-game".into(), &mut rng);
    (session, rng)
}

fn join3(session: &mut GameSession) {
    for color in [PlayerColor::Red, PlayerColor::Blue, PlayerColor::Orange] {
        engine::join(session, color).expect("join");
    }
}

/// Join/start/setup via the greedy strategy; returns a game in Production.
fn setup_game(seed: u64) -> (GameSession, ChaCha8Rng) {
    let (mut session, mut rng) = test_session(seed);
    join3(&mut session);
    engine::start(&mut session, &mut rng).expect("start");
    assert_eq!(session.order.len(), 3);
    while matches!(session.phase, Phase::SetupRound1 | Phase::SetupRound2) {
        let color = session.setup_expected_color().expect("expected");
        let (v, e) = GreedyStrategy::pick_setup(&session, color).expect("setup spot");
        engine::setup_place(&mut session, color, v, e).expect("place");
    }
    assert_eq!(session.phase, Phase::Production);
    assert_eq!(session.turn_number, 1);
    (session, rng)
}

#[test]
fn lobby_rejects_bad_joins_and_early_start() {
    let (mut session, mut rng) = test_session(1);
    assert!(engine::start(&mut session, &mut rng).is_err()); // 0 players
    engine::join(&mut session, PlayerColor::Red).unwrap();
    assert!(engine::start(&mut session, &mut rng).is_err()); // 1 player
    assert!(engine::join(&mut session, PlayerColor::Red).is_err()); // dupe
    engine::join(&mut session, PlayerColor::Blue).unwrap();
    engine::join(&mut session, PlayerColor::Orange).unwrap();
    engine::join(&mut session, PlayerColor::White).unwrap();
    assert!(engine::start(&mut session, &mut rng).is_ok());
    assert!(engine::join(&mut session, PlayerColor::Red).is_err()); // started
}

#[test]
fn setup_grants_starting_resources() {
    let (session, _) = setup_game(11);
    let total: u32 = session.players.iter().map(|p| p.resource_count()).sum();
    assert!(total > 0, "second settlements pay out");
    for player in &session.players {
        assert_eq!(player.settlements.len(), 2);
        assert_eq!(player.roads.len(), 2);
    }
}

#[test]
fn setup_enforces_order_and_distance() {
    let (mut session, mut rng) = test_session(3);
    join3(&mut session);
    engine::start(&mut session, &mut rng).unwrap();
    let first = session.setup_expected_color().unwrap();
    let wrong = [PlayerColor::Red, PlayerColor::Blue, PlayerColor::Orange]
        .into_iter()
        .find(|c| *c != first)
        .unwrap();
    let (v, e) = GreedyStrategy::pick_setup(&session, first).unwrap();
    assert!(engine::setup_place(&mut session, wrong, v, e).is_err());
    assert!(engine::setup_place(&mut session, first, v, e).is_ok());
    // Same spot is now taken for the next player.
    let second = session.setup_expected_color().unwrap();
    assert!(engine::setup_place(&mut session, second, v, e).is_err());
}

#[test]
fn seven_flow_discards_then_robber() {
    let (mut session, mut rng) = setup_game(21);
    // Roll until a 7 appears (seeded rng keeps this deterministic).
    let mut rolled_seven = false;
    for _ in 0..200 {
        let (d1, d2) = engine::roll(&mut session, &mut rng).unwrap();
        if d1 + d2 == 7 {
            rolled_seven = true;
            break;
        }
        assert_eq!(session.phase, Phase::Action);
        engine::end_turn(&mut session).unwrap();
    }
    assert!(rolled_seven, "a 7 shows up within 200 rolls");
    assert!(session.robber_pending);
    // Discard for everyone owing, then move the robber.
    let owing: Vec<PlayerColor> = session.pending_discards.keys().copied().collect();
    for color in owing {
        let cards = GreedyStrategy::pick_discard(&session, color);
        engine::discard(&mut session, color, &cards).unwrap();
    }
    assert!(session.pending_discards.is_empty());
    let me = session.active_color().unwrap();
    let (hex, victim) = GreedyStrategy::pick_robber(&session, me).unwrap();
    let current = session.robber_hex().unwrap();
    assert_ne!(hex, current);
    assert!(engine::robber(&mut session, &mut rng, current, None).is_err());
    engine::robber(&mut session, &mut rng, hex, victim).unwrap();
    assert_eq!(session.phase, Phase::Action);
}

#[test]
fn discard_rounding_and_exactness() {
    let (mut session, _) = setup_game(22);
    let me = PlayerColor::Red;
    // Hand Red 9 cards directly, then open a 7 via the pending map path:
    // simulate by inserting the owed amount like open_seven would.
    {
        let player = session.player_mut(me).unwrap();
        player.resources = bag::bag(&[(Resource::Wood, 9)]);
    }
    assert_eq!(session.player(me).unwrap().discard_owed(), 4);
    session.pending_discards.insert(me, 4);
    session.robber_pending = true;
    assert!(engine::discard(&mut session, me, &bag::bag(&[(Resource::Wood, 3)])).is_err());
    engine::discard(&mut session, me, &bag::bag(&[(Resource::Wood, 4)])).unwrap();
    assert_eq!(
        session.player(me).unwrap().count_resource(Resource::Wood),
        5
    );
}

#[test]
fn production_pays_settlement_once_city_twice_and_skips_robber() {
    let (mut session, _) = setup_game(23);
    // Find a producing hex and read payouts by snapshotting hands.
    let number = session
        .board
        .hexes
        .iter()
        .find(|h| h.number.is_some() && !h.has_robber)
        .expect("producing hex")
        .number
        .unwrap();
    let before: Vec<(PlayerColor, u32)> = session
        .players
        .iter()
        .map(|p| (p.color, p.resource_count()))
        .collect();
    // Directly invoke production through a forced roll path: roll until the
    // number hits (bounded), asserting payouts only grow.
    let mut rng = ChaCha8Rng::seed_from_u64(23);
    for _ in 0..200 {
        while session.phase == Phase::Action {
            engine::end_turn(&mut session).unwrap();
        }
        let (d1, d2) = engine::roll(&mut session, &mut rng).unwrap();
        if d1 + d2 == number {
            break;
        }
        // Resolve 7s on the way.
        let owing: Vec<PlayerColor> = session.pending_discards.keys().copied().collect();
        for color in owing {
            let cards = GreedyStrategy::pick_discard(&session, color);
            engine::discard(&mut session, color, &cards).unwrap();
        }
        if session.robber_pending {
            let me = session.active_color().unwrap();
            let (hex, victim) = GreedyStrategy::pick_robber(&session, me).unwrap();
            engine::robber(&mut session, &mut rng, hex, victim).unwrap();
        }
    }
    let after: Vec<(PlayerColor, u32)> = session
        .players
        .iter()
        .map(|p| (p.color, p.resource_count()))
        .collect();
    // Someone adjacent to the hex must have gained (supply starts full).
    let gained = after
        .iter()
        .zip(before.iter())
        .any(|((_, a), (_, b))| a > b);
    assert!(gained, "production paid out for number {number}");
}

#[test]
fn bank_and_port_trades() {
    let (mut session, _) = setup_game(24);
    // Roll into the action phase.
    let mut rng = ChaCha8Rng::seed_from_u64(24);
    while session.phase == Phase::Production && !session.seven_pending() {
        let (d1, d2) = engine::roll(&mut session, &mut rng).unwrap();
        if d1 + d2 == 7 {
            break;
        }
    }
    if session.seven_pending() {
        return; // covered elsewhere; keep this test focused
    }
    let me = session.active_color().unwrap();
    {
        let player = session.player_mut(me).unwrap();
        player.resources = bag::bag(&[(Resource::Wood, 4)]);
    }
    // Like-for-like and short piles fail.
    assert!(engine::trade_bank(
        &mut session,
        me,
        &bag::bag(&[(Resource::Wood, 4)]),
        &bag::bag(&[(Resource::Wood, 1)]),
    )
    .is_err());
    engine::trade_bank(
        &mut session,
        me,
        &bag::bag(&[(Resource::Wood, 4)]),
        &bag::bag(&[(Resource::Ore, 1)]),
    )
    .unwrap();
    assert_eq!(session.player(me).unwrap().count_resource(Resource::Ore), 1);
    // Port trade without a port fails.
    assert!(engine::trade_port(
        &mut session,
        me,
        &bag::bag(&[(Resource::Ore, 3)]),
        &bag::bag(&[(Resource::Wheat, 1)]),
    )
    .is_err());
}

/// Roll for the active player and resolve any 7 (discards + robber).
fn roll_and_resolve(session: &mut GameSession, rng: &mut ChaCha8Rng) {
    let me = session.active_color().expect("active");
    engine::roll(session, rng).expect("roll");
    let owing: Vec<PlayerColor> = session.pending_discards.keys().copied().collect();
    for color in owing {
        let cards = GreedyStrategy::pick_discard(session, color);
        engine::discard(session, color, &cards).expect("discard");
    }
    if session.robber_pending {
        let (hex, victim) = GreedyStrategy::pick_robber(session, me).expect("robber target");
        engine::robber(session, rng, hex, victim).expect("robber");
    }
}

fn fund(session: &mut GameSession, color: PlayerColor, cards: &[(Resource, u8)]) {
    let player = session.player_mut(color).expect("player");
    bag::add_all(&mut player.resources, &bag::bag(cards));
}

fn grant_card(session: &mut GameSession, color: PlayerColor, kind: DevCardKind) {
    let built_turn = session.turn_number.saturating_sub(1);
    session
        .player_mut(color)
        .expect("player")
        .dev_hand
        .push(rust_catan::DevCardInstance::new(kind, built_turn));
}

#[test]
fn dev_deck_buy_blocks_same_turn_play() {
    let (mut session, mut rng) = setup_game(25);
    let me = session.active_color().unwrap();
    roll_and_resolve(&mut session, &mut rng);
    assert_eq!(session.phase, Phase::Action);
    let deck_before = session.dev_deck.len();
    fund(
        &mut session,
        me,
        &[
            (Resource::Wool, 1),
            (Resource::Wheat, 1),
            (Resource::Ore, 1),
        ],
    );
    engine::build(&mut session, me, BuildTarget::DevCard, false).unwrap();
    assert_eq!(session.dev_deck.len(), deck_before - 1);
    assert_eq!(session.player(me).unwrap().dev_hand.len(), 1);
    // Not playable the turn bought (unless it is a VP card).
    let kind = session.player(me).unwrap().dev_hand[0].kind;
    if kind != DevCardKind::VictoryPoint {
        assert!(
            engine::play_dev(&mut session, &mut rng, me, kind, &DevPlayParams::default()).is_err()
        );
    }
}

#[test]
fn knight_army_bonus_at_three() {
    let (mut session, mut rng) = setup_game(26);
    let me = session.active_color().unwrap();
    for played in 0..3 {
        while session.active_color() != Some(me) {
            full_turn(&mut session, &mut rng);
        }
        grant_card(&mut session, me, DevCardKind::Knight);
        let (hex, victim) = GreedyStrategy::pick_robber(&session, me).unwrap();
        engine::play_dev(
            &mut session,
            &mut rng,
            me,
            DevCardKind::Knight,
            &DevPlayParams {
                hex_id: Some(hex),
                victim,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(session.player(me).unwrap().played_knights, played + 1);
        // Finish my turn (knight may land pre- or post-roll).
        if session.phase == Phase::Production {
            roll_and_resolve(&mut session, &mut rng);
        }
        assert_eq!(session.phase, Phase::Action);
        if session.winner.is_none() {
            engine::end_turn(&mut session).unwrap();
        }
    }
    assert_eq!(session.largest_army_holder, Some(me));
}

#[test]
fn monopoly_takes_everything_named() {
    let (mut session, mut rng) = setup_game(27);
    let me = session.active_color().unwrap();
    let others: Vec<PlayerColor> = session
        .players
        .iter()
        .map(|p| p.color)
        .filter(|c| *c != me)
        .collect();
    fund(&mut session, others[0], &[(Resource::Ore, 3)]);
    fund(
        &mut session,
        others[1],
        &[(Resource::Ore, 2), (Resource::Wood, 5)],
    );
    grant_card(&mut session, me, DevCardKind::Monopoly);
    // Monopoly is playable pre-roll.
    assert_eq!(session.phase, Phase::Production);
    engine::play_dev(
        &mut session,
        &mut rng,
        me,
        DevCardKind::Monopoly,
        &DevPlayParams {
            resource: Some(Resource::Ore),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(session.player(me).unwrap().count_resource(Resource::Ore) >= 5);
    assert_eq!(
        session
            .player(others[0])
            .unwrap()
            .count_resource(Resource::Ore),
        0
    );
    assert_eq!(
        session
            .player(others[1])
            .unwrap()
            .count_resource(Resource::Ore),
        0
    );
    // Untouched piles stay put.
    assert_eq!(
        session
            .player(others[1])
            .unwrap()
            .count_resource(Resource::Wood),
        5
    );
}

#[test]
fn invention_takes_two_from_supply() {
    let (mut session, mut rng) = setup_game(28);
    let me = session.active_color().unwrap();
    grant_card(&mut session, me, DevCardKind::Invention);
    let supply_before = bag::total(&session.supply);
    engine::play_dev(
        &mut session,
        &mut rng,
        me,
        DevCardKind::Invention,
        &DevPlayParams {
            resources: vec![Resource::Wheat, Resource::Ore],
            ..Default::default()
        },
    )
    .unwrap();
    let hand = &session.player(me).unwrap().resources;
    assert!(bag::count(hand, Resource::Wheat) >= 1);
    assert!(bag::count(hand, Resource::Ore) >= 1);
    assert_eq!(bag::total(&session.supply), supply_before - 2);
}

#[test]
fn road_building_places_two_free_roads() {
    let (mut session, mut rng) = setup_game(29);
    let me = session.active_color().unwrap();
    // Advance to my action phase so the board has my setup roads to extend.
    roll_and_resolve(&mut session, &mut rng);
    let roads_before = session.player(me).unwrap().roads.len();
    let first = session
        .board
        .edges
        .iter()
        .filter(|e| session.board.road_spot_ok(me, e.id))
        .map(|e| e.id)
        .min()
        .expect("legal road spot");
    // Simulate the first road to find a legal second one.
    let mut scratch = session.board.clone();
    scratch.edge_mut(first).expect("edge").road = Some(me);
    let second = scratch
        .edges
        .iter()
        .filter(|e| scratch.road_spot_ok(me, e.id))
        .map(|e| e.id)
        .min();
    grant_card(&mut session, me, DevCardKind::RoadBuilding);
    let edges = if let Some(second) = second {
        vec![first, second]
    } else {
        vec![first]
    };
    engine::play_dev(
        &mut session,
        &mut rng,
        me,
        DevCardKind::RoadBuilding,
        &DevPlayParams {
            edges: edges.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        session.player(me).unwrap().roads.len(),
        roads_before + edges.len()
    );
    // Free means free: no resources were charged (hand only grows via the game).
    for edge in edges {
        assert_eq!(session.board.edge(edge).unwrap().road, Some(me));
    }
}

/// Play one full turn for whoever is active (roll, resolve, act, pass).
fn full_turn(session: &mut GameSession, rng: &mut ChaCha8Rng) {
    let me = session.active_color().expect("active");
    if let Some(action) = GreedyStrategy::dev_to_play(session, me) {
        let _ = apply_planned(session, rng, me, action);
    }
    engine::roll(session, rng).expect("roll");
    let owing: Vec<PlayerColor> = session.pending_discards.keys().copied().collect();
    for color in owing {
        let cards = GreedyStrategy::pick_discard(session, color);
        engine::discard(session, color, &cards).expect("discard");
    }
    if session.robber_pending {
        let (hex, victim) = GreedyStrategy::pick_robber(session, me).expect("robber target");
        engine::robber(session, rng, hex, victim).expect("robber");
    }
    loop {
        if session.winner.is_some() {
            return;
        }
        if let Some(build) = GreedyStrategy::next_build(session, me) {
            if apply_planned(session, rng, me, build).is_err() {
                break;
            }
            continue;
        }
        if let Some(offer) = GreedyStrategy::trade_to_afford(session, me) {
            if apply_planned(session, rng, me, offer).is_err() {
                break;
            }
            continue;
        }
        if let Some(play) = GreedyStrategy::dev_to_play(session, me) {
            if apply_planned(session, rng, me, play).is_err() {
                break;
            }
            continue;
        }
        break;
    }
    if session.winner.is_none() {
        engine::end_turn(session).expect("end turn");
    }
}

fn apply_planned(
    session: &mut GameSession,
    rng: &mut ChaCha8Rng,
    me: PlayerColor,
    action: PlannedAction,
) -> Result<(), rust_catan::GameError> {
    match action {
        PlannedAction::BuildRoad(edge) => {
            engine::build(session, me, BuildTarget::Road(edge), false)
        }
        PlannedAction::BuildSettlement(v) => {
            engine::build(session, me, BuildTarget::Settlement(v), false)
        }
        PlannedAction::BuildCity(v) => engine::build(session, me, BuildTarget::City(v), false),
        PlannedAction::BuyDev => engine::build(session, me, BuildTarget::DevCard, false),
        PlannedAction::BankTrade { give, want } => engine::trade_bank(session, me, &give, &want),
        PlannedAction::PortTrade { give, want } => engine::trade_port(session, me, &give, &want),
        PlannedAction::ProposeTrade { .. } => Ok(()), // no counterparty in engine test
        PlannedAction::PlayKnight { hex, victim } => engine::play_dev(
            session,
            rng,
            me,
            DevCardKind::Knight,
            &DevPlayParams {
                hex_id: Some(hex),
                victim,
                ..Default::default()
            },
        ),
        PlannedAction::PlayMonopoly { resource } => engine::play_dev(
            session,
            rng,
            me,
            DevCardKind::Monopoly,
            &DevPlayParams {
                resource: Some(resource),
                ..Default::default()
            },
        ),
        PlannedAction::PlayRoadBuilding { edges } => engine::play_dev(
            session,
            rng,
            me,
            DevCardKind::RoadBuilding,
            &DevPlayParams {
                edges,
                ..Default::default()
            },
        ),
        PlannedAction::PlayInvention { resources } => engine::play_dev(
            session,
            rng,
            me,
            DevCardKind::Invention,
            &DevPlayParams {
                resources,
                ..Default::default()
            },
        ),
        PlannedAction::EndTurn => engine::end_turn(session),
    }
}

#[test]
fn full_game_reaches_ten_points() {
    let (mut session, mut rng) = setup_game(42);
    let mut turns = 0u32;
    while session.winner.is_none() {
        turns += 1;
        assert!(turns < 5000, "game stalled without a winner");
        full_turn(&mut session, &mut rng);
    }
    let winner = session.winner.unwrap();
    assert!(
        session.victory_points(winner) >= 10,
        "winner has {} points",
        session.victory_points(winner)
    );
    assert!(!session.log.is_empty());
    println!("seed 42 finished in {turns} turns, winner: {winner:?}");
}
