CREATE TABLE idempotency_keys (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    actor_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    route TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('in_progress', 'completed')),
    response_status SMALLINT,
    response_body JSONB,
    expires_at TIMESTAMPTZ NOT NULL,
    UNIQUE (owner_id, actor_id, key)
);

CREATE INDEX idempotency_keys_expires_idx ON idempotency_keys (expires_at);
