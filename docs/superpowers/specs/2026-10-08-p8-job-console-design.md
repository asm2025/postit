# P8: Admin job console — design

Date: 2026-10-08. Source: `!ref/plans/02. foundation.md` phase P8 and the `postit-jobs` ("Job console API", "Safe summaries", "Safe errors", "Console actions per job type"), `postit-api` (endpoint table) and "Web client" (Jobs screens) sections. Where this spec is silent, plan 02 governs. Builds on P5 (`postit-jobs`, `APALIS_NOTES.md`), P6 (API) and P7/P7.1 (web app and design tokens).

## Goal

An admin-only, Hangfire-style job console.

- **Backend:** `JobConsole` and per-type `JobSummary` in `postit-jobs`, and the eight `/api/v1/admin/jobs*` endpoints in `postit-api`.
- **Web:** a Jobs section with a dashboard, a filterable list, a job detail page and a recurring-jobs page.
- **Exit (from plan 02):**
    - API tests: filtering and pagination per state; retry of a dead `send_email` job delivers the email once; cancelling a scheduled job stops it from running; trigger-now runs `purge_pending_users`; every console action writes its audit event; members get 403.
    - Redaction test: no email address, subject, body or SMTP detail appears in any console response for a `send_email` job, including its errors.
    - Dev stack: with the local SMTP tool stopped an admin sees a failed `send_email` job, restarts the tool, retries the job from the web app and sees it succeed.

Out of scope: live push (the web app polls), a full per-attempt history table, bulk actions, a payload viewer, the `web` CI job (P9), publishing job types (plan 03 registers its own summaries and disabled actions through the mechanism built here).

## Decisions

1. **Data access: runtime SQL in `apalis_sql.rs`**, not apalis-postgres's listing/metrics interfaces. Plan 02 names those interfaces; P5 pinned the storage schema in `APALIS_NOTES.md` and isolated all raw `apalis.*` SQL in `apalis_sql.rs`, and the rc-version interfaces have no type or time filters and know nothing of the outbox. The console reads `apalis.jobs`, `job_outbox` and `job_recurring_runs` through the same module, and its tests pin the schema assumptions. This amends the plan wording.
2. **`dead` vs `killed` is recorded at abort time.** Both end `Killed` in apalis today. `backend::handle` already turns `Outcome::Abort(message)` into `AbortError(Failure(message))`, and apalis stores that error text in `last_result` (`{"Err": "<text>"}`). The abort path now serializes a small JSON object into that text: `{"kind": "retries_exhausted" | "fatal" | "unknown_type" | "panic", "code": "<stable code>", "message": "<redacted message>"}`. The console parses it. `retries_exhausted` is state `dead`; the other kinds are `killed`. A `Killed` row whose text does not parse (written by an older build) is `killed` with `error_kind` null. `Outcome::RetryAfter` also stores its error through the same shape (`kind: "retry"`), so `failed` rows show why they failed. This closes P5 deferred item 7.
3. **Unrelayed outbox rows are visible.** A `job_outbox` row appears as a `queued` job (ID, type, `run_at`, created time only). Counts and queue depth include it, so a stopped worker shows as a growing backlog. It can be cancelled (the outbox row is deleted) but not retried. The job ID is the same before and after relay (the outbox UUID maps to the apalis ULID, `APALIS_NOTES.md` item B).
4. **Charting uses `recharts`**, not hand-rolled SVG (project preference: maintained libraries over hand-rolled code unless the dependency is too large). Its bundle cost is checked at plan time; it is coloured from the P7.1 tokens.

## `postit-jobs`

### Registration metadata

`JobRegistry::register` and `register_recurring` take a `ConsoleSpec` (a builder with defaults, so existing call sites change by one argument):

- `summary`: `fn(&Value) -> JobSummary`. `JobSummary` is a small ordered map of string fields. The builder only accepts keys ending in `_id` and values from a closed set of enum-like strings (e.g. `mail_kind`), so a summary cannot carry free text. A payload that fails to parse yields an empty summary.
- `actions`: the console actions that apply (`retry`, `cancel`, `delete`), default all three.
- `disabled_reason`: a string shown when the actions are disabled because domain state drives the work (plan 03 publishing jobs).
- `show_error_message`: opt-in to showing the redacted error message, default off.

