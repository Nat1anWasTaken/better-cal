-- MCP access for cloud agents via OAuth 2.1 (see docs/mcp.md).
--
-- An OAuth connection is an `api_tokens` row with kind = 'oauth' (a "grant"):
-- it carries the scopes the owner approved, can be revoked like any token,
-- and is what audit entries point at. Its bearer secrets live in oauth_tokens.

CREATE TABLE oauth_clients (
    -- Random id for dynamically registered clients; the metadata URL for CIMD clients.
    id             text PRIMARY KEY,
    source         text NOT NULL CHECK (source IN ('dcr', 'cimd')),
    name           text NOT NULL,
    redirect_uris  text[] NOT NULL,
    secret_hash    bytea,
    metadata       jsonb NOT NULL DEFAULT '{}',
    created_at     timestamptz NOT NULL DEFAULT now(),
    -- CIMD documents are re-fetched once this is a day old.
    refreshed_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at   timestamptz
);

ALTER TABLE api_tokens
    ADD COLUMN kind text NOT NULL DEFAULT 'api' CHECK (kind IN ('api', 'oauth')),
    ADD COLUMN client_id text REFERENCES oauth_clients (id) ON DELETE CASCADE,
    ALTER COLUMN token_hash DROP NOT NULL,
    ADD CONSTRAINT api_tokens_kind_shape CHECK ((kind = 'api') = (client_id IS NULL) AND (kind = 'oauth') = (token_hash IS NULL));

-- A pending authorization; once approved it also holds the (single-use) code.
CREATE TABLE oauth_auth_requests (
    id                uuid PRIMARY KEY,
    client_id         text NOT NULL REFERENCES oauth_clients (id) ON DELETE CASCADE,
    redirect_uri      text NOT NULL,
    state             text,
    code_challenge    text NOT NULL,
    scopes_requested  text[] NOT NULL,
    resource          text NOT NULL,
    user_id           uuid REFERENCES users (id) ON DELETE CASCADE,
    grant_id          uuid REFERENCES api_tokens (id) ON DELETE CASCADE,
    code_hash         bytea UNIQUE,
    created_at        timestamptz NOT NULL DEFAULT now(),
    expires_at        timestamptz NOT NULL,
    used_at           timestamptz
);

CREATE TABLE oauth_tokens (
    token_hash  bytea PRIMARY KEY,
    grant_id    uuid NOT NULL REFERENCES api_tokens (id) ON DELETE CASCADE,
    kind        text NOT NULL CHECK (kind IN ('access', 'refresh')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    -- Set when a refresh token is exchanged; presenting it again revokes the grant.
    rotated_at  timestamptz
);
CREATE INDEX oauth_tokens_grant ON oauth_tokens (grant_id);
