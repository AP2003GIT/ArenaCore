# ArenaCore

ArenaCore is an authoritative multiplayer backend built as a Rust modular monolith. Milestones 1 through 6 provide a runnable Axum service, development players, concurrency-safe lobbies, deterministic 1v1 and 2v2 skill matchmaking, authenticated real-time notifications, PostgreSQL persistence, operational safeguards, a deterministic load harness, and game-engine integration contracts.

## Prerequisites

- A current stable Rust toolchain with Cargo
- `curl` or another HTTP client
- Docker with Compose for the PostgreSQL deployment path

Install Rust from [rustup.rs](https://rustup.rs/) if `cargo --version` is not available.

## Run locally

```bash
cargo run
```

The server listens on `127.0.0.1:3000` by default. Stop it with `Ctrl+C`; the server waits for in-flight work to finish before exiting.

Configuration uses environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `ARENA_HOST` | `127.0.0.1` | IP address on which the server listens |
| `ARENA_PORT` | `3000` | TCP port on which the server listens |
| `RUST_LOG` | `arena_core=info` | Tracing filter |
| `ARENA_STORAGE` | `memory` | Repository backend: `memory` or `postgres` |
| `ARENA_DATABASE_URL` | none | PostgreSQL URL; required when storage is `postgres` |
| `ARENA_DB_MAX_CONNECTIONS` | `10` | Maximum SQLx pool connections |
| `ARENA_DB_ACQUIRE_TIMEOUT_SECONDS` | `5` | Maximum wait for a pooled connection |
| `ARENA_RATE_LIMIT_REQUESTS` | `120` | Requests allowed per client in one rate-limit window |
| `ARENA_RATE_LIMIT_WINDOW_SECONDS` | `60` | Rate-limit window length |
| `ARENA_MATCH_BASE_GAP` | `100` | Initial allowed rating difference |
| `ARENA_MATCH_GAP_GROWTH_PER_SECOND` | `10` | Rating points added to the window per second waited |
| `ARENA_MATCH_MAX_GAP` | `1000` | Maximum allowed rating difference |
| `ARENA_WS_CHANNEL_CAPACITY` | `32` | Maximum pending events per WebSocket connection |
| `ARENA_WS_HEARTBEAT_SECONDS` | `15` | Interval between server ping frames |
| `ARENA_WS_CLIENT_TIMEOUT_SECONDS` | `45` | Maximum time without client traffic before disconnect |

PowerShell example:

```powershell
$env:ARENA_PORT = "8080"
$env:RUST_LOG = "arena_core=debug"
cargo run
```

The included `.env.example` documents these values. The service intentionally does not load `.env` automatically, which keeps configuration behavior explicit across local and production environments.

## Deploy with Docker Compose

Build and start the API with PostgreSQL:

```bash
docker compose up --build -d
docker compose ps
```

The Compose stack exposes the API on `127.0.0.1:3000` and PostgreSQL on `127.0.0.1:55432`. The non-default host database port avoids collisions with an existing PostgreSQL installation. Compose waits for the database health check before starting ArenaCore. The application embeds and applies SQLx migrations during startup, and `/ready` stays unavailable when PostgreSQL cannot answer a probe.

Check the deployed services:

```bash
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3000/ready
curl http://127.0.0.1:3000/metrics
```

Stop the stack while retaining PostgreSQL data:

```bash
docker compose down
```

For a host-run API with only PostgreSQL in Docker, start `docker compose up -d postgres`, set `ARENA_STORAGE=postgres` and `ARENA_DATABASE_URL=postgresql://arena:arena@127.0.0.1:55432/arenacore`, then run `cargo run`.

## API examples

Check health:

```bash
curl http://127.0.0.1:3000/health
```

Create a development player:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/players \
  -H "Content-Type: application/json" \
  -d '{"display_name":"Ada","rating":1200}'
```

Read a player, replacing the UUID with the `id` returned by the create request:

```bash
curl http://127.0.0.1:3000/api/v1/players/00000000-0000-0000-0000-000000000000
```

`display_name` is trimmed and must contain 3 to 24 Unicode characters. `rating` is optional, defaults to `1200`, and must be between `0` and `3000` inclusive.

Create a lobby, replacing `owner_player_id` with an existing player UUID:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/lobbies \
  -H "Content-Type: application/json" \
  -d '{"owner_player_id":"00000000-0000-0000-0000-000000000000","capacity":4}'
```

Join a lobby:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/lobbies/00000000-0000-0000-0000-000000000000/join \
  -H "Content-Type: application/json" \
  -d '{"player_id":"00000000-0000-0000-0000-000000000000"}'
```

Set a member's ready state:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/lobbies/00000000-0000-0000-0000-000000000000/ready \
  -H "Content-Type: application/json" \
  -d '{"player_id":"00000000-0000-0000-0000-000000000000","ready":true}'
```

Leave a lobby by posting the same `player_id` to `/api/v1/lobbies/{lobby_id}/leave`. Read current state with `GET /api/v1/lobbies/{lobby_id}`.

Lobby capacity defaults to `4` and must be between `2` and `8`. The owner is the first member. Repeating a join for the same lobby is idempotent; a player cannot belong to two lobbies. If the owner leaves, ownership transfers to the longest-tenured remaining member. A lobby becomes `ready` when it has at least two members and every member is ready, returns to `open` when that condition stops being true, and becomes `closed` when its last member leaves.

Enqueue an existing player for a 1v1 match:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/matchmaking/enqueue \
  -H "Content-Type: application/json" \
  -d '{"player_id":"00000000-0000-0000-0000-000000000000","mode":"1v1"}'
```

Use `"2v2"` for four-player matchmaking. Check the player's current queue or match assignment:

```bash
curl http://127.0.0.1:3000/api/v1/matchmaking/status/00000000-0000-0000-0000-000000000000
```

Cancel queue membership. Cancellation is idempotent and returns `204 No Content`:

```bash
curl -X DELETE http://127.0.0.1:3000/api/v1/matchmaking/queue/00000000-0000-0000-0000-000000000000
```

When status becomes `matched`, read the immutable match snapshot:

```bash
curl http://127.0.0.1:3000/api/v1/matches/00000000-0000-0000-0000-000000000000
```

Each player can have only one active activity: lobby membership, queue membership, or a match assignment. Duplicate queue requests and conflicting lobby operations return `409 Conflict` with machine-readable codes.

### Matchmaking policy

The 1v1 and 2v2 queues are independent. The matcher considers players in enqueue order, selects the oldest feasible anchor, and prefers the closest ratings. Two players are compatible when their rating difference fits the larger of their current wait-adjusted windows. This lets a long-waiting player gradually accept a wider skill range without immediately degrading match quality for new arrivals.

For 2v2, the matcher first selects four mutually compatible players with the smallest rating spread. It then enumerates the three unique two-team partitions and minimizes the absolute difference between team rating totals. UUID ordering provides deterministic tie-breaks. A one-second background tick reevaluates queues so waiting players can match after their rating windows expand even when no new enqueue request arrives.

Matches enter `ready` and retain the players' ratings at assignment time. Completion, cancellation after assignment, and rating updates belong to later match-lifecycle work.

### Real-time notifications

Issue a development access token for an existing player:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/dev/sessions \
  -H "Content-Type: application/json" \
  -d '{"player_id":"00000000-0000-0000-0000-000000000000"}'
```

Connect to `ws://127.0.0.1:3000/api/v1/ws?player_id={player_id}` with the response token in an `Authorization: Bearer {access_token}` header. A token is bound to exactly one player, so changing `player_id` cannot subscribe to another player's events. The session endpoint and its in-memory bearer tokens are development scaffolding; production deployments should replace them with the platform identity provider and short-lived signed credentials.

The server sends JSON text frames with a stable envelope:

```json
{
  "type": "match_found",
  "event_id": "00000000-0000-0000-0000-000000000000",
  "occurred_at": "2026-11-19T12:00:00Z",
  "payload": {}
}
```

Current event types are `lobby_updated`, `queue_updated`, and `match_found`. Lobby membership and ready-state changes notify all affected members. Matchmaking events are emitted from the service layer for both HTTP-triggered and background matches.

Each connection has its own bounded event channel. A connection that cannot consume events before its channel fills is removed instead of allowing unbounded memory growth. The server sends ping frames at the configured heartbeat interval, treats pong or other client traffic as liveness, and unregisters the subscription on close, transport failure, timeout, or backpressure removal. Clients should reconnect and fetch current HTTP state after any disconnect because notification delivery is intentionally ephemeral.

### Operations

`GET /health` is a process-liveness check. `GET /ready` verifies the configured storage backend and returns `503 Service Unavailable` if PostgreSQL is unreachable. `GET /metrics` returns Prometheus text metrics for request counts, 4xx/5xx responses, rate-limit rejections, aggregate request duration, and active WebSocket connections.

Rate limiting applies to `/api/v1` routes and uses the connection peer IP when the server is running normally. Operational endpoints remain available to orchestrators and monitoring systems. Rejected requests return `429 Too Many Requests`, a `Retry-After` header, and the standard `rate_limited` error envelope. The built-in limiter is intentionally process-local; a multi-replica deployment should enforce a shared limit at the gateway or use a distributed store.

Errors use a stable envelope:

```json
{
  "error": {
    "code": "player_not_found",
    "message": "player was not found"
  }
}
```

## Architecture

- `api` owns HTTP handlers, request/response DTOs, and transport error mapping.
- `domain` owns players, lobbies, queues, match snapshots, matching policy, and state invariants, independent of Axum.
- `service` owns validation, player/lobby/matchmaking use cases, development sessions, and notification fan-out.
- `storage` defines repository boundaries plus in-memory and SQLx PostgreSQL implementations.
- `operations` owns request metrics and the bounded per-client rate limiter.
- `app` wires routes to application state, while `main` handles process concerns such as configuration, tracing, listening, and shutdown.

The in-memory repositories use standard-library `RwLock` values. Their critical sections contain only short state transitions and never cross an `.await`. A shared player-activity registry atomically prevents lobby, queue, and match membership from overlapping.

The PostgreSQL implementation stores players, lobbies, memberships, queue entries, match snapshots, match players, active-player ownership, and append-only match history. Lobby mutations lock the affected rows and commit membership, owner, status, and activity changes in one transaction. Matchmaking uses one PostgreSQL advisory transaction lock to serialize queue selection across API and background workers; this favors correctness and simple recovery over maximum matching throughput. Match creation, queue removal, player activity updates, and the first history entry commit atomically.

Memory mode remains the default for fast development and database-free tests. Development bearer sessions and live WebSocket delivery are process-local in both modes.

## Game integration and load testing

The engine-neutral client contract documents the exact HTTP payloads, player-bound WebSocket handshake, reconnect behavior, retry rules, and current boundary between matchmaking and gameplay. Small reference clients show the same request flow with Unity `UnityWebRequest`, Unreal `FHttpModule`, and Godot `HTTPRequest`:

- [Game client contract](docs/game-client-contract.md)
- [Unity example](examples/clients/unity/ArenaCoreClient.cs)
- [Unreal example](examples/clients/unreal/ArenaCoreClient.cpp)
- [Godot example](examples/clients/godot/arena_core_client.gd)

Run the four-player correctness scenario against a healthy deployment:

```bash
cargo run --release --bin arena-load -- \
  --scenario load/scenarios/smoke.json \
  --output target/load-reports/smoke.json
```

The harness supports health throughput and deterministic 1v1 or 2v2 workloads. Reports include operation-level latency percentiles, HTTP throughput, failure counts, assignments, matches, WebSocket handshakes, and `match_found` delivery. For intentional high-volume runs, start Compose with the load override:

```bash
docker compose -f compose.yaml -f compose.load.yaml up --build -d
```

See [load testing and profiling](docs/load-testing.md) for the scenario schema, reproducible benchmark procedure, report interpretation, symbol-rich profiling build, and database investigation workflow.

## Checks and tests

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The default suite exercises health/readiness, metrics, rate limits, player behavior, validation, concurrent creation and joins, lobby lifecycle rules, cross-activity conflicts, duplicate queue defense, idempotent cancellation, 1v1 assignment, wait-based gap expansion, deterministic 2v2 balancing, status reads, match reads, token/player binding, WebSocket event delivery, heartbeat handling, bounded-channel backpressure, disconnect cleanup, load-scenario validation, deterministic simulated ratings, and latency percentile calculations.

The PostgreSQL integration test is opt-in so the normal suite remains database-free:

```bash
docker compose up -d postgres
ARENA_TEST_DATABASE_URL=postgresql://arena:arena@127.0.0.1:55432/arenacore cargo test --test postgres
```

The database test applies migrations, clears only the configured test database, stresses concurrent joins, verifies cross-activity conflicts, forms and reloads a match, checks history, and reconnects through a fresh pool. Use a dedicated disposable database URL in CI.

