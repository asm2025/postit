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
-- Delegate lookup by verified email (`UsersRepo::find_active_verified_by_email`).
CREATE INDEX users_email_idx ON users (email) WHERE email_verified AND status = 'active';

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
    last_approval_email_at TIMESTAMPTZ,
    -- Per-kind overrides: { "<kind>": { "push": bool, "email": bool } }.
    notification_switches JSONB NOT NULL DEFAULT '{}'::jsonb,
    comment_digest BOOLEAN NOT NULL DEFAULT FALSE,
    last_comment_digest_at TIMESTAMPTZ
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

CREATE TABLE platforms (
    id TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_]{0,31}$'),
    display_name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row: the newest vault key version this deployment has ever run with.
CREATE TABLE vault_state (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    newest_key_version INTEGER NOT NULL CHECK (newest_key_version > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------------------------------------
-- Plan 03 owned content.
--
-- Every owned table has `id`, `owner_id`, `created_at`, `updated_at`, and the actor columns
-- `created_by` and `updated_by`, plus UNIQUE (owner_id, id). Every reference between owned rows
-- is a composite FK on (owner_id, x_id), so a row can never point at another owner's row.
-- Nullable references use ON DELETE SET NULL (x_id) so the owner column is never nulled.
-- References that history needs to keep (deliveries to accounts, post_media to media) use the
-- default NO ACTION: nothing deletes those parents except the cascade from `users`, which
-- removes the referencing rows in the same statement.
-- Link tables (the ones with a composite primary key) carry `owner_id` and `created_at` only.
-- Sealed columns are BYTEA with the comment 'postit:sealed'; SEALED_COLUMNS must list them all.
-- ---------------------------------------------------------------------------------------------

-- Connections.
CREATE TABLE account_grants (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    kind TEXT NOT NULL CHECK (kind IN ('oauth', 'credentials')),
    remote_user_id TEXT,
    -- NULL once the grant is revoked: a revoked grant keeps no usable credential.
    credential BYTEA,
    connection_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    expires_at TIMESTAMPTZ,
    reconnect_by TIMESTAMPTZ,
    reconnect_warned TEXT NOT NULL DEFAULT 'none'
        CHECK (reconnect_warned IN ('none', '7d', '1d', 'expired')),
    scopes TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'needs_reconnect', 'revoked')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);
CREATE INDEX account_grants_owner_idx ON account_grants (owner_id);
CREATE INDEX account_grants_reconnect_idx ON account_grants (reconnect_by)
    WHERE reconnect_by IS NOT NULL;
-- The revoke-skip rule and platform deauthorization look grants up by remote user.
CREATE INDEX account_grants_remote_user_idx ON account_grants (platform_id, remote_user_id)
    WHERE remote_user_id IS NOT NULL;
-- The refresh cron selects active OAuth grants about to expire.
CREATE INDEX account_grants_refresh_idx ON account_grants (expires_at)
    WHERE kind = 'oauth' AND status = 'active';
COMMENT ON COLUMN account_grants.credential IS 'postit:sealed';

CREATE TABLE connected_accounts (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    grant_id UUID,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    remote_account_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'needs_reconnect', 'revoked', 'disconnected')),
    test_account BOOLEAN NOT NULL DEFAULT FALSE,
    last_checked_at TIMESTAMPTZ,
    inbox_cursor TEXT,
    inbox_next_poll_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, grant_id) REFERENCES account_grants (owner_id, id)
        ON DELETE SET NULL (grant_id)
);
-- A remote account belongs to one postit owner across the deployment; a disconnected row frees it.
CREATE UNIQUE INDEX connected_accounts_remote_key_idx
    ON connected_accounts (platform_id, remote_account_id) WHERE status <> 'disconnected';
-- One sending method per owner.
CREATE UNIQUE INDEX connected_accounts_one_email_idx
    ON connected_accounts (owner_id) WHERE platform_id = 'email' AND status <> 'disconnected';
