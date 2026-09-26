CREATE TABLE audit_events (
    id UUID PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_user_id UUID,
    owner_id UUID,
    subject_user_id UUID,
    kind TEXT NOT NULL,
    ip TEXT,
    request_id UUID,
    details JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX audit_events_at_idx ON audit_events (at);
CREATE INDEX audit_events_kind_idx ON audit_events (kind);
CREATE INDEX audit_events_actor_idx ON audit_events (actor_user_id);
CREATE INDEX audit_events_subject_idx ON audit_events (subject_user_id);
