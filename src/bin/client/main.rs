//! Catan player (client) binary: greedy AI loop over the v1 HTTP API.
//!
//! Run with the server up in another terminal:
//! ```sh
//! cargo run --bin server
//! # first client creates the game, others join it:
//! cargo run --bin client -- Red
//! cargo run --bin client -- Blue <GAME_ID>
//! # optional:
//! # SERVER_URL=http://127.0.0.1:3000 cargo run --bin client -- Blue <GAME_ID>
//! ```
//! The client joins, completes setup, then repeats: roll, resolve 7s,
//! build/trade/play in priority order, end turn — until `GameOver`.

use std::time::Duration;

use rust_catan::client::{Api, GreedyStrategy, PlannedAction};
use rust_catan::server::dto::DevPlayRequest;
use rust_catan::{DevCardKind, GameSession, Phase, PlayerColor};

fn init_sentry() -> sentry::ClientInitGuard {
    let dsn = std::env::var("SENTRY_DSN").unwrap_or_else(|_| {
        "https://547bc132aefdf46bec7eeff82cfa026e@o4512078782136320.ingest.de.sentry.io/4512078786723920"
            .into()
    });
    sentry::init((
        dsn,
        sentry::ClientOptions::new()
            .maybe_release(sentry::release_name!())
            .send_default_pii(true),
    ))
}

