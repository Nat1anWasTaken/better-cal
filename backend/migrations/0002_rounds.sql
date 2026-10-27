-- Leaderboard rounds. A round ranks students on one set's problems by the
-- submits made between started_at and ended_at; at most one is open.

CREATE TABLE rounds (
    id          uuid PRIMARY KEY,
    set_id      uuid NOT NULL REFERENCES problem_sets (id) ON DELETE RESTRICT,
    title       text NOT NULL DEFAULT '',
    started_at  timestamptz NOT NULL DEFAULT now(),
    ended_at    timestamptz,
    -- While set, students see the board as of this moment.
    frozen_at   timestamptz,
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX rounds_one_open ON rounds ((true)) WHERE ended_at IS NULL;
CREATE INDEX submissions_problem_submits ON submissions (problem_id, created_at) WHERE kind = 'submit';
