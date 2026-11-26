# ArenaCore Game Client Contract

This contract describes how an Unreal, Unity, or Godot client integrates with the current ArenaCore backend. HTTP responses are authoritative. WebSocket events reduce polling latency but do not replace an HTTP state read after reconnects or suspected event loss.

## Transport and versioning

- Development base URL: `http://127.0.0.1:3000`
- Versioned API prefix: `/api/v1`
- JSON field names: `snake_case`
- Resource identifiers: UUID strings
- Operational endpoints: `/health`, `/ready`, and `/metrics`
- WebSocket endpoint: `ws://127.0.0.1:3000/api/v1/ws?player_id={uuid}`

Production clients should use HTTPS and WSS. They should treat new JSON fields as backward-compatible additions and ignore fields they do not understand.

## Current client flow

1. Create or retrieve a development player.
2. Create a development session for that player.
3. Open the player's authenticated WebSocket connection.
4. Join a lobby or enter matchmaking, but never both at once.
5. When `match_found` arrives, read `GET /api/v1/matches/{match_id}`.
6. Preserve the returned teams and assignment-time rating snapshot for the match screen.
7. Start the match, then complete it with a winning team or cancel it.
8. Refresh the match and player resources after the terminal lifecycle event.

The development session endpoint is scaffolding. A production identity provider should replace it with short-lived signed credentials without changing the player-bound authorization rule.

## Player and session payloads

Create a player with `POST /api/v1/players`:

```json
{
  "display_name": "Ada",
  "rating": 1200
}
```

The successful `201 Created` response is:

```json
{
  "id": "ff3db0fd-65f1-4a8e-b5da-10f6fa109882",
  "display_name": "Ada",
  "rating": 1200
}
```

Create a development session with `POST /api/v1/dev/sessions`:

```json
{
  "player_id": "ff3db0fd-65f1-4a8e-b5da-10f6fa109882"
}
```

Use the returned token as `Authorization: Bearer {access_token}` during the WebSocket upgrade. The token can subscribe only to its bound player.

## Lobby operations

| Action | Method and path | Request body |
| --- | --- | --- |
| Create | `POST /api/v1/lobbies` | `{"owner_player_id":"uuid","capacity":4}` |
| Read | `GET /api/v1/lobbies/{lobby_id}` | none |
| Join | `POST /api/v1/lobbies/{lobby_id}/join` | `{"player_id":"uuid"}` |
| Leave | `POST /api/v1/lobbies/{lobby_id}/leave` | `{"player_id":"uuid"}` |
| Ready | `POST /api/v1/lobbies/{lobby_id}/ready` | `{"player_id":"uuid","ready":true}` |

Joining the same lobby twice and leaving an already-left lobby are safe to retry. Clients should still disable duplicate UI actions while a request is in flight.

## Matchmaking operations

Enqueue with `POST /api/v1/matchmaking/enqueue`:

```json
{
  "player_id": "ff3db0fd-65f1-4a8e-b5da-10f6fa109882",
  "mode": "1v1"
}
```

The supported modes are `1v1` and `2v2`. Read the latest assignment with `GET /api/v1/matchmaking/status/{player_id}` and cancel a queued player with `DELETE /api/v1/matchmaking/queue/{player_id}`.

When status is `matched`, `match_id` is present:

```json
{
  "player_id": "ff3db0fd-65f1-4a8e-b5da-10f6fa109882",
  "status": "matched",
  "mode": "1v1",
  "match_id": "7632f584-2864-49d5-9552-6609db96c16a"
}
```

The match resource preserves its assignment snapshot while its status and optional result advance through the lifecycle:

```json
{
  "id": "7632f584-2864-49d5-9552-6609db96c16a",
  "mode": "1v1",
  "status": "ready",
  "team_a": {
    "total_rating": 1200,
    "players": [{"player_id":"ff3db0fd-65f1-4a8e-b5da-10f6fa109882","rating":1200}]
  },
  "team_b": {
    "total_rating": 1220,
    "players": [{"player_id":"744fef70-31f4-447d-a5a9-ce7a82e97093","rating":1220}]
  },
  "result": null
}
```

## Match lifecycle operations

