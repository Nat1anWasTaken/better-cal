-- CYCU-CAL initial schema.

CREATE TABLE users (
    id                    uuid PRIMARY KEY,
    username              text NOT NULL,
    display_name          text NOT NULL DEFAULT '',
    role                  text NOT NULL CHECK (role IN ('student', 'owner')),
    password_hash         text NOT NULL,
    disabled              boolean NOT NULL DEFAULT false,
    must_change_password  boolean NOT NULL DEFAULT false,
    cohort_id             uuid,
    created_at            timestamptz NOT NULL DEFAULT now(),
    updated_at            timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_username_lower ON users (lower(username));

-- Opaque session tokens; only a SHA-256 of the token is stored.
CREATE TABLE sessions (
    token_hash    bytea PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token    text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_seen_at  timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE api_tokens (
    id            uuid PRIMARY KEY,
    owner_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name          text NOT NULL,
    token_hash    bytea NOT NULL UNIQUE,
    scopes        text[] NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz,
    revoked_at    timestamptz,
    last_used_at  timestamptz
);

CREATE TABLE problems (
    id                     uuid PRIMARY KEY,
    slug                   text NOT NULL UNIQUE,
    status                 text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    head_revision_id       uuid,
    published_revision_id  uuid,
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now()
);

-- Immutable snapshots: editing a problem always creates a new revision, and
-- submissions point at the revision they were judged against.
CREATE TABLE problem_revisions (
    id            uuid PRIMARY KEY,
    problem_id    uuid NOT NULL REFERENCES problems (id) ON DELETE CASCADE,
    number        integer NOT NULL,
    content       jsonb NOT NULL,
    title         text NOT NULL,
    content_hash  text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    created_by    uuid REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (problem_id, number)
);

ALTER TABLE problems
    ADD CONSTRAINT problems_head_fk FOREIGN KEY (head_revision_id) REFERENCES problem_revisions (id) DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT problems_published_fk FOREIGN KEY (published_revision_id) REFERENCES problem_revisions (id) DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE problem_sets (
    id                uuid PRIMARY KEY,
    slug              text NOT NULL UNIQUE,
    title             text NOT NULL,
    description_md    text NOT NULL DEFAULT '',
    learned_concepts  text[] NOT NULL DEFAULT '{}',
    listed            boolean NOT NULL DEFAULT true,
    status            text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    published_at      timestamptz
);

-- Sets reference problems, never copy them: removing an item leaves the
-- problem (and every other set using it) untouched.
CREATE TABLE problem_set_items (
    set_id      uuid NOT NULL REFERENCES problem_sets (id) ON DELETE CASCADE,
    problem_id  uuid NOT NULL REFERENCES problems (id) ON DELETE RESTRICT,
    position    integer NOT NULL,
    PRIMARY KEY (set_id, problem_id)
);
CREATE INDEX problem_set_items_problem ON problem_set_items (problem_id);

CREATE TABLE cohorts (
    id           uuid PRIMARY KEY,
    name         text NOT NULL UNIQUE,
    description  text NOT NULL DEFAULT '',
    policy       jsonb NOT NULL DEFAULT '{}',
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE users ADD CONSTRAINT users_cohort_fk FOREIGN KEY (cohort_id) REFERENCES cohorts (id) ON DELETE SET NULL;

CREATE TABLE student_overrides (
    user_id     uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    policy      jsonb NOT NULL DEFAULT '{}',
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- Single-row site configuration; `version` bumps on every change so API
-- processes can reload caches without a restart.
CREATE TABLE site_config (
    id             integer PRIMARY KEY CHECK (id = 1),
    active_set_id  uuid REFERENCES problem_sets (id) ON DELETE SET NULL,
    settings       jsonb NOT NULL DEFAULT '{}',
    version        bigint NOT NULL DEFAULT 1,
    updated_at     timestamptz NOT NULL DEFAULT now()
);
INSERT INTO site_config (id) VALUES (1);

CREATE TABLE config_revisions (
    id             bigserial PRIMARY KEY,
    active_set_id  uuid,
    settings       jsonb NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    created_by     uuid REFERENCES users (id) ON DELETE SET NULL
);

-- Editable teaching text. key: 'rule:<id>', 'custom:<id>' or 'texts'.
CREATE TABLE catalog_entries (
    key           text PRIMARY KEY,
    draft         jsonb,
    published     jsonb,
    draft_delete  boolean NOT NULL DEFAULT false,
    updated_at    timestamptz NOT NULL DEFAULT now(),
    updated_by    uuid REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE catalog_history (
    id          bigserial PRIMARY KEY,
    key         text NOT NULL,
    published   jsonb,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE drafts (
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    problem_id  uuid NOT NULL REFERENCES problems (id) ON DELETE CASCADE,
    code        text NOT NULL DEFAULT '',
    -- fill-in problems store only the student's blanks
    blanks      jsonb,
    version     integer NOT NULL DEFAULT 1,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, problem_id)
);

CREATE TABLE submissions (
    id                uuid PRIMARY KEY,
    user_id           uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    problem_id        uuid NOT NULL REFERENCES problems (id) ON DELETE CASCADE,
    revision_id       uuid NOT NULL REFERENCES problem_revisions (id),
    set_id            uuid REFERENCES problem_sets (id) ON DELETE SET NULL,
    kind              text NOT NULL CHECK (kind IN ('run', 'submit', 'test_run')),
    code              text NOT NULL,
    blanks            jsonb,
    assembled_source  text NOT NULL,
    custom_stdin      text,
    status            text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'compiling', 'running', 'done', 'system_error')),
    verdict           text,
    passed            integer,
    total             integer,
    result            jsonb,
    finding_rules     text[] NOT NULL DEFAULT '{}',
    attempts          integer NOT NULL DEFAULT 0,
    lease_until       timestamptz,
    next_attempt_at   timestamptz NOT NULL DEFAULT now(),
    last_error        text,
    idempotency_key   text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    started_at        timestamptz,
    finished_at       timestamptz
);
CREATE INDEX submissions_queue ON submissions (next_attempt_at) WHERE status IN ('queued', 'compiling', 'running');
CREATE INDEX submissions_user ON submissions (user_id, created_at DESC);
CREATE INDEX submissions_problem ON submissions (problem_id, created_at DESC);
CREATE UNIQUE INDEX submissions_idempotency ON submissions (user_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE hint_reveals (
    submission_id  uuid NOT NULL REFERENCES submissions (id) ON DELETE CASCADE,
    finding_id     integer NOT NULL,
    level          integer NOT NULL,
    rule           text NOT NULL,
    revealed_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (submission_id, finding_id, level)
);

CREATE TABLE progress_credits (
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    problem_id  uuid NOT NULL REFERENCES problems (id) ON DELETE CASCADE,
    note        text NOT NULL DEFAULT '',
    created_by  uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, problem_id)
);

-- Replay store for Idempotency-Key on non-submission writes.
CREATE TABLE idempotency_keys (
    principal     text NOT NULL,
    key           text NOT NULL,
    request_hash  text NOT NULL,
    status_code   integer NOT NULL,
    response      jsonb NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (principal, key)
);

CREATE TABLE audit_log (
    id              bigserial PRIMARY KEY,
    at              timestamptz NOT NULL DEFAULT now(),
    actor_user_id   uuid REFERENCES users (id) ON DELETE SET NULL,
    actor_token_id  uuid REFERENCES api_tokens (id) ON DELETE SET NULL,
    action          text NOT NULL,
    target          text NOT NULL DEFAULT '',
    detail          jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_log_at ON audit_log (at DESC);
