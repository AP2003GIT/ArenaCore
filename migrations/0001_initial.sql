CREATE TABLE players (
    id UUID PRIMARY KEY,
    display_name TEXT NOT NULL,
    rating SMALLINT NOT NULL CHECK (rating BETWEEN 0 AND 3000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE lobbies (
    id UUID PRIMARY KEY,
    owner_player_id UUID REFERENCES players(id),
    capacity SMALLINT NOT NULL CHECK (capacity BETWEEN 2 AND 8),
    status TEXT NOT NULL CHECK (status IN ('open', 'ready', 'closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (status = 'closed' AND owner_player_id IS NULL)
        OR (status <> 'closed' AND owner_player_id IS NOT NULL)
    )
);

CREATE TABLE lobby_memberships (
    lobby_id UUID NOT NULL REFERENCES lobbies(id) ON DELETE CASCADE,
    player_id UUID NOT NULL REFERENCES players(id),
    ready BOOLEAN NOT NULL DEFAULT FALSE,
    joined_order BIGINT GENERATED ALWAYS AS IDENTITY,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (lobby_id, player_id),
    UNIQUE (player_id)
);

CREATE INDEX lobby_memberships_lobby_order_idx
    ON lobby_memberships (lobby_id, joined_order);

CREATE TABLE matchmaking_queue (
    player_id UUID PRIMARY KEY REFERENCES players(id),
    mode TEXT NOT NULL CHECK (mode IN ('1v1', '2v2')),
    rating SMALLINT NOT NULL CHECK (rating BETWEEN 0 AND 3000),
    enqueued_at_seconds BIGINT NOT NULL CHECK (enqueued_at_seconds >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX matchmaking_queue_mode_order_idx
    ON matchmaking_queue (mode, enqueued_at_seconds, player_id);

CREATE TABLE matches (
    id UUID PRIMARY KEY,
    mode TEXT NOT NULL CHECK (mode IN ('1v1', '2v2')),
    status TEXT NOT NULL CHECK (status IN ('waiting', 'ready', 'active', 'completed', 'cancelled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE match_players (
    match_id UUID NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    player_id UUID NOT NULL REFERENCES players(id),
    team SMALLINT NOT NULL CHECK (team IN (1, 2)),
    rating SMALLINT NOT NULL CHECK (rating BETWEEN 0 AND 3000),
    PRIMARY KEY (match_id, player_id)
);

CREATE INDEX match_players_player_idx ON match_players (player_id);

CREATE TABLE match_history (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    match_id UUID NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    from_status TEXT,
    to_status TEXT NOT NULL CHECK (to_status IN ('waiting', 'ready', 'active', 'completed', 'cancelled')),
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE INDEX match_history_match_time_idx
    ON match_history (match_id, occurred_at, id);

CREATE TABLE player_activities (
    player_id UUID PRIMARY KEY REFERENCES players(id) ON DELETE CASCADE,
    activity_type TEXT NOT NULL CHECK (activity_type IN ('lobby', 'queued', 'match')),
    activity_id UUID,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (activity_type = 'queued' AND activity_id IS NULL)
        OR (activity_type IN ('lobby', 'match') AND activity_id IS NOT NULL)
    )
);

CREATE INDEX player_activities_type_idx ON player_activities (activity_type);
