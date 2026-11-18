# ArenaCore

ArenaCore is an authoritative multiplayer backend built as a Rust modular monolith. Milestone 1 provides a runnable Axum service, a health check, and an in-memory development player API. Lobby, matchmaking, WebSocket, and PostgreSQL support are intentionally deferred.

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
- `domain` owns the `Player` model and is independent of Axum.
- `service` owns validation and player use cases.
- `storage` defines the repository boundary and the concurrency-safe in-memory implementation.
- `app` wires routes to application state, while `main` handles process concerns such as configuration, tracing, listening, and shutdown.

The in-memory repository uses a standard-library `RwLock`. Its critical sections contain only short `HashMap` operations and never cross an `.await`. The repository trait is asynchronous so a later SQLx implementation can perform real I/O without changing the service API.

Player data is lost whenever the process restarts; persistence belongs to a later milestone.

## Checks and tests

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The tests exercise health, create/read behavior, missing players, invalid fields, malformed JSON, Unicode name length, defaults, and concurrent player creation. They do not require a database or external game client.

