# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                        # compile
cargo test                         # all tests
cargo test <name>                  # single test by name substring
cargo fmt                          # auto-format
cargo fmt --check                  # CI format check (exits non-zero if changes needed)
cargo clippy -- -D warnings        # lint (treat warnings as errors)
cargo run                          # start server (default port 3000)
PORT=8080 cargo run                # custom port
```

## Architecture

Hexagonal / ports-and-adapters with a strict one-way dependency rule:

```
domain/ ← application/ ← ports/ ← adapters/
```

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
