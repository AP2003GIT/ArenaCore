# ArenaCore

ArenaCore is an authoritative multiplayer backend built as a Rust modular monolith. Milestones 1 and 2 provide a runnable Axum service, development players, and concurrency-safe in-memory lobbies with membership, readiness, ownership transfer, and lifecycle rules. Matchmaking, WebSocket, and PostgreSQL support are intentionally deferred.

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
- `domain` owns the `Player` and `Lobby` models and their invariants, independent of Axum.
- `service` owns validation and the player/lobby use cases.
- `storage` defines repository boundaries and concurrency-safe in-memory implementations.
- `app` wires routes to application state, while `main` handles process concerns such as configuration, tracing, listening, and shutdown.

The in-memory repositories use standard-library `RwLock` values. Their critical sections contain only short `HashMap` and lobby state-transition operations and never cross an `.await`. Lobby membership and capacity changes share one write lock so concurrent join requests cannot overfill a lobby or place a player in multiple lobbies. The repository traits are asynchronous so later SQLx implementations can perform real I/O without changing the service API.

Player and lobby data are lost whenever the process restarts; persistence belongs to a later milestone.

## Checks and tests

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests exercise health, player behavior, validation, malformed JSON, Unicode name length, concurrent player creation, lobby capacity, idempotent joins, single-lobby membership, readiness transitions, ownership transfer, closure, and concurrent joins. They do not require a database or external game client.

