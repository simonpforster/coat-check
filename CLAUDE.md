# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Principles

Keep the project slim. Remove unused code, dependencies, and files — don't leave things around "just in case". If it's not needed, it goes.

## Commits

All commits must follow [Conventional Commits](https://www.conventionalcommits.org/). CI enforces this on PRs.

- `feat:` — new feature (minor bump)
- `fix:` — bug fix (patch bump)
- `feat!:` or `fix!:` — breaking change
- `chore:`, `docs:`, `refactor:`, `test:` — no release

Scope with the crate name when the change is crate-specific: `feat(coat-check): add wind chill factor`.

PRs use squash merge. The PR title becomes the commit message on main, so PR titles must also follow conventional commit format.

## Commands

```bash
cargo build                        # compile all crates
cargo test                         # test all crates
cargo test -p coat-check           # test API crate only
cargo test <name>                  # single test by name substring
cargo fmt                          # auto-format
cargo fmt --check                  # CI format check
cargo clippy -- -D warnings        # lint
cargo llvm-cov                     # coverage report
cargo run -p coat-check            # start API server (default port 3000)
PORT=8080 cargo run -p coat-check  # custom port
```

## Workspace layout

Cargo workspace with crates under `crates/`:

```
crates/
└── api/    # coat-check — the HTTP API service
```

New crates go in `crates/<name>/` and are auto-discovered by `members = ["crates/*"]`.

## Architecture (API crate)

Hexagonal / ports-and-adapters with a strict one-way dependency rule:

```
domain/ ← application/ ← ports/ ← adapters/
```

All paths below relative to `crates/api/src/`.

- `domain/` — pure Rust, zero async, zero I/O. `recommendation::evaluate()` is the coat-decision algorithm.
- `ports/inbound.rs` — `CoatCheckPort` trait (what the HTTP layer calls).
- `ports/outbound.rs` — `WeatherPort` trait (what the application layer calls to get weather data).
- `application/coat_check_service.rs` — orchestrates: fans out to `WeatherPort` concurrently via `try_join_all`, evaluates each forecast, worst-case aggregates across locations.
- `adapters/outbound/open_meteo.rs` — reqwest HTTP client, private serde DTOs, implements `WeatherPort`.
- `adapters/inbound/http.rs` — axum router, request/response DTOs, implements the HTTP → `CoatCheckPort` translation.
- `main.rs` — the only place concrete types (`OpenMeteoClient`, `CoatCheckService`) appear together.

All async traits use `#[async_trait::async_trait]` — required on both trait definition and every `impl` block.

## API

```
POST /coat-check
{ "locations": [{ "lat": 51.5, "lon": -0.1, "label": "Office" }] }

GET /health
```

Response `recommendation` is `"coat"`, `"rain_jacket"`, or `"no"`. Cold triggers → coat; warm + wet → rain jacket. Multiple locations evaluated independently; worst-case wins overall.

Swagger UI served at `/swagger-ui`. OpenAPI spec at `/api-docs/openapi.json`. DTOs use `utoipa::ToSchema`; handler uses `#[utoipa::path]`.
