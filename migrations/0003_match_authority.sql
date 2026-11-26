ALTER TABLE match_results
    ALTER COLUMN submitted_by_player_id DROP NOT NULL,
    ADD COLUMN submitted_by_authority TEXT NOT NULL DEFAULT 'development_player';

ALTER TABLE match_results
    ALTER COLUMN submitted_by_authority DROP DEFAULT,
    ADD CONSTRAINT match_results_submission_authority_check CHECK (
        (submitted_by_authority = 'development_player' AND submitted_by_player_id IS NOT NULL)
        OR
        (submitted_by_authority IN ('game_server', 'administrator') AND submitted_by_player_id IS NULL)
    );