The registry exposes the console metadata without handlers, so the `api` role (which registers no handlers) builds the same metadata through a shared `register_console(&mut JobRegistry)` call that every role runs. Summaries for this plan's six types: `send_email` (`recipient_user_id`, `mail_kind`), `delete_user` (`user_id`), and the four recurring/maintenance types (no fields). Only `job_history_purge` opts in to error messages.

### States

`JobState`: `queued | scheduled | running | succeeded | failed | dead | killed`.

| apalis row | console state |
| --- | --- |
| `Pending`, `run_at <= now()`, or an unrelayed outbox row | `queued` |
| `Pending`, `run_at > now()` | `scheduled` |
| `Queued` (claimed, not started) or `Running` | `running` |
| `Done` | `succeeded` |
| `Failed` with `attempts < max_attempts` (waiting to retry) | `failed` |
| `Killed` with marker `retries_exhausted` | `dead` |
| `Killed` otherwise | `killed` |

Job type comes from the envelope inside `apalis.jobs.job` (BYTEA JSON, `job_type` field); the `job_type` column holds the queue name. Type filters therefore extract `convert_from(job, 'UTF8')::jsonb ->> 'job_type'`. The list query is bounded by state/time filters and pagination; the plan adds an index only if the query plan test shows a need (the single-migration policy applies to any schema change).

### `JobConsole`

A struct next to `JobQueue` (no apalis types in its API):

