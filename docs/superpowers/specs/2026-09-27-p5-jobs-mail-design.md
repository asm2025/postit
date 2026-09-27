# P5 — Jobs and mail

> Design spec for plan 02 phase P5 (`!ref/plans/02. foundation.md`). Builds `postit-jobs`
> and `postit-mail`, and finishes `postit-identity` (approval emails, `user_approved`,
> `delete_user`, the pending-cleanup and audit-retention cron jobs). Also provides the
> `data_retention` registration function that `postit-server` calls in P6.

## Context

P4 is merged on `stage`: `postit-data` (pool, migrations 0001–0005, `OwnerScope`,
`AuditLog`, `AuditRepo`, `UsersRepo`, `UserPreferencesRepo`, `IdempotencyRepo`, retention
purge queries) and the jobless/mailless `postit-identity` (discovery, verifier,
`ClaimsTransformer`, `PrincipalCache` with `LISTEN` eviction, `UserAdminService` with
approve/disable/enable/change_role, test issuer). `postit-jobs` and `postit-mail` are empty
stub crates. `postit-config` already has `MailSettings` and `JobsSettings` from P2, which this
phase extends.

Already in place that this phase builds on:

- `UserStatus::Deleting` exists in the enum and schema but nothing sets it yet.
- `user_preferences.last_approval_email_at` and
  `UserPreferencesRepo::set_last_approval_email_at` exist for coalescing.
- `retention::purge_audit_events(ip_retention, retention)` and
  `retention::purge_expired_idempotency_keys()` exist; this phase only schedules them.
- `AuditEvent::detail_user_id` forces other user references into top-level `details` keys
  ending in `_user_id`.
- `ClaimsTransformer::transform` provisions inside one transaction under the bootstrap
  advisory lock.

Deferred P4 rulings (memory `p4-deferred-rulings`) are all pinned to P6 or plan 03; none
block P5.

Crate levels (plan 01, Crate dependencies): `jobs` (config, data), `mail` (config, data,
jobs), `identity` (config, http, data, jobs, mail). `core` is level 1 and usable by all.

## Decisions

Confirmed with the maintainer before writing this spec:

- **apalis pins** (checked on crates.io 2026-09-27; no stable 1.0 yet):
  `apalis = "=1.0.0-rc.10"`, `apalis-cron = "=1.0.0-rc.9"`,
  `apalis-postgres = "=1.0.0-rc.9"`. The first implementation task confirms the three
  compile together and records the APIs used (storage push with a caller-supplied
  executor, LISTEN/NOTIFY fetch, retry layer, cron stream, schema/migration entry point).
  If the pins don't compile together, the task picks the newest set that does and records
  why.
- **Mail stack:** `lettre = "0.11"` (rustls, SMTP transport), `askama = "0.16"`.
- **HMAC:** RustCrypto `hmac = "0.12"` + `sha2 = "0.10"`. `emixcrypto` only provides plain
  SHA-256, not HMAC.
- **Approval emails list pending users by querying at run time.** The `send_email` payload
  for `user_pending_approval` carries only the mail kind and the admin's user ID. The
  context loader reads the current pending users. This is the plan's "IDs, not content" rule
  with the ID list derived at render time instead of carried; it bounds outbox rows under a
  sign-up flood and drops users approved or deleted in the meantime without extra state.
- **`audit.pseudonym_key` is required in every environment**, development included. The
  field becomes a required `RedactedSecret` (not `Option`); the dev vault already holds one.
- **Dev SMTP exit check** is an opt-in integration test (`POSTIT_SMTP_TESTS=1`) that sends
  both templates through `SmtpMailer` to the configured local SMTP tool. CI skips it.
- **`delete_user` never gives up.** It retries with long exponential backoff and no attempt
  cap. A stuck deletion stays visible (and retryable) in P8's console. No extra sweep job.
- **`JobConsole` and `JobSummary` are P8.** Plan 02 lists them in P8's task bullets; P5
  builds only what they will sit on (the registration API and `job_recurring_runs`).

## Section A — `postit-config` changes

Every change here also updates `default.toml`, the environment files where noted, and the
baseline fixture in `loader.rs` tests (`deny_unknown_fields` rejects anything missing or
extra).