CREATE INDEX connected_accounts_owner_idx ON connected_accounts (owner_id, status);
CREATE INDEX connected_accounts_grant_idx ON connected_accounts (grant_id) WHERE grant_id IS NOT NULL;
CREATE INDEX connected_accounts_inbox_poll_idx ON connected_accounts (inbox_next_poll_at)
    WHERE inbox_next_poll_at IS NOT NULL;

CREATE TABLE account_tokens (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL,
    access_token BYTEA NOT NULL,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT account_tokens_account_key UNIQUE (account_id),
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
COMMENT ON COLUMN account_tokens.access_token IS 'postit:sealed';

-- Not owned content: `user_id` is the user who started the flow, and rows die with them.
CREATE TABLE oauth_states (
    id UUID PRIMARY KEY,
    state_hash BYTEA NOT NULL UNIQUE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    pkce_verifier BYTEA,
    return_to TEXT,
    connection_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- Set when the callback consumes the state, so a state is used once.
    consumed_at TIMESTAMPTZ,
    -- Set after the callback, while the pending result waits for the client to confirm it.
    -- A discarded result keeps `grant_tokens` and `remote_user_id` with `connect_id_hash`
    -- NULL, so the purge job revokes it at the provider before deleting it.
    connect_id_hash BYTEA UNIQUE,
    grant_tokens BYTEA,
    discovered JSONB,
    -- The provider's ID for the authorizing user, in the clear as on `account_grants`, for
    -- the revoke-skip rule.
    remote_user_id TEXT
);
CREATE INDEX oauth_states_expires_idx ON oauth_states (expires_at);
CREATE INDEX oauth_states_remote_user_idx ON oauth_states (platform_id, remote_user_id)
    WHERE remote_user_id IS NOT NULL;
COMMENT ON COLUMN oauth_states.pkce_verifier IS 'postit:sealed';
COMMENT ON COLUMN oauth_states.grant_tokens IS 'postit:sealed';

CREATE TABLE pending_connections (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    connect_id_hash BYTEA NOT NULL UNIQUE,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    credential BYTEA NOT NULL,
    connection_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    discovered JSONB NOT NULL DEFAULT '[]'::jsonb,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);
CREATE INDEX pending_connections_expires_idx ON pending_connections (expires_at);
COMMENT ON COLUMN pending_connections.credential IS 'postit:sealed';

-- Delegation. A grant lets `delegate_id` act in `owner_id`'s workspace at `level`, on every
-- account or only those in `delegation_accounts`. Ended grants are kept as history; at most one
-- grant per pair is live. Deleting either user removes the grant.
CREATE TABLE delegations (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delegate_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    level TEXT NOT NULL CHECK (level IN ('view', 'edit', 'publish')),
    all_accounts BOOLEAN NOT NULL,
    expires_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    ended_reason TEXT CHECK (ended_reason IN ('revoked', 'left', 'expired')),
    revoked_by UUID REFERENCES users (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT delegations_not_self CHECK (owner_id <> delegate_id),
    CONSTRAINT delegations_ended_reason_set CHECK ((ended_at IS NULL) = (ended_reason IS NULL))
);
CREATE UNIQUE INDEX delegations_live_pair_idx
    ON delegations (owner_id, delegate_id) WHERE ended_at IS NULL;
CREATE INDEX delegations_owner_idx ON delegations (owner_id, created_at);
CREATE INDEX delegations_live_delegate_idx ON delegations (delegate_id) WHERE ended_at IS NULL;
CREATE INDEX delegations_expiry_idx ON delegations (expires_at)
    WHERE ended_at IS NULL AND expires_at IS NOT NULL;

-- The accounts a limited grant reaches. Repeating `owner_id` makes both references composite,
-- so a grant can never name another owner's account.
CREATE TABLE delegation_accounts (
    delegation_id UUID NOT NULL,
    account_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (delegation_id, account_id),
    FOREIGN KEY (owner_id, delegation_id) REFERENCES delegations (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX delegation_accounts_account_idx ON delegation_accounts (owner_id, account_id);

-- Writing guidance.
CREATE TABLE writing_styles (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL,
    tone TEXT NOT NULL DEFAULT '',
    emoji TEXT NOT NULL DEFAULT 'light' CHECK (emoji IN ('none', 'light', 'heavy')),
    hashtags TEXT NOT NULL DEFAULT 'few' CHECK (hashtags IN ('none', 'few', 'many')),
    language TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT writing_styles_account_key UNIQUE (account_id),
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);

CREATE TABLE brand_profiles (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    description TEXT NOT NULL DEFAULT '',
    audience TEXT NOT NULL DEFAULT '',
    offerings TEXT NOT NULL DEFAULT '',
    avoid_words TEXT[] NOT NULL DEFAULT '{}',
    default_language TEXT,
    examples TEXT[] NOT NULL DEFAULT '{}' CHECK (cardinality(examples) <= 5),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT brand_profiles_owner_key UNIQUE (owner_id)
);

-- Organizing.
CREATE TABLE target_groups (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);

CREATE TABLE target_group_accounts (
    group_id UUID NOT NULL,
    account_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, account_id),
    FOREIGN KEY (owner_id, group_id) REFERENCES target_groups (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX target_group_accounts_account_idx ON target_group_accounts (account_id);

CREATE TABLE campaigns (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    starts_on DATE,
    ends_on DATE,
    color TEXT,
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CHECK (starts_on IS NULL OR ends_on IS NULL OR ends_on >= starts_on)
);

-- Media.
CREATE TABLE media_assets (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    storage_key TEXT NOT NULL,
    thumbnail_key TEXT,
    original_filename TEXT,
    mime TEXT NOT NULL,
    bytes BIGINT NOT NULL DEFAULT 0 CHECK (bytes >= 0),
    sha256 BYTEA,
    width INTEGER,
    height INTEGER,
    duration_ms BIGINT,
    status TEXT NOT NULL CHECK (status IN ('uploading', 'processing', 'ready', 'failed')),
    derived_from UUID,
    fit_spec JSONB,
    declared_bytes BIGINT CHECK (declared_bytes >= 0),
    upload_offset BIGINT CHECK (upload_offset >= 0),
    upload_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, derived_from) REFERENCES media_assets (owner_id, id)
        ON DELETE SET NULL (derived_from)
);
CREATE INDEX media_assets_owner_idx ON media_assets (owner_id, created_at DESC);
CREATE INDEX media_assets_derived_idx ON media_assets (derived_from) WHERE derived_from IS NOT NULL;
CREATE INDEX media_assets_upload_expires_idx ON media_assets (upload_expires_at)
    WHERE status = 'uploading';

-- Posts.
CREATE TABLE posts (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind TEXT NOT NULL DEFAULT 'post' CHECK (kind IN ('post', 'comment')),
    reply_to JSONB,
    campaign_id UUID,
    text TEXT NOT NULL DEFAULT '',
    link TEXT,
    tags TEXT[] NOT NULL DEFAULT '{}',
    disclose_ai_generated BOOLEAN NOT NULL DEFAULT FALSE,
    version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'scheduled', 'publishing', 'published', 'partial', 'failed', 'cancelled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, campaign_id) REFERENCES campaigns (owner_id, id)
        ON DELETE SET NULL (campaign_id)
);
CREATE INDEX posts_owner_status_idx ON posts (owner_id, status, created_at DESC);
CREATE INDEX posts_campaign_idx ON posts (campaign_id) WHERE campaign_id IS NOT NULL;

CREATE TABLE post_media (
    post_id UUID NOT NULL,
    media_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    alt_text TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, media_id),
    UNIQUE (post_id, position),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, media_id) REFERENCES media_assets (owner_id, id)
);
CREATE INDEX post_media_media_idx ON post_media (media_id);

CREATE TABLE post_targets (
    post_id UUID NOT NULL,
    account_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, account_id),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX post_targets_account_idx ON post_targets (account_id);

CREATE TABLE post_overrides (
    post_id UUID NOT NULL,
    account_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    patch JSONB NOT NULL DEFAULT '{}'::jsonb,
    options JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    PRIMARY KEY (post_id, account_id),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX post_overrides_account_idx ON post_overrides (account_id);

CREATE TABLE recurrences (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID NOT NULL,
    rule TEXT NOT NULL,
    timezone TEXT NOT NULL,
    rewrite BOOLEAN NOT NULL DEFAULT FALSE,
    next_occurrence_at TIMESTAMPTZ,
    occurrences_created INTEGER NOT NULL DEFAULT 0,
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    paused_reason TEXT CHECK (paused_reason IN ('post_edited', 'no_targets', 'user', 'ended')),
    confirmed_version INTEGER,
    confirmed_by UUID REFERENCES users (id) ON DELETE SET NULL,
    confirmed_at TIMESTAMPTZ,
    skipped JSONB NOT NULL DEFAULT '[]'::jsonb,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT recurrences_post_key UNIQUE (post_id),
    CHECK (paused = (paused_reason IS NOT NULL)),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE
);
CREATE INDEX recurrences_due_idx ON recurrences (next_occurrence_at) WHERE NOT paused;

CREATE TABLE recurrence_targets (
    recurrence_id UUID NOT NULL,
    account_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (recurrence_id, account_id),
    FOREIGN KEY (owner_id, recurrence_id) REFERENCES recurrences (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX recurrence_targets_account_idx ON recurrence_targets (account_id);

-- Publishing.
CREATE TABLE publish_jobs (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    post_id UUID NOT NULL,
    post_version INTEGER NOT NULL,
    recurrence_id UUID,
    content_snapshot JSONB NOT NULL,
    scheduled_at TIMESTAMPTZ NOT NULL,
    native_schedule BOOLEAN NOT NULL DEFAULT FALSE,
    dry_run BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'scheduled'
        CHECK (status IN ('scheduled', 'publishing', 'published', 'cancelled', 'failed', 'partial')),
    version INTEGER NOT NULL DEFAULT 1,
    cancelled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, recurrence_id) REFERENCES recurrences (owner_id, id)
        ON DELETE SET NULL (recurrence_id)
);
-- At most one live, non-terminal job per post. Dry runs are exempt.
CREATE UNIQUE INDEX publish_jobs_one_live_job_idx
    ON publish_jobs (post_id) WHERE NOT dry_run AND status IN ('scheduled', 'publishing');
CREATE INDEX publish_jobs_post_idx ON publish_jobs (post_id, status);
CREATE INDEX publish_jobs_recurrence_idx ON publish_jobs (recurrence_id) WHERE recurrence_id IS NOT NULL;

CREATE TABLE deliveries (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    job_id UUID NOT NULL,
    post_id UUID NOT NULL,
    account_id UUID NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('publish', 'comment', 'direct')),
    state TEXT NOT NULL DEFAULT 'pending'
        CHECK (state IN (
            'pending', 'claimed', 'in_flight', 'processing', 'outcome_unknown', 'parked',
            'published', 'failed', 'dead_letter', 'cancelled', 'simulated'
        )),
    parked_reason TEXT CHECK (parked_reason IN (
        'owner_disabled', 'account_needs_reconnect', 'platform_unavailable', 'platform_disabled',
        'live_publish_disabled', 'quota_exceeded', 'outcome_unresolvable'
    )),
    -- sha256(job_id || account_id), computed in Rust.
    idempotency_key BYTEA NOT NULL,
    dispatch_gen INTEGER NOT NULL DEFAULT 0,
    lease_expires_at TIMESTAMPTZ,
    cancel_requested_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ,
    remote_id TEXT,
    remote_url TEXT,
    container_ref TEXT,
    remote_publish_at TIMESTAMPTZ,
    direct_counts JSONB,
    last_error JSONB,
    parked_notified_at TIMESTAMPTZ,
    -- When a `quota_exceeded` park may resume: the bucket's next period. `next_attempt_at`
    -- keeps the due time, which `park_grace` counts from. Set on a move into `parked` only.
    parked_until TIMESTAMPTZ,
    -- The poll or reconcile chain's position: a job carrying `k` proceeds only while this is
    -- `k`, and moves it to `k + 1`, so a duplicate job cannot fork the chain. Every state
    -- change resets it to 0.
    check_seq INTEGER NOT NULL DEFAULT 0,
    -- When the chain's next poll or reconcile job is due, while `processing` or
    -- `outcome_unknown`; the due sweep restarts a chain whose job never came. NULL otherwise.
    check_due_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT deliveries_idempotency_key_key UNIQUE (idempotency_key),
    CHECK ((state = 'parked') = (parked_reason IS NOT NULL)),
    FOREIGN KEY (owner_id, job_id) REFERENCES publish_jobs (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
);
CREATE INDEX deliveries_job_idx ON deliveries (job_id);
CREATE INDEX deliveries_post_idx ON deliveries (post_id);
CREATE INDEX deliveries_account_idx ON deliveries (account_id);
CREATE INDEX deliveries_due_idx ON deliveries (next_attempt_at) WHERE state = 'pending';
CREATE INDEX deliveries_lease_idx ON deliveries (lease_expires_at)
    WHERE state IN ('claimed', 'in_flight');
CREATE INDEX deliveries_parked_idx ON deliveries (parked_reason) WHERE state = 'parked';

CREATE TABLE delivery_attempts (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delivery_id UUID NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    outcome TEXT,
    error JSONB,
    request_summary JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, delivery_id) REFERENCES deliveries (owner_id, id) ON DELETE CASCADE
);
CREATE INDEX delivery_attempts_delivery_idx ON delivery_attempts (delivery_id, started_at);
CREATE INDEX delivery_attempts_started_idx ON delivery_attempts (started_at);

-- Contacts and email campaigns.
CREATE TABLE contacts (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    -- HMAC-SHA256 of the normalized address; the same value `suppressions` stores.
    email_hash BYTEA NOT NULL,
    first_name TEXT,
    last_name TEXT,
    fields JSONB NOT NULL DEFAULT '{}'::jsonb,
    tags TEXT[] NOT NULL DEFAULT '{}',
    locale TEXT,
    consent_source TEXT,
    consent_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'subscribed'
        CHECK (status IN ('subscribed', 'unsubscribed', 'bounced', 'complained')),
    status_changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT contacts_owner_email_key UNIQUE (owner_id, email)
);
CREATE INDEX contacts_hash_idx ON contacts (owner_id, email_hash);

CREATE TABLE contact_lists (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);

CREATE TABLE contact_list_members (
    list_id UUID NOT NULL,
    contact_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (list_id, contact_id),
    FOREIGN KEY (owner_id, list_id) REFERENCES contact_lists (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, contact_id) REFERENCES contacts (owner_id, id) ON DELETE CASCADE
);
CREATE INDEX contact_list_members_contact_idx ON contact_list_members (contact_id);

-- Kept when the contact is deleted, so an unsubscribe survives a re-import. Holds no address.
CREATE TABLE suppressions (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    email_hash BYTEA NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN ('unsubscribed', 'bounced', 'complained', 'manual')),
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT suppressions_owner_hash_key UNIQUE (owner_id, email_hash)
);

CREATE TABLE contact_imports (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'processing'
        CHECK (status IN ('processing', 'completed', 'failed')),
    rows_total INTEGER NOT NULL DEFAULT 0,
    rows_ok INTEGER NOT NULL DEFAULT 0,
    rows_rejected INTEGER NOT NULL DEFAULT 0,
    -- Row numbers and reason codes only, never addresses.
    report JSONB NOT NULL DEFAULT '[]'::jsonb,
    attested_by UUID REFERENCES users (id) ON DELETE SET NULL,
    attested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);
CREATE INDEX contact_imports_created_idx ON contact_imports (created_at);

CREATE TABLE direct_sends (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delivery_id UUID NOT NULL,
    contact_id UUID,
    email_hash BYTEA NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'sent', 'failed', 'suppressed', 'bounced')),
    batch UUID,
    lease_expires_at TIMESTAMPTZ,
    sent_at TIMESTAMPTZ,
    provider_message_id TEXT,
    error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT direct_sends_delivery_contact_key UNIQUE (delivery_id, contact_id),
    FOREIGN KEY (owner_id, delivery_id) REFERENCES deliveries (owner_id, id) ON DELETE CASCADE,
    FOREIGN KEY (owner_id, contact_id) REFERENCES contacts (owner_id, id)
        ON DELETE SET NULL (contact_id)
);
CREATE INDEX direct_sends_queued_idx ON direct_sends (delivery_id, id) WHERE state = 'queued';
CREATE INDEX direct_sends_lease_idx ON direct_sends (lease_expires_at)
    WHERE state = 'queued' AND lease_expires_at IS NOT NULL;