| Action | Method and path | Request body |
| --- | --- | --- |
| Start | `POST /api/v1/matches/{match_id}/start` | Development: `{"player_id":"uuid"}`; signed authority: `{}` |
| Complete | `POST /api/v1/matches/{match_id}/complete` | Development: `{"player_id":"uuid","winning_team":"a"}`; signed authority: `{"winning_team":"a"}` |
| Cancel | `POST /api/v1/matches/{match_id}/cancel` | Development: `{"player_id":"uuid"}`; signed authority: `{}` |

Development participant commands require an `Authorization: Bearer {access_token}` header issued for that same player and are disabled unless `ARENA_MATCH_ALLOW_DEV_PARTICIPANTS=true`. In a deployed environment, send a short-lived signed bearer credential scoped to the match and command. Game servers may start and complete; administrators may also cancel. Signed-authority request bodies omit `player_id`. See the [match authority contract](match-authority.md) for the canonical signed message and key configuration.

A match starts as `ready`, becomes `active`, and can then become `completed`; either `ready` or `active` may instead become `cancelled`. A successful completion includes the winner and auditable Elo changes:

```json
{
  "id": "7632f584-2864-49d5-9552-6609db96c16a",
  "mode": "1v1",
  "status": "completed",
  "team_a": {
    "total_rating": 1200,
    "players": [{"player_id":"ff3db0fd-65f1-4a8e-b5da-10f6fa109882","rating":1200}]
  },
  "team_b": {
    "total_rating": 1200,
    "players": [{"player_id":"744fef70-31f4-447d-a5a9-ce7a82e97093","rating":1200}]
  },
  "result": {
    "winning_team": "a",
    "rating_changes": [
      {"player_id":"ff3db0fd-65f1-4a8e-b5da-10f6fa109882","rating_before":1200,"rating_after":1216,"delta":16},
      {"player_id":"744fef70-31f4-447d-a5a9-ce7a82e97093","rating_before":1200,"rating_after":1184,"delta":-16}
    ]
  }
}
```

Repeating start, cancellation, or the same completion is safe. A completion with a conflicting winner returns `409 Conflict`. Terminal transitions release every participant from the match activity.

## WebSocket behavior

Events use this envelope:

```json
{
  "type": "match_found",
  "event_id": "3cb3e31c-cbdc-44cc-861f-c36c1376e8b2",
  "occurred_at": "2026-11-21T12:00:00Z",
  "payload": {}
}
```

Current event types are `lobby_updated`, `queue_updated`, `match_found`, `match_started`, `match_completed`, and `match_cancelled`. Lifecycle payloads include the match status and teams, plus the winner and rating changes when completed. Clients must answer protocol ping frames with pong frames. After a disconnect they should reconnect with bounded exponential backoff, then fetch lobby, matchmaking, match, and player state over HTTP. Event delivery is ephemeral and is not a durable replay log.

## Errors and retry policy

Errors use a stable envelope:

```json
{
  "error": {
    "code": "player_not_found",
    "message": "player was not found"
  }
}
```

- Do not retry validation failures (`400` or `422`) without changing the request.
- Refresh local state after a domain conflict (`409`).
- Respect `Retry-After` after `429 Too Many Requests`.
- Retry transient network failures and `5xx` responses with jittered exponential backoff.
- Put a client-side timeout on every request and expose a cancel path in the UI.

## Engine mapping

| Engine | HTTP facility | WebSocket approach | Serialization |
| --- | --- | --- | --- |
| Unity | `UnityWebRequest` | A maintained WebSocket package or platform transport adapter | `JsonUtility` or the project's JSON package |
| Unreal | `FHttpModule` | `WebSockets` module and `IWebSocket` | `FJsonObject` and `FJsonSerializer` |
| Godot | `HTTPRequest` | `WebSocketPeer` | `JSON.stringify` and `JSON.parse_string` |

Reference HTTP snippets live under `examples/clients`. They demonstrate payload construction and response handling, but remain intentionally small so each game can wrap them in its own scene, subsystem, cancellation, and credential model.

## Deferred gameplay protocol

ArenaCore does not yet allocate a dedicated game server or transport gameplay frames. Do not send movement, combat, physics, or anti-cheat data through the lobby WebSocket. The current lifecycle endpoints record control-plane state and ratings; they are not a gameplay transport. A later protocol should be designed only after load results establish connection counts, acceptable match-start latency, deployment topology, and the required authority model.