- `audit.pseudonym_key: RedactedSecret` (was `Option`). Validation's "required outside
  development" rule is removed as redundant; deserialization now fails when it is missing,
  with the existing actionable error path.
- `mail.smtp.tls: SmtpTls` replacing `starttls: bool`, where
  `SmtpTls = "none" | "starttls" | "tls"`. `default.toml` sets `starttls`;
  `development.toml` sets `none` with `host = "localhost"`, `port = 25`. Validation: `none`
  is rejected outside development.
- `mail.send_email_max_attempts: u32` (default 8) — the give-up budget for `send_email`.
- `jobs.concurrency`: keeps its `HashMap<String, u32>` shape; `default.toml` sets
  `mail = 4`, `maintenance = 1`, `default = 4`. A queue missing from the map uses 1.
- `jobs.schedules: JobSchedules` with one cron expression per recurring job:
  `job_history_purge`, `purge_pending_users`, `audit_retention`, `data_retention`.
  Defaults are daily at staggered minutes (`0 10 3 * * *`, `0 20 3 * * *`,
  `0 30 3 * * *`, `0 40 3 * * *`; apalis-cron uses seconds-first expressions). Each is
  validated as a cron expression at load time.

## Section B — `postit-data` changes

### Migrations

- `0006_job_outbox.sql`: `job_outbox (id uuid PK, job_type text NOT NULL, payload jsonb
  NOT NULL, run_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now())`,
  index on `created_at`. An `AFTER INSERT` statement-level trigger runs
  `pg_notify('postit_job_outbox', '')`, which fires at commit, so the relay wakes only for
  committed rows.
- `0007_job_recurring_runs.sql`: `job_recurring_runs (id uuid PK, name text NOT NULL,
  scheduled_for timestamptz NOT NULL, manual bool NOT NULL, job_id text, outcome text,
  finished_at timestamptz, created_at timestamptz NOT NULL DEFAULT now())`, with a partial
  unique index on `(name, scheduled_for) WHERE NOT manual`.

Both are additive, per the expand-then-contract rule.

### Repositories

- **`JobOutboxRepo`**: `insert(conn, id, job_type, payload, run_at)`,
  `claim_batch(conn, limit) -> Vec<OutboxRow>` (`SELECT … ORDER BY created_at FOR UPDATE
  SKIP LOCKED LIMIT $1`), `delete(conn, id)`. Used only by `postit-jobs`.
- **`RecurringRunsRepo`**: `try_insert_scheduled(conn, id, name, scheduled_for) -> bool`
  (`ON CONFLICT DO NOTHING`), `insert_manual(conn, id, name, at)`,
  `set_job_id(conn, id, job_id)`, `finish(conn, id, outcome, at)`,
  `purge_older_than(conn, cutoff)`.
- **`UsersRepo`** gains `delete(conn, id)` (hard delete, sends
  `NOTIFY postit_user_changed` like the other status-changing writes),
  `list_active_admin_ids(conn)`, `list_pending_created_after(conn, after, limit)`, and
  `list_pending_older_than(conn, cutoff, limit)`. `set_status` accepts `Deleting`.
- **`AuditLog::pseudonymize_user(conn, user_id, key: &SecretString) -> u64`**: computes
  `HMAC-SHA256(key, user_id bytes)`, takes the first 16 bytes, sets the version nibble to 8
  and the RFC 4122 variant bits (`10xx`), and in one statement per column replaces the user
  ID in `actor_user_id`, `owner_id`, `subject_user_id`, and every top-level `details` key
  ending in `_user_id` whose value equals the user ID. It clears `ip` on every event where
  the user was the actor. Idempotent: a second run finds no raw user ID left. Lives in
  `postit-data` because `AuditLog` is the only writer of `audit_events`.
