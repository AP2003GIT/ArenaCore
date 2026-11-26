ALTER TABLE matches
    ADD COLUMN started_at TIMESTAMPTZ,
    ADD COLUMN finished_at TIMESTAMPTZ;

CREATE TABLE match_results (
    match_id UUID PRIMARY KEY REFERENCES matches(id) ON DELETE CASCADE,
    winning_team SMALLINT NOT NULL CHECK (winning_team IN (1, 2)),
    submitted_by_player_id UUID NOT NULL REFERENCES players(id),
    completed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE match_rating_changes (
    match_id UUID NOT NULL REFERENCES match_results(match_id) ON DELETE CASCADE,
    player_id UUID NOT NULL REFERENCES players(id),
    rating_before SMALLINT NOT NULL CHECK (rating_before BETWEEN 0 AND 3000),
    rating_after SMALLINT NOT NULL CHECK (rating_after BETWEEN 0 AND 3000),
    rating_delta SMALLINT NOT NULL,
    PRIMARY KEY (match_id, player_id)
);

CREATE INDEX match_rating_changes_player_idx
    ON match_rating_changes (player_id, match_id);
