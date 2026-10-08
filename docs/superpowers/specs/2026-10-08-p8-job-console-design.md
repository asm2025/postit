# P8: Admin job console — design

Date: 2026-10-08, revised the same day after review. Source: `!ref/plans/02. foundation.md` phase P8 and the `postit-jobs` ("Job console API", "Safe summaries", "Safe errors", "Console actions per job type"), `postit-api` (endpoint table) and "Web client" (Jobs screens) sections. Where this spec is silent, plan 02 governs. Builds on P5 (`postit-jobs`, `APALIS_NOTES.md`), P6 (API) and P7/P7.1 (web app and design tokens).

## Goal

An admin-only, Hangfire-style job console.

- **Backend:** a postit-owned job ledger in `postit-data`, `JobConsole` and per-type `JobSummary` in `postit-jobs`, and the eight `/api/v1/admin/jobs*` endpoints in `postit-api`.
- **Web:** a Jobs section with a dashboard, a filterable list, a job detail page with every attempt, and a recurring-jobs page.
- **Exit (from plan 02):**
    - API tests: filtering and pagination per state; retry of a dead `send_email` job delivers the email once; cancelling a scheduled job stops it from running; trigger-now runs `purge_pending_users`; every console action writes its audit event; members get 403.
    - Redaction test: no email address, subject, body or SMTP detail appears in any console response for a `send_email` job, including its errors.
    - Dev stack: with the local SMTP tool stopped an admin sees a failed `send_email` job, restarts the tool, retries the job from the web app and sees it succeed.

Out of scope: live push (the web app polls), bulk actions, a payload viewer, the `web` CI job (P9), publishing job types (plan 03 registers its own summaries and disabled actions through the mechanism built here).

## Decisions

1. **A postit-owned job ledger; apalis is only the execution engine.** The console never reads or writes `apalis.*`. Reasons: apalis-postgres's listing and metrics interfaces filter by one status and queue only, offer no retry or cancel, and keep no history of tries; raw SQL against its tables would break silently on schema change. The ledger is two tables (`jobs`, `job_attempts`) behind repositories in `postit-data` using compile-time-checked `sqlx` queries like every other repository, so entity changes surface as compile errors. This amends plan 02's "implemented over apalis-postgres's listing and metrics interfaces".
2. **Every try is recorded.** `job_attempts` has one row per try, so debugging has the full history (all attempts, not just the last few; retention bounds the table). This replaces the earlier idea of encoding a `dead`/`killed` marker in apalis's `last_result` text, and closes P5 deferred item 7 (exhausted retries indistinguishable from fatal errors; `RetryAfter` losing the handler's error).
3. **Unrelayed jobs are visible.** The ledger row is written when the job is enqueued, in the same transaction as the `job_outbox` row, so a job waiting for the relay already shows as `queued` and a stopped worker appears as a growing backlog. The job ID never changes between outbox, relay and execution (the outbox UUID maps to the apalis ULID, `APALIS_NOTES.md` item B).
4. **Charting uses `recharts`**, not hand-rolled SVG (project preference: maintained libraries over hand-rolled code unless the dependency is too large). Its bundle cost is checked at plan time; it is coloured from the P7.1 tokens.
5. **Retry creates a new job.** The old job stays in its terminal state as history, and the new one links back through `retried_from`. No apalis row is ever mutated by the console.

## Data model (`postit-data`)

Per the single-migration policy (decided 2026-10-08), the plan's first data task consolidates `0001`–`0007` into one migration file, and the new tables are written directly into it as plain `CREATE TABLE` / `CREATE INDEX` statements. The consolidation concatenates the existing definitions in order and folds any later `ALTER` into the original `CREATE`, so the result has no drop-and-recreate of columns or indexes. The result is checked by comparing the schema of a database built from the old files against one built from the new file (before adding the ledger tables), then the dev database is reset and the `.sqlx` cache regenerated.

`jobs`:

