# Rust-Catan Implementation Plan (Server-Focused)

Source of truth for rules: `res/catan-rulebook.pdf` (CATAN 6th ed, CN3081, v6.250401).
Current code audited: `src/lib.rs`, `src/game_session.rs`, `src/model/*`, `src/bin/server/main.rs`, `src/bin/client/main.rs`, `Cargo.toml`.

Goal: make `cargo run --bin server` + `cargo run --bin client` play a legal, complete game of Catan to 10 VPs. Server is authoritative; client(s) are thin + AI loop that polls/acts via HTTP.

> Scope of this file: plan only. No behavior change yet.

## 1. Current State Audit

What works:
- Axum server skeleton with `GET /health`, `GET /state`, `POST /join`, `Arc<Mutex<GameSession>>`, tracing + Sentry + `TraceLayer`.
- Reqwest client skeleton: health check -> join -> fetch state.
- Shared lib crate (`rust_catan`) with serde types.

What is missing / wrong:
- `GameSession { turn: u32, players: Vec<Player> }` — no board, no supply, no turn owner, no phase, no dice, no robber, no dev deck, no winner, no move log.
- `Board { layout: UnGraph<RoadNode, ()> }` — always empty, never built, not serializable (`UnGraph` has no serde impl configured), edges carry `()` so roads cannot be represented.
- `RoadNode { node_id, node_type, neighbouring_fields: Vec<FieldType> }` — no coordinates, no port info, no settlement/city owner split.
- `RoadNodeType::{HasVP, HasVPWithSettler(PlayerColor), HasTown(PlayerColor), Empty}` — does not model Catan: need `Empty | Settlement(Owner) | City(Owner)` on nodes, `Empty | Road(Owner)` on edges. Current names conflate VP tiles with board.
- `FieldType::{Forest, Savanna, Desert, Mountain}` — rulebook needs 19 hexes: 3x hills (brick), 4x forest (wood), 4x pasture (wool), 4x fields (wheat), 3x mountains (ore), 1x desert (nothing). `Savanna` is ambiguous, hills/pasture/fields missing.
- `Resource::{Hide, Rock, Horn, Meat}` — only 4 variants, names don't match rulebook (wood/brick/wool/wheat/ore). Need 5 + mapping `FieldType -> Resource`.
- `Player { color, settlements_on, towns_on }` — no resource hand, no dev hand, no roads list, no played knights count, no VP cache, no port access. `towns_on` should be `cities_on`.
- Server `POST /join`: increments `turn` on join (should not), allows any number of players, no game start/lobby, no validation errors (only 500/200), no auth/identity — any client can act as any color.
- No game logic anywhere: dice, production, 7/robber/discard, trade, build costs, distance rule, connectivity, longest route, largest army, dev cards, win check.

## 2. Rules Distilled (for implementation)

From rulebook PDF player aid + body:
- Objective: first to 10 VPs on your own turn wins immediately (VP cards may be revealed even if bought same turn to reach 10).
- Setup: fixed layout for MVP (p.4-5), variable setup later (p.11). Round 1 in turn order: 1 settlement + 1 adjacent road. Round 2 reverse order: 1 settlement + 1 adjacent road. Collect starting resources from 2nd settlement's adjacent hexes. Robber starts on desert. First player = highest dice roll.
- Turn structure: 1) Production phase: optionally play 1 dev card *before* roll, then roll 2d6, then collect (settlement=1x, city=2x per adjacent matching-number hex, robber hex blocked, supply shortage rule) or resolve 7. 2) Action phase in any order/any times if affordable: trade, build, play 1 dev card (only if not already played pre-roll, not the turn bought — except VP). Then pass dice left.
- Resolve 7: 1) all players with >7 resource cards discard half rounded down, 2) active player must move robber to new hex + steal 1 random resource from a player with building on that hex (choose victim if multiple).
- Trade: player-to-player on your turn only (no giving away free, no like-for-like laundering); bank 4:1; port 3:1 or 2:1-specific if you have building on it.
- Build (costs per player-aid panel — re-verify icons before coding; standard is Road=1 wood+1 brick, Settlement=1 wood+1 brick+1 wool+1 wheat, City=2 wheat+3 ore (upgrade settlement), DevCard=1 wool+1 wheat+1 ore):
  - Road (0 VP): empty edge, must connect to own road/building, cannot build past opponent building.
  - Settlement (1 VP): empty intersection, distance rule (>=2 edges from any building), must connect to own road. Only 5 pieces; must upgrade to city to free one.
  - City (2 VP): replaces own settlement. Only 4 pieces.
  - Dev card: draw top of shuffled 25-card deck.