CREATE INDEX direct_sends_created_idx ON direct_sends (created_at);

CREATE TABLE email_events (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    direct_send_id UUID NOT NULL,
    kind TEXT NOT NULL
        CHECK (kind IN ('delivered', 'bounced', 'complained', 'clicked', 'opened', 'unsubscribed')),
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    link_index INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, direct_send_id) REFERENCES direct_sends (owner_id, id)
        ON DELETE CASCADE
);
CREATE INDEX email_events_send_idx ON email_events (direct_send_id);
CREATE INDEX email_events_at_idx ON email_events (at);

-- Inbox and analytics.
CREATE TABLE inbox_items (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL,
    remote_comment_id TEXT NOT NULL,
    remote_post_id TEXT,
    remote_parent_id TEXT,
    delivery_id UUID,
    author_handle TEXT,
    author_name TEXT,
    text TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new', 'read', 'replied', 'dismissed')),
    reply_post_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT inbox_items_account_comment_key UNIQUE (account_id, remote_comment_id),
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE,
    FOREIGN KEY (owner_id, delivery_id) REFERENCES deliveries (owner_id, id)
        ON DELETE SET NULL (delivery_id),
    FOREIGN KEY (owner_id, reply_post_id) REFERENCES posts (owner_id, id)
        ON DELETE SET NULL (reply_post_id)
);
CREATE INDEX inbox_items_account_idx ON inbox_items (account_id, received_at DESC);
CREATE INDEX inbox_items_received_idx ON inbox_items (received_at);