- **`AuditEventKind`** gains `UserDeleted`, `EmailSent`, `EmailDropped`, `EmailFailed`.
  (`job_retried`, `job_cancelled`, `job_deleted`, and `job_triggered` are P8's.)

## Section C — `postit-jobs`

apalis types never appear in this crate's public API: no re-exports, no apalis type in a
public signature, error, or trait. The review checklist for every P5 task includes this.

### Public API

```rust
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    const JOB_TYPE: &'static str;
    const QUEUE: Queue;
}

pub enum Queue { Mail, Maintenance, Default }   // `Publishing` added in plan 03

pub struct JobContext { pub attempt: u32, pub max_attempts: Option<u32> }
impl JobContext { pub fn is_last_attempt(&self) -> bool }

pub enum JobError { Retry(String), Fatal(String) }  // messages are redacted by callers

pub enum RetryPolicy {
    None,
    Backoff { max_attempts: Option<u32>, initial: Duration, max: Duration },
}

pub struct JobQueue { /* PgPool + IdGenerator */ }
impl JobQueue {
    pub async fn enqueue<J: Job>(&self, job: &J) -> Result<(), JobsError>;
    pub async fn enqueue_at<J: Job>(&self, job: &J, at: DateTime<Utc>) -> Result<(), JobsError>;
    pub async fn enqueue_in<J: Job>(&self, conn: &mut PgConnection, job: &J,
                                    run_at: Option<DateTime<Utc>>) -> Result<(), JobsError>;
}

pub struct JobRegistry { .. }
impl JobRegistry {
    pub fn register<J, H, Fut>(&mut self, retry: RetryPolicy, handler: H)
    where H: Fn(J, JobContext) -> Fut + Clone + Send + Sync + 'static,
          Fut: Future<Output = Result<(), JobError>> + Send;
    pub fn register_recurring<J: Job + Default>(&mut self, name: &'static str,
                                                schedule: &str, retry: RetryPolicy)
        -> Result<(), JobsError>;
}

pub async fn migrate(pool: &PgPool) -> Result<(), JobsError>;
pub struct Worker { .. }
impl Worker {
    pub fn new(pool: PgPool, settings: &JobsSettings, registry: JobRegistry) -> Self;
    pub async fn run(self, shutdown: impl Future<Output = ()> + Send) -> Result<(), JobsError>;
}
```

Exact names may shift during implementation; the shape (a `Job` trait with constant type
and queue, a registry, an outbox-backed queue, and a runner taking a shutdown future) is
the contract.

### Enqueue path

All three enqueue methods write a `job_outbox` row. `enqueue` and `enqueue_at` open and
commit their own transaction; `enqueue_in` writes in the caller's, so the job exists if and
only if the caller's change commits. Enqueueing never touches apalis, so the `api` role can
enqueue before any worker has started.

### Outbox relay

Runs inside `Worker::run` (so in every process that runs a worker). It holds a dedicated
`LISTEN postit_job_outbox` connection and also polls every `jobs.outbox_poll_interval`.
Each wake-up drains in batches: in one transaction, `claim_batch` (`FOR UPDATE SKIP
LOCKED`, so two relays never take the same row), push each row into the apalis storage for
the row's job type with its `run_at` (apalis holds scheduled jobs, so P8's console sees
them as `scheduled`), delete the row, commit. If apalis-postgres's push cannot join the
caller's transaction, the fallback is push-then-delete on the same connection, which widens
the duplicate window after a crash; handlers are idempotent, so this is safe, and the
first task records which path was taken. An outbox row whose job type is not registered is
logged and left in place (a newer release's job waiting for an upgraded worker).

A listener connection drop triggers a reconnect and an immediate drain.

### Queues and retry

Each `Queue` gets its own apalis worker pool with concurrency from `jobs.concurrency`, so
a slow mail server cannot starve maintenance. Each registration's `RetryPolicy` becomes
the apalis retry layer for that job type. `JobContext` exposes the attempt number and the
cap, so a handler knows when a failure is final (`send_email` records `email_failed` only
then).

### Recurring jobs

`register_recurring` wires an `apalis-cron` schedule. On each tick every process calls
`RecurringRunsRepo::try_insert_scheduled(name, tick)`; only the process whose insert
succeeded enqueues the job (through the same outbox path, then `set_job_id` once the relay
knows the apalis job ID, or at enqueue if it is assigned up front). Handler completion
calls `finish` with `succeeded` or `failed`. `insert_manual` exists for P8's trigger-now.
Tests use a per-second schedule rather than an injected clock, since apalis-cron reads
wall-clock time.

### `job_history_purge`

Recurring, queue `maintenance`. Deletes apalis jobs that succeeded before
`now - jobs.history_retention.succeeded` and those that failed, died, or were killed before
`now - jobs.history_retention.failed`, plus `job_recurring_runs` rows older than the
larger of the two. The apalis-side delete is raw SQL against apalis's schema, so it is tied
to the pinned version; its test fails loudly if apalis changes the tables.

### `data_retention` registration

`postit_jobs::maintenance::register_data_retention(&mut JobRegistry, &Settings)` registers
the daily `data_retention` job that calls `retention::purge_expired_idempotency_keys`. Plan
03 phases add their purge queries to the same handler. `postit-server` calls this function
in P6; P5 tests call it directly.

### Migrations

`migrate(pool)` runs apalis-postgres's migrations into apalis's own schema under a
`pg_advisory_xact_lock`, so concurrent role starts are safe. P6's `postit-server` calls it
right after `postit-data`'s migrations.

## Section D — `postit-mail`

### Mailer

```rust
#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError>;
}
pub struct RenderedMessage { pub to: String, pub subject: String, pub text: String, pub html: String }
```

- `SmtpMailer` (lettre, rustls): `SmtpTls::None` uses plain `builder_dangerous`,
  `Starttls` uses `starttls_relay`, `Tls` uses `relay`; credentials from
  `mail.smtp.username`/`password` as `SecretString`, never logged. SMTP 5xx maps to
  `MailError::Permanent`, connection and 4xx errors to `MailError::Transient`.
- `MemoryMailer` (always compiled, cheap): records every sent message for tests.

`RenderedMessage`'s `Debug` prints only the subject line's length and the recipient's
domain, so a stray `{:?}` never logs an address or body.

### Templates

`askama` templates embedded at compile time, each with `.txt` and `.html` variants:

- `user_pending_approval`: to an admin; lists each pending user's display name and email
  (if verified) and links to `{app.public_url}/admin/users?status=pending`.
- `user_approved`: to the user; links to `{app.public_url}`.

### Mail outbox and the `send_email` job

```rust
pub enum MailKind { UserPendingApproval, UserApproved }   // plan 03 adds notify kinds

pub struct MailOutbox { queue: JobQueue }
impl MailOutbox {
    pub async fn send(&self, conn: &mut PgConnection, kind: MailKind, recipient: UserId,
                      params: MailParams, run_at: Option<DateTime<Utc>>) -> Result<(), MailError>;
}
```

`MailParams` is a small enum of typed ID parameters (empty for both P5 kinds). The
`send_email` payload is exactly `{ kind, recipient: UserId, params }` — no address, name,
or body. A test deserializes every enqueued payload and asserts it contains no `@` and no
display name.

### Context loaders

```rust
#[async_trait]
pub trait MailContextLoader: Send + Sync {
    /// Called with the recipient row already locked by the handler.
    async fn load(&self, conn: &mut PgConnection, recipient: &UserRecord, params: &MailParams)
        -> Result<LoadOutcome, MailError>;
    async fn mark_sent(&self, conn: &mut PgConnection, recipient: UserId, at: DateTime<Utc>)
        -> Result<(), MailError>;
}
pub struct MailLoaders { .. }   // MailKind -> Arc<dyn MailContextLoader>

pub enum LoadOutcome {
    Send(MailContent),
    Skip,                       // nothing to say, or already marked
    Defer(DateTime<Utc>),       // content exists but may not be sent before this time
}
```

`postit-identity` implements the P5 loaders; plan 03's `notify` adds its own. Registering
two loaders for one kind is a startup error.

### `send_email` handler

Queue `mail`, `RetryPolicy::Backoff { max_attempts: Some(mail.send_email_max_attempts), … }`.

1. Begin a transaction; `SELECT … FOR UPDATE` the recipient's `users` row (serializes
   concurrent sends to the same recipient, which is what makes coalescing race-free).
