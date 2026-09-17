# Rust Catan

This repository contains an implementation of the game Catan in Rust and a Player AI, which tries to play the game as optimally as possible.

Rules reference: [`docs/RULES.md`](docs/RULES.md) (summary) and [`res/catan-rulebook.pdf`](res/catan-rulebook.pdf) (authoritative). Implementation plan: [`PLAN.md`](PLAN.md).

# Included Crates

The repo contains two binary crates. One creates a game server, and the other creates a player that will try to play on this game server.

# Running a game

```sh
# Terminal 1: authoritative server (fixed board, 2-4 players, first to 10 VP).
cargo run --bin server
# Terminal 2-4: one greedy AI client per color (first client may create the game).
GAME_ID=$(curl -s -X POST http://127.0.0.1:3000/games | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
cargo run --bin client -- Red "$GAME_ID"
cargo run --bin client -- Blue "$GAME_ID"
cargo run --bin client -- Orange "$GAME_ID"
# Or let the first client create the game and print its id:
cargo run --bin client -- Red
```

# API sketch (v1, JSON over HTTP)

Mutating routes need `Authorization: Bearer <token>` from `POST /games/:id/join`.

```sh
BASE=http://127.0.0.1:3000
G=$(curl -s -X POST $BASE/games | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
RED=$(curl -s -X POST $BASE/games/$G/join -H 'Content-Type: application/json' -d '{"color":"Red"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])')
curl -s -X POST $BASE/games/$G/start | head -c 120; echo
curl -s -X POST $BASE/games/$G/setup -H "Authorization: Bearer $RED" -H 'Content-Type: application/json' -d '{"color":"Red","intersection_id":20,"edge_id":30}'
echo
curl -s $BASE/games/$G/state | head -c 120; echo
```

Full contract: lobby (`POST /games`, `join`, `start`), setup, `roll`, `discard`, `robber`, `build`, `trade` (+ `trade/:trade_id/accept|decline`), `dev/play`, `end-turn`, `GET state|log`. Legacy aliases `GET /state` and `POST /join` target the `default` game.

# Checks

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```