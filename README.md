# coat-check

A REST API that tells you whether to bring a coat today. Send your locations, get back a recommendation: **coat**, **rain jacket**, or **nothing**.

Queries [Open-Meteo](https://open-meteo.com/) (free, no API key) for daily forecasts and evaluates them against sensible thresholds.

## Quick start

```bash
cargo run
```

Server starts on port 3000 (override with `PORT=8080 cargo run`).

```bash
curl -s -X POST http://localhost:3000/coat-check \
  -H 'Content-Type: application/json' \
  -d '{
    "locations": [
      { "lat": 51.5074, "lon": -0.1278, "label": "London" },
      { "lat": 52.4862, "lon": -1.8904, "label": "Birmingham" }
    ]
  }' | jq .
```

```json
{
  "recommendation": "coat",
  "reason": "London: feels like as low as 8.3°C (threshold 12°C)",
  "locations": [
    {
      "label": "London",
      "recommendation": "coat",
      "reasons": ["feels like as low as 8.3°C (threshold 12°C)"],
      "temp_max_celsius": 13.2,
      "..."
    },
    {
      "label": "Birmingham",
      "recommendation": "no",
      "reasons": [],
      "..."
    }
  ]
}
```

Multiple locations are evaluated independently. The overall recommendation is the worst case — if any location says coat, pack one.

## How it decides

| Trigger | Threshold | Result |
|---------|-----------|--------|
| Feels-like min | < 12°C | Coat |
| Temp max | < 15°C | Coat |
| Wind speed max | ≥ 40 km/h | Coat |
| Snowfall | > 0 cm | Coat |
| Precipitation | ≥ 1 mm | Rain jacket |
| Thunderstorm (WMO code) | ≥ 95 | Rain jacket |

Cold triggers → coat. Wet-only triggers on a warm day → rain jacket.

## API

### `POST /coat-check`

**Request body:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `locations` | array | yes | One or more locations |
| `locations[].lat` | float | yes | Latitude (-90 to 90) |
| `locations[].lon` | float | yes | Longitude (-180 to 180) |
| `locations[].label` | string | no | Human-readable name |

**Response:**

| Field | Description |
|-------|-------------|
| `recommendation` | `"coat"`, `"rain_jacket"`, or `"no"` |
| `reason` | Human-readable summary |
| `locations[]` | Per-location breakdown with weather data |

**Status codes:** 200 success, 422 invalid input, 502 weather service unavailable.

### `GET /health`

Returns `ok`.

### Swagger UI

Available at `/swagger-ui` when the server is running. OpenAPI spec at `/api-docs/openapi.json`.

## Development

```bash
cargo test                         # run tests
cargo fmt                          # format
cargo clippy -- -D warnings        # lint
cargo llvm-cov                     # coverage report
cargo llvm-cov --html              # HTML coverage report
```

### Commits

This repo uses [Conventional Commits](https://www.conventionalcommits.org/). CI will reject PRs with non-conforming messages.

```
feat: add humidity factor to recommendation
fix: handle empty forecast response from Open-Meteo
chore: update dependencies
feat!: change response field from "yes" to "coat"
```

Use scopes to target a specific crate. This tells Release Please which package to bump:

```
feat(coat-check): add wind chill factor
fix(coat-check): handle empty forecast response
```

Without a scope, Release Please infers the package from which files changed in the commit.

PRs are squash-merged, so the **PR title** becomes the commit message on `main`. Make sure PR titles follow the conventional commit format.

Releases are managed by [Release Please](https://github.com/googleapis/release-please). When PRs merge to `main`, it opens a Release PR that bumps versions and generates a changelog. Merge that PR to cut a release.

## Architecture

Hexagonal (ports and adapters). Domain logic has zero I/O dependencies.

```
src/
├── domain/          # Pure logic — the coat algorithm, value objects
├── ports/           # Trait boundaries (inbound + outbound)
├── application/     # Orchestration — fans out to weather port, aggregates
├── adapters/        # HTTP server (axum) + Open-Meteo client (reqwest)
└── main.rs          # Wires concrete types together
```
