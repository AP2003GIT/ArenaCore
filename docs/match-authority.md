# Match Authority Contract

Milestone 8 restricts match lifecycle commands to authenticated actors. The server accepts development player sessions when explicitly enabled, or short-lived Ed25519 credentials signed by a configured game-server or administrator key.

## Configure the server

`ARENA_MATCH_ALLOW_DEV_PARTICIPANTS` defaults to `false`. Set it to `true` only in a trusted local development environment. In production, keep it false and configure the raw 32-byte Ed25519 public keys as unpadded base64url:

```text
ARENA_MATCH_ALLOW_DEV_PARTICIPANTS=false
ARENA_MATCH_GAME_SERVER_PUBLIC_KEY=<base64url public key>
ARENA_MATCH_ADMIN_PUBLIC_KEY=<different base64url public key>
```

Either signed role can be left unconfigured. A key cannot be assigned to both roles. The private signing keys belong in the trusted game-server and administrator environments; ArenaCore only needs the public keys. Changing a configured key requires restarting ArenaCore and invalidates credentials signed by the previous key.

## Sign a command

The trusted caller signs the exact UTF-8 message below with Ed25519, then sends the message and unpadded base64url signature joined by a period as a bearer token:

```text
arenacore-match-v1.<role>.<match_uuid>.<action>.<winner>.<expires_unix_seconds>
```

Fields are case-sensitive. `role` is `game_server` or `administrator`. `match_uuid` is the lowercase, hyphenated UUID used in the route. `action` is `start`, `complete`, or `cancel`. `winner` is `a` or `b` for completion and `-` for start/cancel. The expiration is a Unix timestamp in seconds; it must be in the future and no more than 300 seconds ahead.

Examples of the exact strings to sign:

```text
arenacore-match-v1.game_server.7632f584-2864-49d5-9552-6609db96c16a.start.-.1790000000
arenacore-match-v1.game_server.7632f584-2864-49d5-9552-6609db96c16a.complete.a.1790000000
arenacore-match-v1.administrator.7632f584-2864-49d5-9552-6609db96c16a.cancel.-.1790000000
```

Send the token in `Authorization: Bearer <message>.<signature>`. Tokens are bound to one match, action, and (for completion) winner. A token for one command cannot authorize another command. There is no credential-issuing endpoint; keep signing inside the trusted caller and do not send private keys to ArenaCore or game clients.

For example, after the game server signs a completion command for team A, it sends:

```http
POST /api/v1/matches/7632f584-2864-49d5-9552-6609db96c16a/complete
Authorization: Bearer <signed-token>
Content-Type: application/json

{"winning_team":"a"}
```

## Command permissions and bodies

| Role | Start | Complete | Cancel |
| --- | --- | --- | --- |
| Game server | Allowed | Allowed | Forbidden |
| Administrator | Allowed | Allowed | Allowed |
| Development participant | Allowed when enabled | Allowed when enabled | Allowed when enabled |

For signed credentials, start and cancel requests use `{}`; completion uses `{"winning_team":"a"}` or `{"winning_team":"b"}`. Do not include `player_id`. Development participant requests include `player_id` and must use the player's bearer token from `POST /api/v1/dev/sessions`.

Missing, malformed, expired, incorrectly scoped, or unverifiable credentials return `401 Unauthorized`. A valid game-server credential for cancellation returns `403 Forbidden`. Lifecycle transitions and result submission remain idempotent, so safe retry does not apply rating changes twice.