- Dev deck (25): 14x Knight (move robber + steal, counts to Largest Army), 5x Victory Point (hidden until winning reveal), 2x Monopoly (name 1 resource, take all from everyone), 2x Road Building (2 free roads), 2x Invention/Year of Plenty (take any 2 resources from supply). Hidden, not stealable, not counted for 7-discard, never re-enters supply, at most 1 played/turn.
- Bonuses (2 VP each): Largest Army (first to 3 knights, stolen on strictly-more), Longest Route (first to 5 continuous roads, stolen on strictly-more, returned to supply on tie-break/broken below 5). Route-breaking via opponent settlement in middle counts.
- Components/limits to enforce: 19 hexes, 18 number discs (no 7), 95 resource cards (19x5 — enforce shortage rule), 25 dev cards, per-color pieces (15 roads, 5 settlements, 4 cities), 2 bonus tiles, 1 robber, ports.

## 3. Shared Model Changes (`src/` lib — needed by both server and client)

Keep everything serde-serializable (server JSON <-> client). Avoid lifetimes / `petgraph` in DTOs; use IDs.

1. `model/enums/resource.rs`: replace with `Wood, Brick, Wool, Wheat, Ore`. Keep serde renames stable. Add `ALL: [Resource; 5]`.
2. `model/enums/field_type.rs`: replace with `Forest(Wood), Hills(Brick), Pasture(Wool), Fields(Wheat), Mountains(Ore), Desert`. Add `produces() -> Option<Resource>`.
3. New `model/hex.rs`: `Hex { id: HexId, field: FieldType, number: Option<u8> (None for desert), has_robber: bool }`. Validate numbers in 2..=12 except 7.
4. New `model/costs.rs`: `COST_ROAD, COST_SETTLEMENT, COST_CITY, COST_DEV_CARD` as `HashMap<Resource, u8>` + `can_afford(hand) / pay(hand)`.
5. `model/board.rs` rework:
   - Do NOT serialize `petgraph` directly. Canonical DTO: `BoardState { hexes: Vec<Hex>, intersections: Vec<Intersection>, edges: Vec<Edge>, ports: Vec<Port> }` where `Intersection { id, settlement: Option<Building> }`, `Edge { id, a, b, road: Option<PlayerColor> }`, `Port { id, intersection_ids or edge_id, ratio: (u8, Option<Resource>) }` for 3:1 and 2:1.
   - Keep `petgraph::UnGraph` only as server-side helper/validator built from DTO (for connectivity + longest path), not in API.
   - `BoardBuilder`: `fixed_setup() -> BoardState` (match p.4 diagram: 54 intersections / 72 edges / 19 hex adjacency + number tokens + ports + robber on desert). `variable_setup(rng)` deferred to later milestone.
   - Validators: `distance_rule_ok()`, `settlement_connects_to_own_road()`, `road_connects_to_own_network()`, `longest_continuous_road(color)`.
6. `model/player.rs` rework: `Player { color, resources: HashMap<Resource,u8>, dev_hand: Vec<DevCard>, played_knights: u8, roads_built, settlements_left, cities_left, ports_owned (derived), victory_points_cached }` + `settlements: Vec<IntersectionId>, cities: Vec<IntersectionId>, roads: Vec<EdgeId>`.
7. New `model/dev_card.rs`: `DevCardKind::{Knight, VictoryPoint, Monopoly, RoadBuilding, Invention}`, deck builder (shuffled 25), `playable_this_turn(built_turn, current_turn)`.
8. New `model/trade.rs`: `TradeOffer { from, give: Bag, want: Bag }`, bank/port ratio resolution.
9. `game_session.rs` rework: `GameSession { id: GameId, phase: Phase::{Lobby, SetupRound1, SetupRound2, Production, Action, GameOver}, turn_index: usize, turn_number: u32, order: Vec<PlayerColor>, board: BoardState, supply: HashMap<Resource,u8>, dev_deck: Vec<DevCard>, longest_route_holder: Option<PlayerColor>, largest_army_holder: Option<PlayerColor>, winner: Option<PlayerColor>, log: Vec<GameEvent>, pending_discard: Option<...> }`. `turn: u32` retained as `turn_number` for compat. Add `active_color()`, `victory_points(color)` calculator (settlements + cities + VP cards + bonuses).

## 4. Server Components (focus)

`src/bin/server/main.rs` today is one file with 3 routes. Split into modules under `src/server/` (lib) + thin `main.rs`:

1. `server/state.rs`: `AppState { games: Arc<RwLock<HashMap<GameId, Mutex<Game>>> }` (multi-game; per-game mutex to avoid global lock). Single-game `Arc<Mutex>` is OK for M1 but migrate to this by M3. Use `RwLock`, not `Mutex`, for read-heavy `/state`.
2. `server/routes.rs` + per-domain handlers:
   - Lobby: `POST /games` (create, returns id + fixed board), `POST /games/:id/join {color}` (2-4 players, reject dupes/full/started; return 409 with JSON error, not bare status), `POST /games/:id/start` (roll for first player, enter SetupRound1).
   - Setup: `POST /games/:id/setup {color, intersection_id, edge_id}` (enforce order + forward then reverse + distance rule + adjacency; grant 2nd-settlement resources).
   - Turn engine: `POST /games/:id/roll` (2d6 via `rand`; if !=7 produce, if 7 set `pending_discard` and require discards before robber move), `POST /games/:id/discard {color, cards}`, `POST /games/:id/robber {hex_id, victim?}` + steal.
   - Actions: `POST /games/:id/build {kind: Road|Settlement|City|DevCard, ...ids}` (check phase=Action or Setup, costs, piece limits, placement rules, deduct + update supply, recalc longest route/army, check win), `POST /games/:id/trade {bank|port|player...}` (validate ratios, hands, turn ownership), `POST /games/:id/dev/play {kind, params}` (1/turn, not same-turn-bought except VP, apply effect), `POST /games/:id/end-turn` (check win already handled; advance `turn_index`, reset per-turn flags, set phase=Production).
   - Read: `GET /games/:id/state` (full `GameSession` JSON), `GET /games/:id/log` (optional event tail for AI/debugging). Keep legacy `GET /health`, `GET /state`, `POST /join` as aliases to default game until clients migrate.
3. `server/engine.rs`: pure functions `apply_roll, apply_production, apply_discard, apply_robber, apply_build, apply_trade, apply_dev_play, advance_turn, check_winner` — unit-testable without HTTP. All mutations validated; return typed `GameError` -> mapped to HTTP 400/403/404/409.
4. `server/auth.rs` (minimal): `PlayerToken` — on join return secret token; require `Authorization: Bearer <token>` for mutating routes so clients cannot move each other. MVP can be `?color=` + token header; document that real auth is out of scope.
5. `server/error.rs`: `GameError { code, message }` JSON + `IntoResponse`. Replace bare `StatusCode` returns.
6. `server/random.rs`: seedable RNG (`rand`, `rand_chacha`) so tests reproduce dice/shuffles; `SERVER_SEED` env.
7. Infra already present — keep: `TraceLayer`, `tracing` structured fields per route, Sentry guard. Add request IDs (`tower-http` `RequestIdLayer`) and per-game log correlation (`game_id`, `active_color`, `phase` fields).

New deps needed: `rand`, `rand_chacha`, `uuid` (game ids), `thiserror` (typed errors). Consider `parking_lot::RwLock` or tokio `RwLock` instead of std `Mutex` in async handlers.

Concurrency/consistency rules: single writer per game (per-game `Mutex`), validate-then-mutate atomically inside lock, never hold lock across `.await` that does I/O, clone DTO for `GET /state`.

## 5. Proposed API Contract (v1, REST+JSON — polling, no websockets for MVP)

- `GET /health -> {status:"ok"}`
- `POST /games -> GameSession` (creates fixed-setup board, phase=Lobby)
- `POST /games/:id/join {"color":"Red"} -> {session, token}`
- `POST /games/:id/start -> GameSession`
- `POST /games/:id/setup {"color","intersection_id","edge_id"} -> GameSession`
- `POST /games/:id/roll -> {dice:[u8;2], session}`
- `POST /games/:id/discard {"color","cards":{"Wood":2}} -> GameSession`
- `POST /games/:id/robber {"hex_id":7,"victim":"Blue"} -> GameSession`
- `POST /games/:id/build {"kind":"Road","edge_id":12} | {"kind":"Settlement","intersection_id":5} | {"kind":"City","intersection_id":5} | {"kind":"DevCard"} -> GameSession`
- `POST /games/:id/trade {"kind":"Bank","give":{"Wood":4},"want":{"Ore":1}} | {"kind":"Port",...} | {"kind":"Player","to":"Blue","give":{},"want":{}} -> GameSession` (player-player needs accept step: `POST /games/:id/trade/:trade_id/accept` — defer to M5)
- `POST /games/:id/dev/play {"kind":"Knight","hex_id":..,"victim":..} etc. -> GameSession`
- `POST /games/:id/end-turn -> GameSession`
- `GET /games/:id/state -> GameSession`, `GET /games/:id/log -> GameEvent[]`
- All mutating responses include full `GameSession` so simple AI clients can just re-read state. Errors: `{error:{code,message}}` with 4xx.

