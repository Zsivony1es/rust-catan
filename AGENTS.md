# AGENTS.md

Guidance for AI / human contributors working in this repo.

## Rules source

- Game rules summary: [`docs/RULES.md`](docs/RULES.md)
- Authoritative rules PDF: [CATAN 6th ed, CN3081, v6.250401](https://www.catan.com/sites/default/files/2025-03/CN3081%20CATAN%E2%80%93The%20Game%20Rulebook%20secure%20%281%29.pdf) (not vendored) — if it conflicts with `docs/RULES.md`, the PDF wins.
- Implementation plan (server-focused): [`PLAN.md`](PLAN.md)

Read `docs/RULES.md` before changing game logic, board model, costs, trading, robber, dev cards, scoring, or win conditions.

## Repo layout

- `src/lib.rs` — shared library (`rust_catan`).
- `src/model/` — board, player, enums (currently incomplete — see `PLAN.md` §1).
- `src/game_session.rs` — shared session DTO (currently minimal).
- `src/bin/server/main.rs` — Axum server (`/health`, `/state`, `/join` only for now).
- `src/bin/client/main.rs` — reqwest test client (health → join → state only for now).
- `docs/` — design docs; `docs/RULES.md` is the rules reference.

## Conventions

- Server is authoritative; client only sends intents and renders `GameSession` JSON.
- Keep shared types serde-serializable, no lifetimes in DTOs.
- Plan-only artifacts (`PLAN.md`, `docs/RULES.md`) describe intent — do not treat them as implemented behavior.
- Run `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` before opening a PR.
