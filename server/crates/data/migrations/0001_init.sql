-- from 0001_citext.sql
CREATE EXTENSION IF NOT EXISTS citext;

-- from 0002_users.sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    oidc_issuer TEXT NOT NULL,
    oidc_subject TEXT NOT NULL,
    email CITEXT,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'disabled', 'deleting')),
    approved_at TIMESTAMPTZ,
    approved_by UUID REFERENCES users (id) ON DELETE SET NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (oidc_issuer, oidc_subject)
);

CREATE INDEX users_status_idx ON users (status);
CREATE INDEX users_role_status_idx ON users (role, status);

-- from 0003_audit_events.sql
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

-- from 0004_user_preferences.sql
CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_notifications BOOLEAN NOT NULL DEFAULT TRUE,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    locale TEXT NOT NULL DEFAULT 'en-US',
    store_ai_prompts BOOLEAN NOT NULL DEFAULT FALSE,
    last_approval_email_at TIMESTAMPTZ
);

-- from 0005_idempotency_keys.sql
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

-- from 0006_job_outbox.sql
CREATE TABLE job_outbox (
    id UUID PRIMARY KEY,
    job_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    run_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX job_outbox_created_at_idx ON job_outbox (created_at);

-- NOTIFY is delivered at commit, so the relay only ever wakes for committed rows.
CREATE FUNCTION job_outbox_notify() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('postit_job_outbox', '');
    RETURN NULL;
END;
$$;

CREATE TRIGGER job_outbox_notify_trigger
    AFTER INSERT ON job_outbox
    FOR EACH STATEMENT EXECUTE FUNCTION job_outbox_notify();

-- from 0007_job_recurring_runs.sql
CREATE TABLE job_recurring_runs (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    scheduled_for TIMESTAMPTZ NOT NULL,
    manual BOOLEAN NOT NULL,
    job_id UUID,
    outcome TEXT CHECK (outcome IN ('succeeded', 'failed')),
    finished_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One scheduled run per (name, tick) across every worker process.
CREATE UNIQUE INDEX job_recurring_runs_tick_idx
    ON job_recurring_runs (name, scheduled_for) WHERE NOT manual;
CREATE INDEX job_recurring_runs_created_at_idx ON job_recurring_runs (created_at);


-- from the P8 job ledger: one row per job, one row per try. `jobs.id` equals the
-- `job_outbox` id and the job storage's task id.
CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    job_type TEXT NOT NULL,
    queue TEXT NOT NULL,
    payload JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'dead', 'killed', 'cancelled')),
    run_at TIMESTAMPTZ NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER,
    recurring_name TEXT,
    retried_from UUID REFERENCES jobs (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ
);
CREATE INDEX jobs_state_run_at_idx ON jobs (state, run_at);
CREATE INDEX jobs_type_created_at_idx ON jobs (job_type, created_at);
CREATE INDEX jobs_finished_at_idx ON jobs (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX jobs_recurring_idx ON jobs (recurring_name, created_at) WHERE recurring_name IS NOT NULL;
CREATE INDEX jobs_retried_from_idx ON jobs (retried_from) WHERE retried_from IS NOT NULL;

CREATE TABLE job_attempts (
    job_id UUID NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    attempt INTEGER NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    worker TEXT NOT NULL,
    outcome TEXT
        CHECK (outcome IN ('succeeded', 'retrying', 'failed', 'killed', 'interrupted', 'cancelled')),
    error_kind TEXT
        CHECK (error_kind IN ('retry', 'retries_exhausted', 'fatal', 'unknown_type', 'panic', 'interrupted')),
    error_code TEXT,
    -- Stored only for job types that opted in to messages; null otherwise.
    error_message TEXT,
    PRIMARY KEY (job_id, attempt)
);
CREATE INDEX job_attempts_finished_at_idx ON job_attempts (finished_at) WHERE finished_at IS NOT NULL;