| Column | Notes |
| --- | --- |
| `id` | UUID v7, primary key; equals the outbox ID and the apalis task ID (as ULID) |
| `job_type`, `queue` | text |
| `payload` | JSONB; IDs only by the `Job` contract; never returned by the API |
| `state` | `queued`, `running`, `succeeded`, `failed`, `dead`, `killed`, `cancelled` (stored); `scheduled` is derived at read time (`queued` with `run_at > now()`) |
| `run_at` | when the job becomes (or became) due |
| `attempts` | tries started so far |
| `max_attempts` | from the type's `RetryPolicy`; null = unlimited |
| `recurring_name` | set for recurring ticks and manual triggers |
| `retried_from` | nullable FK to `jobs(id)` |
| `created_at`, `started_at`, `finished_at` | timestamps (`started_at`/`finished_at` describe the latest try) |

Indexes: `(state, run_at)`, `(job_type, created_at)`, `(finished_at)`, and `(recurring_name, created_at)`.

`job_attempts`:

| Column | Notes |
| --- | --- |
| `job_id` | FK to `jobs(id)`, `ON DELETE CASCADE` |
| `attempt` | 1-based; primary key `(job_id, attempt)` |
| `started_at`, `finished_at` | `finished_at` is null while running |
| `worker` | worker name |
| `outcome` | `succeeded`, `retrying`, `failed` (final, retries exhausted), `killed` (fatal, unknown type, panic), `interrupted` (worker died mid-try), `cancelled` |
| `error_kind` | `retry`, `retries_exhausted`, `fatal`, `unknown_type`, `panic`, `interrupted` |
| `error_code` | stable short code from the handler's error |
| `error_message` | stored only when the job type opted in to messages (see Safe output); otherwise null, so user content never reaches the table |

`job_recurring_runs` is unchanged; recurring rows in `jobs` carry `recurring_name`, and the recurring page still reads last run and outcome from `job_recurring_runs`.

Repositories (`JobsRepo`, `JobAttemptsRepo`) own all SQL: insert, state transitions (each a guarded `UPDATE … WHERE state = …`), list with filters and pagination (`emixdb::Pagination` and `ResultSet<T>`), stats aggregates (counts per state and per type, queue depth, throughput buckets from `job_attempts.finished_at`), and the retention purge.

## `postit-jobs`

### Ledger writes