Client identity: `Authorization: Bearer <join-token>` on mutating calls; `color` in body must match token.

## 6. Client Components (must be able to play — keep simple, AI-ready)

`src/bin/client/main.rs` today only joins + prints. Evolve to loop:

1. `client/api.rs`: typed wrapper over endpoints above (reqwest + token store + retry on 409/turn-not-mine with backoff poll).
2. `client/rules_view.rs`: local helpers reading `GameSession` — `my_resources(), affordable_builds(), legal_placements()` (reuse lib validators so client doesn't duplicate logic).
3. `client/strategy.rs`: pluggable `Strategy` trait. MVP `GreedyStrategy`: if setup phase pick highest pip-count intersection with diverse resources; per turn: roll -> if 7 discard worst-duplicate resources + move robber to leader's hex; in action: build priority City > Settlement > Road > DevCard if affordable, else bank/port trade toward nearest affordable, else end turn. Keep deterministic + seedable for tests.
4. `client/main.rs` loop: `join -> poll state until my turn -> roll -> handle discard/robber if needed -> act until no affordable action -> end-turn -> repeat until GameOver`. CLI args: `SERVER_URL GAME_ID COLOR STRATEGY`. Log each action. This satisfies "player AI that tries to play as optimally as possible" v1; optimal search (MCTS/expected-value) is post-MVP.
5. Multi-client testing: run 3-4 `client` processes with different colors against one server to play full game.

## 7. Testing & Verification

- Unit (lib): board builder counts (19 hexes, 18 numbers, robber on desert), distance rule, road connectivity, longest-road cases incl. broken-route example from p.8, production payout incl. city double + robber block + shortage rule, 7-discard rounding, trade ratios, dev deck composition (14/5/2/2/2), VP calc + win at 10.
- Engine integration: scripted full game (fixed dice seed) via `engine::` functions without HTTP.
- API integration (axum `tower::ServiceExt::oneshot` or reqwest against test server): join/start/setup/roll/build/trade/robber/end-turn happy path + 400/409 error cases + token mismatch 403.
- E2E: boot server, run 3 scripted clients to GameOver, assert winner has >=10 VP and log is legal.
- `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` in CI.

## 8. Phased Milestones (server-first)

- M0 Cleanup: rename `Resource`/`FieldType`, fix `Player` fields (`cities_on`), add deps (`rand`, `uuid`, `thiserror`), define `GameError`, keep old routes compiling with compat shims.
- M1 Lobby + state: multi-game `AppState`, `POST /games`, join/start, `GET /games/:id/state`, tokens, fixed board builder (hexes+numbers, no ports yet). Client can join + see board.
- M2 Setup phase: ordered placement + validation + 2nd-settlement resources. Client can complete setup.
- M3 Core turn loop: roll/production/discard/robber/end-turn + win check (no trade/dev/bonuses yet). Playable settlement/road/city game to 10 VP with simplified scoring.
- M4 Building full: costs, piece limits, distance/connectivity, city upgrade, bank 4:1 trade. Greedy client can build.
- M5 Trade + ports + player-trade accept flow + dev deck (buy only). Ports on board.
- M6 Dev play (all 5 kinds) + Largest Army + Longest Route + broken-route handling. Full rules.
- M7 AI + hardening: greedy -> heuristic strategy, variable setup, game log, request IDs, E2E 4-client game, CI, README update with curl examples.

## 9. Risks / Open Questions

1. Building-cost icons in PDF extraction are image-only — confirm Road/Settlement/City/Dev costs from physical player-aid before M4.
2. `petgraph` vs hand-rolled adjacency: keep `petgraph` server-side only or drop it to reduce serialization friction? Decision: DTO + helper graph, revisit if longest-path perf matters (board is tiny).
3. Single `GameSession` vs multi-game: M1 migrates; keep backward-compat aliases to avoid breaking current client.
4. Polling vs websocket: polling is fine for AI MVP; human UI later wants websocket/SSE for live state — do not add now.
5. Sentry DSN is hardcoded in both binaries — move to `SENTRY_DSN` env before release.
6. Naming: current `Hide/Rock/Horn/Meat`, `Savanna`, `towns_on`, `HasVP...` must be renamed — this is breaking for any saved JSON; acceptable pre-1.0, note in changelog.
