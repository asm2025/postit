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
