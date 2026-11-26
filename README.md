# ArenaCore

ArenaCore is the backend for an online multiplayer game. It helps a game create players, gather them into lobbies, find fair matches, track match results, and keep player ratings up to date.

You can use it with a game made in Unity, Unreal, Godot, or another engine. ArenaCore handles the online services around a match; it does not run the game itself or simulate movement and combat.

## What it does

- Creates and stores player profiles and skill ratings.
- Lets players join lobbies or enter matchmaking.
- Finds balanced 1v1 and 2v2 matches.
- Tracks each match from ready to active to completed or cancelled.
- Updates ratings after a match and keeps a record of the result.
- Sends live lobby and match updates to connected players.
- Saves data in PostgreSQL when running in database mode.

## Start ArenaCore

You can run the service with Docker, or run it directly with Rust.

### Option 1: Docker Compose

Install Docker Desktop, open a terminal in this folder, and run:

```bash
docker compose up --build -d
```

This starts the API and PostgreSQL. The API is at `http://127.0.0.1:3000`; PostgreSQL is available on port `55432` on your computer. ArenaCore applies its database updates automatically when it starts.

Check that it is running:

```bash
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3000/ready
```

The first address checks that the API is running. The second checks that it can reach its database. To stop the services while keeping the saved database data, run:

```bash
docker compose down
```

For local experiments that let players submit their own match results, enable the development option before starting Compose. Keep this option off for a public or production server:

```powershell
$env:ARENA_MATCH_ALLOW_DEV_PARTICIPANTS = "true"
docker compose up --build -d
```

Compose reads that setting from your environment. The default is `false`.

### Option 2: Run with Rust

Install a stable Rust toolchain, then run:

```bash
cargo run
```

The API listens on `http://127.0.0.1:3000`. This quick-start uses temporary in-memory storage: its data is cleared when the server stops. To use PostgreSQL, first start it with `docker compose up -d postgres`, then set the database settings before running ArenaCore. In PowerShell:

```powershell
$env:ARENA_STORAGE = "postgres"
$env:ARENA_DATABASE_URL = "postgresql://arena:arena@127.0.0.1:55432/arenacore"
cargo run
```

## How a match works

```mermaid
flowchart LR
    A[Create players] --> B[Join a lobby or matchmaking queue]
    B --> C[Balanced match is created]
    C --> D[Match starts]
    D --> E[Match completes]
    E --> F[Ratings and result are saved]
    C --> G[Match is cancelled]
    D --> G
```

The API is organized around a few main actions:

| What you want to do | Example endpoint |
| --- | --- |
| Create a player | `POST /api/v1/players` |
| Create or join a lobby | `POST /api/v1/lobbies` and `POST /api/v1/lobbies/{id}/join` |
| Find a match | `POST /api/v1/matchmaking/enqueue` |
| Check a player's queue or match | `GET /api/v1/matchmaking/status/{player_id}` |
| Read a match | `GET /api/v1/matches/{match_id}` |
| Start, complete, or cancel a match | `POST /api/v1/matches/{match_id}/start`, `/complete`, or `/cancel` |

The API uses JSON. For example, create a player with:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/players \
  -H "Content-Type: application/json" \
  -d '{"display_name":"Ada","rating":1200}'
```

The response includes the new player's `id`. Use that ID in later requests. To queue the player for a 1v1 match, send:

```bash
curl -X POST http://127.0.0.1:3000/api/v1/matchmaking/enqueue \
  -H "Content-Type: application/json" \
  -d '{"player_id":"<PLAYER_ID>","mode":"1v1"}'
```

Use `"2v2"` to make a four-player match. When enough compatible players are queued, ArenaCore creates the match and assigns teams. Check the player's status to get its match ID.

## Who can report match results?

Match commands require a bearer token. By default, players cannot submit match results themselves.

For local development only, set `ARENA_MATCH_ALLOW_DEV_PARTICIPANTS=true` before starting the service. A participant can then use a short-lived development session token from `POST /api/v1/dev/sessions` to start or complete their match. The same option lets a participant cancel a match.

For a real deployment, keep that option off and configure an Ed25519 public key for the game server. The game server uses its private key to create short-lived credentials scoped to one match and one action. It can start and complete matches. A separate administrator key can also authorize cancellation. Private keys stay with the trusted game server or administrator; ArenaCore only receives the public keys.

See the [match authority guide](docs/match-authority.md) for the credential format and [game client contract](docs/game-client-contract.md) for request details and Unity, Unreal, and Godot examples.

## Live updates

Game clients can connect to ArenaCore's WebSocket endpoint to receive events such as `match_found`, `match_started`, and `match_completed`. Clients should read the HTTP resource after reconnecting because these live events are not saved for later replay.

## Configuration

The server reads configuration from environment variables. The main settings are:

| Setting | Default | What it controls |
| --- | --- | --- |
| `ARENA_HOST` / `ARENA_PORT` | `127.0.0.1` / `3000` | API address |
| `ARENA_STORAGE` | `memory` | `memory` for temporary local data, or `postgres` for saved database data |
| `ARENA_DATABASE_URL` | unset | PostgreSQL connection string when database mode is enabled |
| `ARENA_MATCH_ALLOW_DEV_PARTICIPANTS` | `false` | Whether participants may submit match commands with development sessions |
| `ARENA_MATCH_GAME_SERVER_PUBLIC_KEY` | unset | Game-server Ed25519 public key in unpadded base64url format |
| `ARENA_MATCH_ADMIN_PUBLIC_KEY` | unset | Optional administrator Ed25519 public key in unpadded base64url format |
| `ARENA_RATING_K_FACTOR` | `32` | How strongly a match result changes player ratings |

See [.env.example](.env.example) for all available settings. When running the Rust program directly, set variables in your shell; the program does not automatically load a `.env` file. Docker Compose reads a `.env` file for the match-authority settings it passes into the container.

## Useful project guides

- [Match authority guide](docs/match-authority.md) — permissions, keys, and signed match commands.
- [Game client contract](docs/game-client-contract.md) — API payloads, reconnect behavior, and engine examples.
- [Load testing guide](docs/load-testing.md) — repeatable performance scenarios and profiling.

## For contributors

Run the formatting, lint, and test checks with:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Most contributors can use `cargo test` without extra setup. The optional PostgreSQL integration test clears its tables, so only run it with `ARENA_TEST_DATABASE_URL` pointed at a dedicated, disposable database.

ArenaCore is a Rust modular monolith: the API handles web requests, services coordinate the game's rules, domain code defines players and matches, and storage saves the data. This keeps the first version straightforward to run while leaving room to split services later if the game needs it.
