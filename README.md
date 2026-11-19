# ArenaCore

ArenaCore is an authoritative multiplayer backend built as a Rust modular monolith. Milestones 1 through 3 provide a runnable Axum service, development players, concurrency-safe lobbies, and deterministic 1v1 and 2v2 skill matchmaking. WebSocket delivery and PostgreSQL persistence are intentionally deferred.

## Prerequisites

- A current stable Rust toolchain with Cargo
- `curl` or another HTTP client

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
| `ARENA_MATCH_BASE_GAP` | `100` | Initial allowed rating difference |
| `ARENA_MATCH_GAP_GROWTH_PER_SECOND` | `10` | Rating points added to the window per second waited |
| `ARENA_MATCH_MAX_GAP` | `1000` | Maximum allowed rating difference |

PowerShell example:

```powershell
$env:ARENA_PORT = "8080"
$env:RUST_LOG = "arena_core=debug"
cargo run
```

The included `.env.example` documents these values. The service intentionally does not load `.env` automatically, which keeps configuration behavior explicit across local and production environments.

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
- `service` owns validation and the player, lobby, and matchmaking use cases.
- `storage` defines repository boundaries and concurrency-safe in-memory implementations.
- `app` wires routes to application state, while `main` handles process concerns such as configuration, tracing, listening, and shutdown.

The in-memory repositories use standard-library `RwLock` values. Their critical sections contain only short state transitions and never cross an `.await`. A shared player-activity registry atomically prevents lobby, queue, and match membership from overlapping. Match formation and queue removal occur under one narrow write lock, so concurrent requests cannot assign a player twice. The repository traits are asynchronous so later SQLx implementations can perform real I/O without changing the service API.

Player, lobby, queue, and match data are lost whenever the process restarts; persistence belongs to a later milestone.

## Checks and tests

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests exercise health, player behavior, validation, concurrent creation and joins, lobby lifecycle rules, cross-activity conflicts, duplicate queue defense, idempotent cancellation, 1v1 assignment, wait-based gap expansion, deterministic 2v2 balancing, status reads, and match reads. They do not require a database or external game client.

