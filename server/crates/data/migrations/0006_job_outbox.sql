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