CREATE TABLE post_metric_snapshots (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    delivery_id UUID NOT NULL,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    metric_offset TEXT NOT NULL CHECK (metric_offset IN ('1h', '24h', '7d', '30d')),
    metrics JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT post_metric_snapshots_delivery_offset_key UNIQUE (delivery_id, metric_offset),
    FOREIGN KEY (owner_id, delivery_id) REFERENCES deliveries (owner_id, id) ON DELETE CASCADE
);

CREATE TABLE account_metrics_daily (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL,
    day DATE NOT NULL,
    followers BIGINT,
    reach BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT account_metrics_daily_account_day_key UNIQUE (account_id, day),
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);

-- Drafting.
CREATE TABLE ai_providers (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider_key TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}'::jsonb,
    api_key BYTEA,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);
CREATE UNIQUE INDEX ai_providers_one_default_idx ON ai_providers (owner_id) WHERE is_default;
COMMENT ON COLUMN ai_providers.api_key IS 'postit:sealed';

CREATE TABLE ai_generations (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider_id UUID,
    model TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('post', 'reply', 'rewrite')),
    prompt_hash BYTEA NOT NULL,
    -- Only when the owner has enabled storing prompts; cleared by retention.
    prompt TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    post_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    FOREIGN KEY (owner_id, provider_id) REFERENCES ai_providers (owner_id, id)
        ON DELETE SET NULL (provider_id),
    FOREIGN KEY (owner_id, post_id) REFERENCES posts (owner_id, id) ON DELETE SET NULL (post_id)
);
CREATE INDEX ai_generations_created_idx ON ai_generations (created_at);