- **Enqueue.** `JobQueue::enqueue_raw_in` inserts the `jobs` row (state `queued`, `max_attempts` from the registry's retry policy when known, otherwise filled when first dispatched) and the outbox row in the caller's connection or transaction, so both exist if and only if the caller commits. Recurring ticks and manual triggers go through the same path and set `recurring_name`.
- **Dispatch.** `dispatch` is the only writer of attempt transitions. At the start of a try:
    1. If the job is `cancelled`, it finishes without running the handler.
    2. A previous attempt still open is closed as `interrupted` (the worker died).
    3. A new attempt row is inserted and the job becomes `running`.
  When the try ends, the attempt and the job are updated together in one transaction: `Done` gives `succeeded`; `RetryAfter` gives outcome `retrying`, job `failed` with `run_at` set to the next due time; `Abort` gives `dead` when the cause was exhausted retries, otherwise `killed`. Ledger writes are best-effort around execution: a failed write is logged and never changes whether or how the job runs (as recurring-run outcomes are handled today).
- **Retention.** `job_history_purge` deletes finished `jobs` (and their attempts by cascade) past `jobs.retention.succeeded` / `failed`, keeps its existing apalis-row purge, and also deletes stale `apalis.workers` rows (P5 deferred item 6).

### Registration metadata

`JobRegistry::register` and `register_recurring` take a `ConsoleSpec` (a builder with defaults, so existing call sites change by one argument):

- `summary`: `fn(&Value) -> JobSummary`. `JobSummary` is a small ordered map of string fields. The builder only accepts keys ending in `_id` and values from a closed set of enum-like strings (e.g. `mail_kind`), so a summary cannot carry free text. A payload that fails to parse yields an empty summary.
- `actions`: which console actions apply (`retry`, `cancel`, `delete`), default all three.
- `disabled_reason`: shown when the actions are disabled because domain state drives the work (plan 03 publishing jobs).
- `show_error_message`: opt-in to storing and returning the redacted error message, default off.

The `api` role registers no handlers, so the metadata comes from a shared `register_console(&mut JobRegistry)` that every role runs. Summaries for this plan's six types: `send_email` (`recipient_user_id`, `mail_kind`), `delete_user` (`user_id`), and the four recurring/maintenance types (no fields). Only `job_history_purge` opts in to error messages.

### Console states

`queued | scheduled | running | succeeded | failed | dead | killed`, plus `cancelled` (an addition to the plan's seven, because a cancelled job is neither a failure nor a success; the failed-jobs badge counts `failed + dead + killed` only). `failed` means a try failed and a retry is waiting.

### `JobConsole`

A struct next to `JobQueue`, built on the repositories (no apalis types in its API, and no apalis queries at all):

- `list(filter, pagination)`: filters on state, job type and time range (on `run_at` for queued/scheduled, `finished_at` for finished states).
- `detail(id)`: the job, its summary, `retried_from` and any job that retried it, and every attempt (number, timings, worker, outcome, error kind/code, and message where allowed).
- `stats(window)`: counts per state and per type, queue depth per queue, throughput as succeeded/failed per bucket (24 hourly buckets by default).
- `retry(admin, id)`: for `failed`, `dead`, `killed` or `cancelled`. Creates a new job with the same type and payload (`retried_from` set) through the normal enqueue path.
- `cancel(admin, id)`: for `queued` or `scheduled`. Marks the job `cancelled` (guarded `UPDATE … WHERE state = 'queued'`) and deletes its unrelayed outbox row if still present; if the row was already relayed, `dispatch` skips it. A `running` job is refused.
- `delete(admin, id)`: for finished states. Removes the ledger rows; the apalis row is left to `job_history_purge`.
- `recurring()`: name, cron expression (from the registry), last run and outcome (`job_recurring_runs`), next run (from the schedule).
- `trigger(admin, name)`: the existing manual-run path; the `api` role never runs the job in-process.

Every action checks the registration (`actions`) and the current state in one guarded statement, so concurrent actions cannot race. Failures return `ConsoleError::{NotFound, NotAllowed(reason)}`.

### Safe output

The console returns `error_kind` and `error_code`, and `error_message` only for types with `show_error_message` (where it is also the only case in which a message is stored). Raw payloads are never returned. Tests build error messages containing an address and an SMTP detail and assert neither appears in the table or any response.

## Audit

`AuditEventKind` gains `JobRetried`, `JobCancelled`, `JobDeleted`, `JobTriggered` (`job_retried`, `job_cancelled`, `job_deleted`, `job_triggered`); `ALL` and `as_str` update. The `kind` column is `TEXT`, so there is no change for it. Details carry `job_id`, `job_type`, and for retries the new job's ID (`new_job_id`) or for triggers `name`. `postit-api` records the event in the same request as the action, with the acting admin as actor. Audit events record admin actions; the attempt table is the debugging record of what the job did.

## `postit-api`

- **Routes** in `routes/jobs.rs`, all behind `RequireAdmin`, `X-Postit-Act-As` rejected like the other admin routes: `GET /admin/jobs`, `GET /admin/jobs/stats`, `GET /admin/jobs/{id}`, `POST /admin/jobs/{id}/retry`, `POST /admin/jobs/{id}/cancel`, `DELETE /admin/jobs/{id}`, `GET /admin/jobs/recurring`, `POST /admin/jobs/recurring/{name}/trigger`. Static segments are registered ahead of `{id}`.
- **List query:** `state`, `job_type`, `from`, `to`, `page`, `page_size`, with `deny_unknown_fields`; `from > to` and unknown states are 422 `validation_failed`.
- **DTOs** (`utoipa` schemas, enums registered in `openapi.rs`): `JobDto` (id, type, queue, state, attempts, max attempts, `run_at`, timestamps, `summary`, last `error_kind`/`error_code`, `actions`, `actions_disabled_reason`), `JobDetailDto` (adds `attempts_list`, `retried_from`, `retried_by`), `JobStatsDto`, `RecurringJobDto`. `actions` lists what is allowed for that job right now, so the web app does not duplicate the rules. Clients must tolerate new `JobState` values.
- **Errors:** an unknown job or recurring name is 404 `not_found`; an action that does not apply to the job's type or state is 409 with a new stable code `job_action_not_allowed` (additive within v1, added to the error table).
- **Wiring:** `postit-server` builds `JobConsole` over the pool and the console metadata in every role and puts it in `AppState`. `cargo xtask openapi` regenerates `api/openapi.json` and `web/src/api/schema.d.ts`; the `contract` diff is additive only.

## Web (`web/`)

New `features/jobs/`, admin-gated like Users and Audit, using the P7.1 tokens, fonts and existing components (`Panel`, `Pager`, `Table`, `Badge`, `ConfirmDialog`).

- **Routes:** `/jobs` (dashboard), `/jobs/list` (list), `/jobs/:id` (detail), `/jobs/recurring`.
- **Dashboard:** a counter card per state; a `recharts` stacked bar chart of throughput (succeeded and failed); queue depth per queue. TanStack Query polls stats every 5 s and pauses when the tab is hidden.
- **List:** state tabs, job-type select, time range, pager; rows link to the detail.
- **Detail:** summary IDs, timings, the retry chain, the attempt list (each try's timings, worker, outcome, error kind/code and message when present), and action buttons rendered from `actions` (with `actions_disabled_reason` as the explanation when empty).
- **Actions:** retry, cancel and delete go through `ConfirmDialog`; success invalidates the list, detail and stats queries. Retry navigates to the new job.
- **Recurring:** name, cron, last run and outcome, next run, Trigger now (confirmed).
- **Navigation:** a Jobs item in the sidebar's Admin group with a failed-jobs badge (`failed + dead + killed` from stats, polled like the Users pending count).
- **Errors:** `job_action_not_allowed` is mapped to a user-facing message in the existing problem+json mapping. Unknown states render as their raw string.
- **Accessibility and layout:** per P7.1: 44 px touch targets, visible focus, reduced motion, usable from 360 px; the chart has a text alternative (a visually hidden table or labelled summary) and both themes meet AA contrast.

## Testing

- **`postit-data`:** `#[sqlx::test]` suites for both repositories: guarded transitions, filters and pagination, stats and buckets, cascade delete, retention purge. `cargo sqlx prepare --check` passes with the committed cache.
- **`postit-jobs` (real Postgres, real worker):**
    - Ledger stays consistent with execution: enqueue in a rolled-back transaction leaves no ledger row; success, retry-then-success, retries exhausted (`dead`), fatal and panic (`killed`), unknown type, cancel before relay, cancel after relay (handler never runs), cancel refused while running, and a worker crash leaving an `interrupted` attempt that the next try closes.
    - Every `JobState` classification, including `scheduled` derivation and unrelayed jobs.
    - Retry creates a linked job and leaves the original untouched; concurrent actions cannot both succeed.
    - Stats, throughput buckets, recurring listing and trigger.
    - Redaction: an error message containing an address and SMTP detail is neither stored nor returned for a type that did not opt in, and is returned (redacted) for one that did.
    - Retention purge of ledger rows and stale worker rows.
    - A ledger write failure does not change whether the job runs.
- **`postit-api` (test issuer, `MemoryMailer`):** the exit-criteria cases above, plus `from > to` 422, 404 for unknown IDs, 409 `job_action_not_allowed`, every action's audit event, members and pending users refused, and act-as rejected.
- **Web (Vitest, Testing Library, MSW):** route guard (members cannot reach it), state tabs and filters, buttons following `actions`, confirm dialogs, the attempt list, the failed-jobs badge, the polling hook, and chart render. `tsc --noEmit`, lint, format and the build are clean.
- **Manual (recorded in the plan):** the dev-stack SMTP-stopped scenario from the exit criteria.

## Risks

- **Two records of one job (ledger and apalis).** Mitigated by one writer for transitions (`dispatch`), enqueue-time ledger insert in the caller's transaction, `interrupted` recovery, and the consistency tests above. The ledger, not apalis, is what the console trusts.
- **Write volume.** Each try adds an attempt row and a job update. Fine for a small team's job volume; retention keeps the tables small.
- **Pre-existing dev data.** Jobs enqueued before this release have no ledger row and never appear in the console. Acceptable pre-release; dev databases are reset under the single-migration policy.
- **Retry of a job that partly ran.** Safe by the plan's rule that every handler is idempotent against domain state; the `send_email` sent-marker covers the exit-criteria case.