- `list(filter, pagination) -> ResultSet<JobRow>`: filters on state, job type and time range (on the state's relevant timestamp: `run_at` for queued/scheduled, `done_at` for finished states).
- `detail(id) -> JobDetail`: type, queue, state, attempts, max attempts, timings, summary, and the last error. apalis stores only the last result, so detail shows the last error and the attempt count, not a per-attempt table (stated limitation).
- `stats(window) -> JobStats`: counts per state and per type, queue depth per queue, and throughput as succeeded/failed counts per bucket (24 hourly buckets by default; bucket size derived from the window).
- `retry(admin, id)`: for `failed`, `dead` or `killed`. Resets the same row to `Pending`, `attempts = 0`, `run_at = now()`, clears `lock_by`/`done_at`, and notifies workers. The same ID is kept so the audit trail stays connected.
- `cancel(admin, id)`: for `queued` or `scheduled`. Deletes the apalis row, or the outbox row. A `running` job is refused.
- `delete(admin, id)`: for finished states (`succeeded`, `dead`, `killed`, exhausted `failed`).
- `recurring() -> Vec<RecurringRow>`: name, cron expression, last run and outcome (from `job_recurring_runs`), next run (from the schedule).
- `trigger(admin, name)`: the existing manual-run path (insert a manual `job_recurring_runs` row, enqueue). The `api` role never runs the job in-process.

Every action first checks that the job's registration allows it and that its current state allows it, in one statement (`UPDATE … WHERE status = …`) so concurrent actions cannot race. Failures return `ConsoleError::{NotFound, NotAllowed(reason)}`.

### Safe output

The console returns `error_kind` and `error_code` only. The redacted message is included only for types with `show_error_message`. Raw payloads are never returned. Tests build error messages containing an address and an SMTP detail and assert neither appears.

### Housekeeping folded in

`job_history_purge` also deletes `apalis.workers` rows whose `last_seen` is older than a threshold (P5 deferred item 6). The other P5 deferred items stay deferred.

## `postit-data` and audit

`AuditEventKind` gains `JobRetried`, `JobCancelled`, `JobDeleted`, `JobTriggered` (`job_retried`, `job_cancelled`, `job_deleted`, `job_triggered`); `ALL` and `as_str` update. The `kind` column is `TEXT`, so there is no migration. Details carry `job_id` and `job_type` (and `name` for triggers). `postit-api` calls `AuditLog::record` in the same request as the action, with the acting admin as actor.

## `postit-api`

- **Routes** in `routes/jobs.rs`, all behind `RequireAdmin`, `X-Postit-Act-As` rejected like the other admin routes: `GET /admin/jobs`, `GET /admin/jobs/stats`, `GET /admin/jobs/{id}`, `POST /admin/jobs/{id}/retry`, `POST /admin/jobs/{id}/cancel`, `DELETE /admin/jobs/{id}`, `GET /admin/jobs/recurring`, `POST /admin/jobs/recurring/{name}/trigger`. Static segments are registered ahead of `{id}`.
- **List query:** `state`, `job_type`, `from`, `to`, `page`, `page_size`, with `deny_unknown_fields`; `from > to` and unknown states are 422 `validation_failed`.
- **DTOs** (all with `utoipa` schemas, enums registered in `openapi.rs`): `JobDto` (id, type, queue, state, attempts, max attempts, `run_at`, timestamps, `summary`, `error_kind`, `error_code`, `actions`, `actions_disabled_reason`), `JobDetailDto`, `JobStatsDto`, `RecurringJobDto`. `actions` lists the actions allowed for that job right now, so the web app does not duplicate the rules. Clients must tolerate new `JobState` values.
- **Errors:** unknown job or recurring name is 404 `not_found`; an action that does not apply to the job's type or state is 409 with a new stable code `job_action_not_allowed` (additive within v1, documented in the error table).
- **Wiring:** `postit-server` builds `JobConsole` over the pool and the console metadata in every role and puts it in `AppState`. `cargo xtask openapi` regenerates `api/openapi.json` and `web/src/api/schema.d.ts`; the `contract` diff is additive only.

## Web (`web/`)

New `features/jobs/`, admin-gated like Users and Audit, using the P7.1 tokens, fonts and existing components (`Panel`, `Pager`, `Table`, `Badge`, `ConfirmDialog`).

- **Routes:** `/jobs` (dashboard), `/jobs/list` (list), `/jobs/:id` (detail), `/jobs/recurring`.
- **Dashboard:** a counter card per state; a `recharts` stacked bar chart of throughput (succeeded and failed); queue depth per queue. TanStack Query polls stats every 5 s and pauses when the tab is hidden.
- **List:** state tabs, job-type select, time range, pager; rows link to the detail.
- **Detail:** summary IDs, timings, attempts, error kind/code (message only when the API returns one), and action buttons rendered from `actions` (with `actions_disabled_reason` as the explanation when empty).
- **Actions:** retry, cancel and delete go through `ConfirmDialog`; success invalidates the list, detail and stats queries.
- **Recurring:** name, cron, last run and outcome, next run, Trigger now (confirmed).
- **Navigation:** a Jobs item in the sidebar's Admin group with a failed-jobs badge (`failed + dead + killed` from stats, polled like the Users pending count).
- **Errors:** `job_action_not_allowed` is mapped to a user-facing message in the existing problem+json mapping. Unknown states render as their raw string.
- **Accessibility and layout:** per P7.1: 44 px touch targets, visible focus, reduced motion, usable from 360 px; the chart has a text alternative (a visually hidden table or labelled summary) and both themes meet AA contrast.

## Testing

- **`postit-jobs` (real Postgres, real worker):**
    - State classification for every row of the table above, including outbox rows.
    - The `dead`/`killed` marker, and an unparseable legacy `Killed` row.
    - Filtering by state, type and time range with pagination.
    - Retry, cancel and delete, including refusal on a wrong state, on a disabled type and on concurrent actions.
    - Stats and throughput buckets; recurring listing and trigger.
    - Redaction: an error message with an address and SMTP detail never appears for a type that did not opt in, and appears (redacted) for one that did.
    - Stale worker rows purged.
    - Schema pin: a test that fails loudly if the assumed `apalis.jobs` columns change.
- **`postit-api` (test issuer, `MemoryMailer`):** the exit-criteria cases above, plus `from > to` 422, 404 for unknown IDs, 409 `job_action_not_allowed`, every action's audit event, members and pending users refused, and act-as rejected.
- **Web (Vitest, Testing Library, MSW):** route guard (members cannot reach it), state tabs and filters, buttons following `actions`, confirm dialogs, the failed-jobs badge, the polling hook, and chart render. `tsc --noEmit`, lint, format and the build are clean.
- **Manual (recorded in the plan):** the dev-stack SMTP-stopped scenario from the exit criteria.

## Risks

- **apalis rc schema drift.** Mitigated by the pinned versions, isolation in `apalis_sql.rs`, and the schema-pin test.
- **Type filter cost.** JSON extraction from BYTEA is not indexable as written; fine for a small team's job volume, and retention keeps the table small. Revisit if the query-plan test shows a problem.
- **Retry of a job that partly ran.** Safe by the plan's rule that every handler is idempotent against domain state; the `send_email` sent-marker covers the exit-criteria case.