2. Drop (record `email_dropped`, commit, return `Ok`) when the recipient is missing, not
   `active`, or has no verified email.
3. Call the kind's loader. `Skip` → commit, return `Ok` with no audit event, because a
   coalesced no-op is not a dropped mail. `Defer(at)` → `MailOutbox::send` the same
   payload with `run_at = at` in this transaction, commit, return `Ok`.
4. Render, send through `Mailer`, then `mark_sent` and record `email_sent`, commit.
5. On a `MailError::Transient`, roll back and return `JobError::Retry`; on the last attempt
   (or on `Permanent`), record `email_failed` in a fresh transaction and return
   `JobError::Fatal`.

Audit events carry the recipient's user ID as the subject and the mail kind as a detail.
Delivery is at-least-once: a crash between the SMTP accept and the commit can send twice.

The lock is held across the SMTP call. Sends are short and per-recipient, so this blocks
only another mail to the same recipient.

## Section E — `postit-identity` completion

### Errors

`IdentityError` gains `UserDeleting` (API: 409 `user_deleting`) and `CannotDeleteSelf`
(API: 403 `forbidden`). `approve`, `disable`, `enable`, and `change_role` return
`UserDeleting` instead of `InvalidTransition` when the target is `deleting`.

### Approval-request emails (coalesced)