-- Quotas.
CREATE TABLE platform_quota_usage (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    bucket TEXT NOT NULL,
    period_start DATE NOT NULL,
    units_used BIGINT NOT NULL DEFAULT 0 CHECK (units_used >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id),
    CONSTRAINT platform_quota_usage_key UNIQUE (account_id, platform_id, bucket, period_start),
    FOREIGN KEY (owner_id, account_id) REFERENCES connected_accounts (owner_id, id)
        ON DELETE CASCADE
);

-- Delivery to people. `user_id` is the recipient, so these rows die with the user.
CREATE TABLE push_endpoints (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    endpoint BYTEA NOT NULL,
    p256dh BYTEA NOT NULL,
    auth BYTEA NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('android', 'ios', 'web')),
    endpoint_hash BYTEA NOT NULL,
    device_label TEXT,
    last_success_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, endpoint_hash)
);
CREATE INDEX push_endpoints_user_idx ON push_endpoints (user_id);
COMMENT ON COLUMN push_endpoints.endpoint IS 'postit:sealed';
COMMENT ON COLUMN push_endpoints.p256dh IS 'postit:sealed';
COMMENT ON COLUMN push_endpoints.auth IS 'postit:sealed';

CREATE TABLE notifications (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The workspace the notification is about; null for deployment notifications.
    owner_id UUID REFERENCES users (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'delivery_dead_lettered', 'delivery_failed', 'delivery_outcome_unknown', 'delivery_parked',
        'delivery_stale', 'job_partial', 'account_needs_reconnect', 'account_revoked_by_platform',
        'reconnect_by_passed', 'recurrence_skipped', 'delivery_published', 'reconnect_reminder',
        'delegation_changed', 'new_comments', 'user_pending'
    )),
    severity TEXT NOT NULL DEFAULT 'info' CHECK (severity IN ('info', 'critical')),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    read_at TIMESTAMPTZ,
    emailed_at TIMESTAMPTZ,
    pushed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX notifications_user_idx ON notifications (user_id, created_at DESC);
CREATE INDEX notifications_read_idx ON notifications (read_at) WHERE read_at IS NOT NULL;
-- 'emailed_at' means settled for email: set when emailed (alert or digest), or at insert when none is wanted.
CREATE INDEX notifications_unemailed_idx ON notifications (user_id)
    WHERE emailed_at IS NULL AND severity = 'critical';
CREATE INDEX notifications_unread_idx ON notifications (user_id) WHERE read_at IS NULL;

-- Deployment settings and limits. Admin-edited; not owned content.
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE user_limits (
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_daily_cap INTEGER NOT NULL CHECK (email_daily_cap >= 0),
    contacts_cap INTEGER NOT NULL CHECK (contacts_cap >= 0),
    storage_quota_bytes BIGINT NOT NULL CHECK (storage_quota_bytes >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL
);

-- Deployment-wide platform quota, shared by every user. Holds no user reference.
CREATE TABLE platform_app_quota_usage (
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    bucket TEXT NOT NULL,
    period_start DATE NOT NULL,
    units_used BIGINT NOT NULL DEFAULT 0 CHECK (units_used >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (platform_id, bucket, period_start)
);