fn parse_color(raw: &str) -> PlayerColor {
    match raw.to_lowercase().as_str() {
        "red" => PlayerColor::Red,
        "blue" => PlayerColor::Blue,
        "orange" => PlayerColor::Orange,
        "white" => PlayerColor::White,
        other => {
            eprintln!("unknown color {other:?}, defaulting to Red");
            PlayerColor::Red
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = init_sentry();

    let base_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
    // `cargo run --bin client -- Blue <GAME_ID>` joins; without a game id the
    // client creates one and prints it for the other players.
    let mut args = std::env::args().skip(1);
    let color = parse_color(&args.next().unwrap_or_else(|| "Red".into()));
    let game_id_arg = args.next();

    let game_id = match game_id_arg {
        Some(id) => id,
        None => {
            let session = Api::create_game(&base_url, None).await?;
            println!("created game {}", session.id);
            session.id
        }
    };
    let mut api = Api::new(base_url, game_id, color);
    api.health().await?;
    let session = api.join().await?;
    println!(
        "joined game {} as {color:?} ({} players)",
        session.id,
        session.players.len()
    );

    run_game(&api, color).await
}

async fn run_game(api: &Api, me: PlayerColor) -> Result<(), Box<dyn std::error::Error>> {
    // The loop never exits on action errors: a rejected move is logged and
    // retried on fresh state. Only GameOver ends the loop.
    let mut consecutive_errors = 0u32;
    loop {
        match step(api, me).await {
            Ok(done) => {
                if done {
                    return Ok(());
                }
                consecutive_errors = 0;
            }
            Err(error) => {
                consecutive_errors += 1;
                eprintln!("{me:?} step failed ({consecutive_errors}x): {error}");
                if consecutive_errors >= 5 {
                    // Possibly stuck re-suggesting a rejected move: try passing.
                    consecutive_errors = 0;
                    let _ = api.end_turn().await;
                } else {
                    sleep().await;
                }
            }
        }
    }
}

/// One poll-and-act step. `Ok(true)` means the game is over.
async fn step(api: &Api, me: PlayerColor) -> Result<bool, Box<dyn std::error::Error>> {
    let session = api.state().await?;
    if session.phase == Phase::GameOver {
        println!(
            "game over: winner={:?} (my points: {})",
            session.winner,
            session.victory_points(me)
        );
        return Ok(true);
    }

    // Discards are owed even off-turn.
    if session.pending_discards.contains_key(&me) {
        let cards = GreedyStrategy::pick_discard(&session, me);
        println!("{me:?} discards {cards:?}");
        api.discard(&cards).await?;
        return Ok(false);
    }

    // A pending robber move preempts everything else on my turn (it may
    // still fail while others owe discards; the loop retries).
    if session.robber_pending && session.active_color() == Some(me) {
        if let Some((hex, victim)) = GreedyStrategy::pick_robber(&session, me) {
            println!("{me:?} moves robber to hex {hex} (victim {victim:?})");
            api.robber(hex, victim).await?;
        } else {
            sleep().await;
        }
        return Ok(false);
    }

    match session.phase {
        Phase::GameOver => Ok(true),
        Phase::Lobby => {
            if session.players.len() >= 2 {
                // One of us starts once enough players joined.
                let _ = api.start().await;
            }
            sleep().await;
            Ok(false)
        }
        Phase::SetupRound1 | Phase::SetupRound2 => {
            if session.setup_expected_color() == Some(me) {
                if let Some((intersection, edge)) = GreedyStrategy::pick_setup(&session, me) {
                    println!("{me:?} setup: settlement {intersection} + road {edge}");
                    api.setup(intersection, edge).await?;
                } else {
                    eprintln!("{me:?}: no legal setup spot");
                    sleep().await;
                }
            } else {
                sleep().await;
            }
            Ok(false)
        }
        Phase::Production => {
            if session.active_color() == Some(me) {
                // Optional pre-roll dev play, then roll (once).
                if !session.seven_pending() {
                    if let Some(action) = GreedyStrategy::dev_to_play(&session, me) {
                        if let Err(error) = play(&session, api, me, action).await {
                            eprintln!("{me:?} pre-roll play failed: {error}");
                        }
                    }
                }
                if session.dice.is_none() {
                    let rolled = api.roll().await?;
                    println!("{me:?} rolled {:?}", rolled.dice);
                } else {
                    sleep().await;
                }
            } else {
                sleep().await;
            }
            Ok(false)
        }
        Phase::Action => {
            if session.active_color() != Some(me) {
                sleep().await;
                return Ok(false);
            }
            match act_once(api, me, &session).await {
                Ok(true) => {}
                Ok(false) => {
                    // Nothing left to do.
                    let after = api.end_turn().await?;
                    println!(
                        "{me:?} ends turn {} (points: {})",
                        after.turn_number,
                        after.victory_points(me)
                    );
                }
                Err(error) => return Err(error),
            }
            Ok(false)
        }
    }
}

/// One action-phase step; `Ok(true)` = acted, `Ok(false)` = pass the dice.
async fn act_once(
    api: &Api,
    me: PlayerColor,
    session: &GameSession,
) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(build) = GreedyStrategy::next_build(session, me) {
        let after = match build {
            PlannedAction::BuildRoad(edge) => {
                println!("{me:?} builds road on edge {edge}");
                api.build_road(edge).await?
            }
            PlannedAction::BuildSettlement(v) => {
                println!("{me:?} builds settlement on {v}");
                api.build_settlement(v).await?
            }
            PlannedAction::BuildCity(v) => {
                println!("{me:?} builds city on {v}");
                api.build_city(v).await?
            }
            PlannedAction::BuyDev => {
                println!("{me:?} buys a development card");
                api.buy_dev().await?
            }
            _ => return Ok(false),
        };
        println!("{me:?} now at {} points", after.victory_points(me));
        return Ok(true);
    }
    if let Some(offer) = GreedyStrategy::trade_to_afford(session, me) {
        match offer {
            PlannedAction::BankTrade { give, want } => {
                println!("{me:?} bank trade {give:?} -> {want:?}");
                api.trade_bank(&give, &want).await?;
            }
            PlannedAction::PortTrade { give, want } => {
                println!("{me:?} port trade {give:?} -> {want:?}");
                api.trade_port(&give, &want).await?;
            }
            PlannedAction::ProposeTrade { .. } => return Ok(false),
            _ => return Ok(false),
        }
        return Ok(true);
    }
    if let Some(action) = GreedyStrategy::dev_to_play(session, me) {
        return play(session, api, me, action).await.map(|()| true);
    }
    Ok(false)
}

async fn play(
    _session: &GameSession,
    api: &Api,
    me: PlayerColor,
    action: PlannedAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        PlannedAction::PlayKnight { hex, victim } => {
            println!("{me:?} plays Knight (robber to hex {hex})");
            api.play_knight(hex, victim).await?;
        }
        PlannedAction::PlayMonopoly { resource } => {
            println!("{me:?} plays Monopoly on {resource:?}");
            api.play_dev(DevPlayRequest {
                kind: DevCardKind::Monopoly,
                resource: Some(resource),
                ..Default::default()
            })
            .await?;
        }
        PlannedAction::PlayRoadBuilding { edges } => {
            println!("{me:?} plays Road Building on edges {edges:?}");
            api.play_dev(DevPlayRequest {
                kind: DevCardKind::RoadBuilding,
                edges,
                ..Default::default()
            })
            .await?;
        }
        PlannedAction::PlayInvention { resources } => {
            println!("{me:?} plays Invention for {resources:?}");
            api.play_dev(DevPlayRequest {
                kind: DevCardKind::Invention,
                resources,
                ..Default::default()
            })
            .await?;
        }
        _ => {}
    }
    Ok(())
}

async fn sleep() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}