`ClaimsTransformer::new` takes a `MailOutbox`. When provisioning creates a user that stays
`pending` (not a bootstrap admin), the same transaction enqueues one `send_email`
(`UserPendingApproval`, admin) per active admin with
`run_at = max(now, admin.last_approval_email_at + auth.approval_email_interval)`.

`PendingApprovalLoader::load` lists users still `pending` whose `created_at` is after the
admin's `last_approval_email_at`, or after the admin's `approved_at` when the marker is
null (so a new admin isn't sent every historical pending user), or after the admin's
`created_at` when both are null (the bootstrap admin). The list is capped (default 50, with
"and N more" in the template). Outcomes:

- list empty → `Skip` (an earlier email already covered these users);
- list non-empty and the marker is less than one interval old →
  `Defer(marker + interval)` (a user who signed up just after the last email still gets
  listed in the next one);
- otherwise → `Send`.

Many jobs for one admin collapse to one email per interval: the recipient row lock
serializes them, the first sends and marks, the rest `Skip` or `Defer`. Duplicate deferred
jobs for the same slot also collapse the same way. `mark_sent` writes
`last_approval_email_at`.

The existing P4 tests constructing `ClaimsTransformer` are updated to pass a `MailOutbox`
over a test pool.

### `user_approved`

`UserAdminService::approve` enqueues `send_email(UserApproved, target)` in its
transaction. `ApprovedLoader::load` returns `Send` unconditionally (no marker; at-least-once
is acceptable for a one-off account email); `mark_sent` is a no-op.

### Deletion

`UserAdminService` gains:

- `delete_user(actor, target)`: admin path and rejection of a pending user.
  `CannotDeleteSelf` when `actor == target`; `UserDeleting` when already `deleting`;
  `LastAdmin` when the target is the last active admin.
- `delete_self(user)`: the `DELETE /me` path. `LastAdmin` for the last active admin;
  `UserDeleting` when already `deleting`. Display-name confirmation is `postit-api`'s job
  in P6.

Both, in one transaction: lock the target row, set `deleting`, record `user_deleted`
(actor, subject), `enqueue_in(delete_user { user_id })`. The `NOTIFY postit_user_changed`
from `set_status` evicts the principal cache everywhere.

### `delete_user` job

Queue `default`, `RetryPolicy::Backoff { max_attempts: None, initial: 30s, max: 1h }`.
Handler holds the `audit.pseudonym_key`. Steps, each idempotent and run in its own
transaction so a crash between steps resumes cleanly:

1. Load the user. Missing → already done, return `Ok`. Not `deleting` → `Fatal` (a job
   should never exist for a non-deleting user; logged for investigation).
2. (Plan 03 inserts: cancel publish jobs, delete scheduled remote posts, revoke social
   tokens, delete owned rows and media, end delegations.) Nothing to do in P5.
3. `AuditLog::pseudonymize_user`.
4. `UsersRepo::delete` (cascades `user_preferences` and `idempotency_keys`; sets
   `approved_by` and other actor columns to null).

Crash-resume tests use a `cfg(test)`-only failure hook that fails the handler after step N,
then run the job again and assert the final state is the same as an uninterrupted run, for
every N and for both deletion paths.

### `purge_pending_users`

Recurring (`jobs.schedules.purge_pending_users`), queue `maintenance`. Lists `pending`
users created before `now - auth.pending_ttl` in batches; for each, one transaction: lock,
re-check still `pending`, set `deleting`, record `user_deleted` (no actor), `enqueue_in`
`delete_user`.

### `audit_retention`

Recurring (`jobs.schedules.audit_retention`), queue `maintenance`. Calls
`retention::purge_audit_events(audit.ip_retention, audit.retention)` and logs the counts.

### Registration

`postit_identity::jobs::register(&mut JobRegistry, &mut MailLoaders, deps)` registers
`delete_user`, `purge_pending_users`, `audit_retention`, and both mail loaders.
`postit_mail::register(&mut JobRegistry, mailer, loaders, settings)` registers
`send_email`. P6's `postit-server` calls these; P5 tests build the same composition in a
shared test harness (`testkit` features on `postit-jobs` and `postit-identity`).

## Tests

Against real Postgres (`#[sqlx::test]`) with `MemoryMailer`, driving real `Worker`
instances in-process.

- **Jobs:** a job enqueued through `JobQueue` runs; `enqueue_in` in a rolled-back
  transaction leaves no job; a committed `enqueue_in` runs after the relay is killed between
  claim and delete (duplicate tolerated, handler runs to the same end state); `enqueue_at`
  runs no earlier than `run_at`; an unregistered job type stays in the outbox; a blocked
  `mail` queue does not delay a `maintenance` job; retry policy honoured and `JobContext`
  reports the last attempt; two `Worker`s against one database fire each per-second cron
  tick exactly once (checked in `job_recurring_runs` and handler invocation counts);
  `job_history_purge` deletes only jobs and runs past their windows.
- **Mail:** no address, name, or body in any `send_email` payload; mail to a deleted,
  non-`active`, or unverified recipient is dropped with `email_dropped`; a marked mail is
  skipped on retry; a transient failure retries and a final failure records `email_failed`;
  `SmtpMailer` never logs credentials (redaction test); opt-in
  `POSTIT_SMTP_TESTS=1` sends both templates to the configured local SMTP tool.
- **Identity:** a new user's first sign-in emails every active admin; a bootstrap admin's
  first sign-in emails no one; five sign-ups inside one interval produce one email per
  admin listing all five; a second interval's sign-up produces a second email listing only
  the new user; a sign-up whose job runs just after another email was sent is deferred and
  listed in the next email, not lost; a user approved before the email is sent is not listed; approval emails the
  user; self-deletion and admin deletion resume after a failure at every step; pseudonyms
  replace every column and `_user_id` details key, are UUIDv8, do not equal a plain SHA-256
  of the ID, and are identical across two deleted events of the same user; a deleted
  admin's `approved_by` references become null without blocking deletion; status changes
  on a `deleting` user return `UserDeleting`; an admin cannot delete themselves;
  the last-admin guard on both delete paths; `purge_pending_users` deletes only pending users
  past the TTL; `audit_retention` applies both windows.
- **Config:** missing `audit.pseudonym_key` fails to load in every environment;
  `mail.smtp.tls = "none"` is rejected outside development; an invalid cron expression
  fails with an actionable message.

## Out of scope for P5

- `JobConsole`, `JobSummary`, per-type console actions, and the `/admin/jobs` routes (P8).
- `postit-server` composition, role wiring, graceful shutdown plumbing, and `/ready`
  (P6). P5 exposes `migrate`, `Worker::run(shutdown)`, and the registration functions.
- The `publishing` queue and domain-driven retry opt-outs (plan 03).
- The P4 deferred rulings (P6 / plan 03 B2).

## Exit criteria (plan 02's own)

- A job enqueued through `JobQueue` runs in a worker.
- A new user's first sign-in emails every active admin: in the local SMTP tool in
  development (opt-in test) and in `MemoryMailer` in tests. Approval emails the user. No job
  payload holds an address, name, or body.
- Self-deletion and admin deletion resume after a crash at every step and pseudonymize
  every audit reference.
- Every cron job fires on schedule (short test schedule), once per tick with two worker
  runners.
- No apalis type appears in another crate's public API.
- Quality gates pass: `cargo fmt --check`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --workspace`, `cargo sqlx prepare --check
  --workspace`.
