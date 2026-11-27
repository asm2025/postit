# D4a Workspace and accounts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first D4 slice: a web user switches workspaces, manages delegations, preferences, and notifications, connects accounts by credentials, by OAuth, and by CSV import (now a background job), manages accounts, writing styles, and target groups, and an admin manages platforms, the live switch, and per-user limits.

**Architecture:** Three small backend changes (connection guides in the agent descriptor; the account CSV import moved onto the jobs ledger with sealed staging rows, after the contact import's pattern; a single-notification route for push clicks), then web screens over the D3 REST surface. One workspace module owns act-as (header, cache keys, remount on switch, ended grants); one `useAccess` hook gates actions; one `SchemaForm` kit component renders every credentials form. No platform-specific code in the web app.

**Tech Stack:** Rust (axum, sqlx, apalis behind `postit-jobs`, `postit-vault`), React 19, React Router 8, TanStack Query 5, `openapi-fetch`, i18next, Radix/shadcn, Tailwind 4, Vitest + MSW, Playwright.

**Spec:** `docs/specs/2026-11-27-d4-web-publishing-ui-design.md` (sections "Shared decisions", "Backend changes D4 needs: D4a", "Slice details: D4a"). Also read plan 03 Phase D4 and the REST surface table in `docs/contracts/03. publishing roadmap.md`.

## Global Constraints

- US English everywhere (code, catalogs, docs, commits).
- Web: pnpm only, run from PowerShell (the Git Bash pnpm shim is broken). Gate: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build` in `web/`.
- Pages use `web/src/components/kit/` and `web/src/components/ui/`; `kitGuard.test.ts` must stay green. A missing widget becomes a kit component.
- Every user-visible string is in an `en-US` catalog under `web/src/i18n/en-US/`; layouts use logical properties and mirror under `dir="rtl"`. A task that adds a namespace registers it in both `web/src/i18n/index.ts` (resources) and `web/src/i18n/i18next.d.ts` (types), or `pnpm typecheck` fails.
- No platform ID literal (`telegram`, `email`, `slack`, …) in `web/src/features/**` or `web/src/components/**` outside tests; platform names, colors, icons, forms, and guides come from `GET /platforms`.
- No credential value, API key, or `connect_id` in `localStorage`, logs, or error reports. `connect_id` lives in `sessionStorage` for at most its 10-minute lifetime.
- Backend gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, targeted `cargo test -p <crate>` for crates whose behavior changed (not the full workspace by reflex), `cargo xtask openapi --check` (PowerShell; regenerates `web/src/api/schema.d.ts` and `mobile/lib/api/`).
- `CARGO_TARGET_DIR=K:/cargo-target/postit`; DB tests use `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, quotes stripped; `-- --test-threads=4` for `postit-data`.
- Schema changes edit `backend/crates/data/migrations/0001_InitialSchema.sql` in place (single-migration policy); new `query!` macros need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data`.
- Workspace lints deny `unwrap_used` and `expect_used` outside tests.
- Commit trailer: `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`.

## Review Focus

1. **Workspace switch with data or state in flight:** a response for workspace A arriving after the switch to B must never render in B, a form or dialog opened in A must not submit in B, and switching A → B → A must show A's data, not B's. Tests in Task 6 (`switching drops the old workspace's queries and late answers`, `switching remounts the page`, `A to B to A shows A's data`) and the static `queryKeyGuard.test.ts`.
2. **Act-as on self and admin routes:** the routes that refuse `X-Postit-Act-As` (every `/me…`, `/notifications…`, `/push…`, `/users…` including the admin user routes, `/delegations/received…`, `/auth/config`, `/admin/…`) must never carry it, or the server answers `owner_only`; workspace routes (including `/media…`) must always carry it. Matching is by whole path segment, never by string prefix (`me` must not match `media`). Test in Task 6 (`needsActAs` table test, checked against `backend/openapi.json`).
3. **A grant that ends mid-session:** any `403 delegation_invalid` switches the tab to the own workspace once and shows why, without a retry loop. Test in Task 6.
4. **A stale or foreign pending connection:** a `connect_id` entry past `expires_at`, or one left from another user in the same tab, is dropped and no checklist is shown; a reload inside 10 minutes resumes. Test in Task 13.
5. **CSV import edge cases:** a header error answers at once (`422 csv_invalid`) with no import created, and so does a declared `Content-Length` over the limit (`413`); a body that passes the header check and then goes over the byte or row limit, or fails to read, is not thrown away: the import says why it stopped and the last file line it took; a dry run reports those rows as usual, and a commit run connects nothing until the user chooses to connect the rows read (then removes those lines and imports the rest) or to cancel, which deletes the staged rows; an import left waiting for that choice is failed and its rows deleted by the 24-hour stale sweep; a malformed row is skipped and reported, never stops the file; a crash mid-chunk resumes without losing or double-connecting rows; a commit after a reload (the file no longer in memory) asks the user to pick the file again; a 1,000-row file (the default `accounts.csv_max_rows`) shows progress, not a frozen page. Tests in Tasks 2, 3, 4, and 15.
6. **Opening a notification:** a notice about an owner's workspace switches there first; a notice about the user themselves (`delegation_changed`, `user_pending`, deployment notices) never switches. Test in Task 10.

## Decisions taken in this plan

- **Guide shape (call, departs from the spec's flat `guide` + `guide_url`):** one `guide` object `{ steps, url }`, so the steps and their link travel and default together.
- **Import progress answer (call):** `202` returns the whole `AccountImportDto` (a superset of the spec's `{ import_id }`), so the first render needs no second request.
- **Import status names:** `processing`, `completed`, `failed`, as `contact_imports` uses (the spec wrote `done`), plus `awaiting_decision` and `canceled` for a cut-short commit (next item).
- **A body cut short (decided):** the import stages every row read whole before the cut and records `stopped` (`max_rows`, `max_bytes`, or `unreadable`) with the last data row and file line taken. Only a bad or unreadable header refuses the upload. A dry run then runs as usual (it changes nothing). A commit run stops at `awaiting_decision` with nothing connected, and the user decides: **Connect the rows read** (`POST /accounts/imports/{id}/continue` queues the job) or **Cancel** (`DELETE /accounts/imports/{id}` deletes the staged rows; status `canceled`). The choice comes before anything is applied, because an undo afterwards cannot restore the credentials a row replaced on an account the user already had, and the new accounts may already be in use. A waiting import is swept like a stalled one: after 24 hours without a change it is `failed` and its staged rows are deleted. There is no undo for a finished import (decided).
- **Row positions:** each report entry carries `row` (1-based data row) and `line` (the file line the record starts on, from the CSV reader's position, so quoted multi-line cells count right); the screen shows lines.
- **Push click (decided):** a new self route `GET /api/v1/notifications/{id}` resolves the notification the service worker names.
- **Delegations while delegated (decided):** the nav keeps "Delegations"; in a delegated workspace the page shows only the Received tab, and Granted needs the own workspace.
- **`/platforms` and `/ai/presets` without act-as (call):** both accept the header but answer the same for every workspace, and their cache keys are not workspace-scoped, so the client does not send it; an ended grant then cannot fail the platform catalog.

## Deferred and verification items

- Picked up: B3 "Import progress for large files" (Tasks 2–4); C3 "The connection guide", web half (Tasks 1 and 13; stays open for D6).
- Not in D4a: C3 "Telegram counts characters, not UTF-16 units". The spec routes the check to "the D4a plan", but the live counter that could make the early stop visible is the D4b composer, so D4b evaluates it; no change here.
- Later slices: C1/C2 DNS finding codes (D4d); B4 and B6 Dart entries (D6). The B6 web counterpart (a PATCH must send the current `expires_at`) is handled in Task 11.
- Human checks: `2026-11-27-consolidated-verification.md` has no D4a-triggered items (D2 checks wait for D4c, Papercut for D4d). This slice's own Playwright run goes to its verification file (Task 17).

---

## File Structure

Backend:
- `backend/crates/core/src/descriptor.rs` — `ConnectGuide`, `AgentDescriptor.guide`, `DescriptorError::InvalidGuideUrl`.
- `backend/crates/core/src/ids.rs` — `AccountImportId`.
- `backend/crates/agents/{telegram,email,mock}/src/descriptor.rs` — guide steps.
- `backend/crates/data/migrations/0001_InitialSchema.sql` — `account_imports`, `account_import_rows`.
- `backend/crates/data/src/account_imports.rs` (new) — `AccountImportRepo`.
- `backend/crates/data/src/vault.rs`, `retention.rs` — sealed column entry; purge and stale sweep.
- `backend/crates/oauth/src/import.rs` — staged import service; `backend/crates/oauth/src/jobs.rs` — `AccountImportJob`.
- `backend/crates/api/src/routes/accounts.rs`, `dto_accounts.rs`, `routes/mod.rs` — `202` import, `GET /accounts/imports/{id}`; `routes/notifications.rs` — `GET /notifications/{id}`.
- `backend/crates/server/src/compose.rs`, `lib.rs` — wiring.

Web (new folders follow `features/jobs/`):
- `web/src/workspace/` — `store.ts`, `WorkspaceProvider.tsx`, `useAccess.ts`, `WorkspaceSwitcher.tsx`, `WorkspaceBanner.tsx`, `keys.ts`, `queryKeyGuard.test.ts`.
- `web/src/api/actAs.ts` — the self/admin segment rules.
- `web/src/components/kit/` — `SchemaForm.tsx`, `schema.ts`, `PlatformBadge.tsx`, `AccountPicker.tsx`, `ProgressBar.tsx`; `web/src/components/ui/` — `popover.tsx`, `tabs.tsx`, `textarea.tsx` (shadcn, the repo's `radix-nova` style with `rtl: true`).
- `web/src/i18n/agentText.ts`, `web/src/i18n/en-US/{agents,accounts,delegations,notifications,admin}.json`.
- `web/src/features/platforms/queries.ts` — `usePlatforms`, `usePlatformIcon`.
- `web/src/features/notifications/`, `web/src/features/delegations/`, `web/src/features/accounts/` (list, detail, connect, checklist, oauth-done, import, target groups), `web/src/features/admin/` (platforms, publishing, limits).
- `web/public/sw.js` unchanged (it already opens `/?notification=<id>`).
- `web/e2e/d4a.spec.ts`, `web/e2e/support.ts`.

---

### Task 1: Connection guides in the agent descriptor

Picks up `docs/plans/2026-11-01-c3-telegram-agent-deferred.md`, "The connection guide" (web half).

**Files:**
- Modify: `backend/crates/core/src/descriptor.rs`, every `AgentDescriptor { … }` literal (`rg "AgentDescriptor \{" backend/crates` lists them: core tests, agent-kit echo, drafter testkit, oauth catalog tests, publishing outcome tests, core wire_format test, the three agents)
- Modify: `backend/crates/agents/telegram/src/descriptor.rs`, `backend/crates/agents/email/src/descriptor.rs`, `backend/crates/agents/mock/src/descriptor.rs`
- Test: `backend/crates/core/tests/wire_format.rs`, `descriptor.rs` unit tests, each agent's descriptor tests

**Interfaces:**
- Produces: `#[derive(Debug, Clone, Default, Serialize)] #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))] pub struct ConnectGuide { pub steps: Vec<Message>, #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))] pub url: Option<Cow<'static, str>> }`; `AgentDescriptor.guide: ConnectGuide`. `AgentDescriptor` derives only `Serialize` (its `icon_png` is `&'static [u8]`), so `ConnectGuide` needs no `Deserialize` and the field needs no `#[serde(default)]`. The URL is a `Cow<'static, str>` rather than `Url` because descriptors are built at runtime and `Url::parse` returns a `Result` (`expect_used` is denied); `AgentDescriptor::validate()` checks it instead: `DescriptorError::InvalidGuideUrl(String)` unless it parses as an absolute `https` URL. On the wire: `"guide": { "steps": [{ "key", "en" }], "url": null }`.

- [ ] **Step 1: Write failing tests**
  - `wire_format.rs::descriptor_serializes_guide`: a descriptor with two steps and a URL serializes `guide.steps[0] == {"key":"x.guide.1","en":"…"}` and `guide.url`; one built with `ConnectGuide::default()` serializes `"guide": {"steps": [], "url": null}`.
  - `descriptor.rs` unit test `validate_refuses_a_non_https_guide_url`: `http://…` and `not a url` give `InvalidGuideUrl`; `https://…` and `None` pass.
  - `telegram` descriptor test `guide_covers_the_c3_steps`: 5 steps, keys `telegram.guide.1`…`telegram.guide.5`, English text covering, in order: create a dedicated bot with BotFather; add it as a channel or group admin with "Post messages"; add a private channel by its numeric ID (an invite link does not work); when replacing a revoked token, name the chats again (discovery only sees the last 24 hours of updates); to disconnect for good, revoke the token in BotFather. `url` = `https://core.telegram.org/bots/features#botfather`.
  - `email` descriptor test `guide_has_smtp_steps`: 3 steps (`email.guide.1`…`3`): find the SMTP host, port, and TLS mode in your provider's help; use an app password where the provider offers one, not your mailbox password; the postal address is required by anti-spam law and appears in every footer. `url` = `None`.
  - `mock` descriptor tests: broadcast, direct, and OAuth mocks each have exactly one step (`mock.guide.1`, "Any value works; this is a test platform.").
  - Each agent's existing `descriptor_is_valid` test keeps passing (it now covers the guide URL).
- [ ] **Step 2: Run** `cargo test -p postit-core -p postit-agent-telegram -p postit-agent-email -p postit-agent-mock` — expect compile failure (no `guide`).
- [ ] **Step 3: Implement** the type, the field, and the validate rule; fill the three agents; add `guide: ConnectGuide::default()` to every other literal.
- [ ] **Step 4: Run** the same tests (pass), then `cargo xtask openapi` (PowerShell) and confirm `schema.d.ts` has `ConnectGuide` and `AgentDescriptor.guide`.
- [ ] **Step 5: Commit** `feat: carry connection guides in the agent descriptor`.

### Task 2: Account import staging schema and repository

**Files:**
- Modify: `backend/crates/data/migrations/0001_InitialSchema.sql` (after `contact_import_rows`)
- Modify: `backend/crates/core/src/ids.rs`, `lib.rs` (`id_newtype!(AccountImportId, "Identifier of an account import.")`, next to `ContactImportId`)
- Create: `backend/crates/data/src/account_imports.rs`; Modify: `backend/crates/data/src/lib.rs`, `vault.rs`, `retention.rs`
- Modify: `backend/crates/config/src/settings.rs`, `loader.rs` (`retention.account_imports`, default `"30d"`, same as `contact_imports`), `docs/configuration.md`
- Modify: `backend/crates/jobs/src/maintenance.rs` (call the purge and the stale sweep next to the contact ones)
- Test: `backend/crates/data/tests/account_imports.rs`, `tests/schema.rs` (sealed-column check runs as is), `tests/scope_matrix.rs`

Schema (exact):

```sql
CREATE TABLE account_imports (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    platform_id TEXT NOT NULL REFERENCES platforms (id),
    "commit" BOOLEAN NOT NULL,             -- false: check only (the dry run)
    -- `awaiting_decision`: a commit run whose file stopped early, waiting for the owner to
    -- continue (back to `processing`) or cancel (`canceled`, staged rows deleted).
    status TEXT NOT NULL DEFAULT 'processing'
        CHECK (status IN ('processing', 'awaiting_decision', 'completed', 'failed', 'canceled')),
    rows_total INTEGER NOT NULL DEFAULT 0,
    rows_done INTEGER NOT NULL DEFAULT 0,
    -- Why reading stopped before the end of the file, or null when it reached the end. The rows
    -- read whole before the stop are staged.
    stopped TEXT CHECK (stopped IN ('max_rows', 'max_bytes', 'unreadable')),
    -- The last data row staged and the file line it starts on (0 when none), so a user whose
    -- file stopped early can remove those lines and import the rest.
    last_row INTEGER NOT NULL DEFAULT 0,
    last_line INTEGER NOT NULL DEFAULT 0,
    -- One entry per processed row in file order: row and line numbers, status, violation
    -- codes, field name, account IDs. Never a cell value.
    report JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Remote account IDs settled so far, so a dry run reports a repeated account the same way
    -- after a resume. IDs only.
    seen_remote_ids TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by UUID REFERENCES users (id) ON DELETE SET NULL,
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL,
    UNIQUE (owner_id, id)
);
CREATE INDEX account_imports_created_idx ON account_imports (created_at);
CREATE INDEX account_imports_status_idx ON account_imports (updated_at)
    WHERE status IN ('processing', 'awaiting_decision');

-- Rows read from the uploaded file and waiting for the import job. The job deletes a chunk's
-- rows in the same transaction that records their report entries; a failed or canceled
-- import's rows are deleted when it is failed or canceled.
CREATE TABLE account_import_rows (
    id UUID PRIMARY KEY,
    import_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    row_no INTEGER NOT NULL,
    line_no INTEGER NOT NULL,
    data BYTEA NOT NULL, -- postit:sealed: the row's cells and display name
    UNIQUE (import_id, row_no),
    FOREIGN KEY (owner_id, import_id) REFERENCES account_imports (owner_id, id) ON DELETE CASCADE
);
```

(`commit` is a reserved word in Postgres, hence the quotes; if `sqlx` macros make that awkward, name the column `is_commit` and keep `commit` in Rust and on the wire.)

**Interfaces:**
- Produces in `postit_data::account_imports`:
  - `pub struct AccountImport { id: AccountImportId, platform: PlatformId, commit: bool, status: ImportState, rows_total: i32, rows_done: i32, stopped: Option<ImportStop>, last_row: i32, last_line: i32, report: Value, created_at, updated_at }`; `pub enum ImportState { Processing, AwaitingDecision, Completed, Failed, Canceled }`; `pub enum ImportStop { MaxRows, MaxBytes, Unreadable }` (snake_case on the wire).
  - `pub struct StagedAccountRow { pub id: Uuid, pub row_no: i32, pub line_no: i32, pub data: Sealed<StagedCells> }`; `#[derive(Serialize, Deserialize)] pub enum StagedCells { Cells { cells: Vec<(String, String)>, display_name: Option<String> }, Malformed }` (names and raw cells in header order; `Malformed` is a record the CSV reader could not parse, reported as `invalid_fields` / `malformed_row`).
  - `AccountImportRepo::create(conn, scope, id, platform, commit) -> Result<(), DataError>` (requires `OwnerOnly`)
  - `::stage(conn, scope, id, rows: &[StagedAccountRow]) -> Result<(), DataError>` (touches `updated_at`)
  - `::set_total(conn, scope, id, total: i32, stopped: Option<ImportStop>, last_row: i32, last_line: i32, status: ImportState)` (`Processing` or `AwaitingDecision`)
  - `::resume(conn, scope, id) -> Result<bool, DataError>` — `awaiting_decision` → `processing`, touches `updated_at`; `false` when the import is in any other state (requires `OwnerOnly`)
  - `::cancel(conn, scope, id) -> Result<bool, DataError>` — `awaiting_decision` → `canceled` and its rows deleted, in one statement; `false` in any other state (requires `OwnerOnly`)
  - `::next_chunk(conn, owner, id, limit: i64) -> Result<Vec<StagedAccountRow>, DataError>` — a plain `SELECT`, lowest `row_no` first; **it deletes nothing**, so a crash while the chunk's rows are being checked with the platform loses no row.
  - `::settle_chunk(conn, owner, id, through_row_no: i32, entries: &[Value], seen: &[String]) -> Result<(), DataError>` — in the caller's transaction: deletes the import's rows with `row_no <= through_row_no`, appends `entries` to `report` and `seen` to `seen_remote_ids`, adds `entries.len()` to `rows_done`, touches `updated_at`.
  - `::seen(conn, owner, id) -> Result<Vec<String>, DataError>`; `::finish(conn, owner, id)` (`completed`); `::abandon(conn, owner, id)` (`failed`, rows deleted, in one statement as `ContactImportRepo::abandon`)
  - `::get(conn, scope, id) -> Result<AccountImport, DataError>` (owner-only; `NotFound` for another owner's)
  - `AccountImportRepo::seal_context(row: Uuid) -> SealContext { table: "account_import_rows", column: "data", row }`
  - `retention::purge_account_imports(conn, cutoff) -> Result<u64, DataError>` (finished imports only: `completed`, `failed`, `canceled`); `retention::fail_stale_account_imports(conn, cutoff) -> Result<u64, DataError>` (an import `processing` or `awaiting_decision` with 24 hours without a change becomes `failed`, its rows deleted in the same statement, like `fail_stale_imports`; the maintenance job passes the same `STALE_IMPORT_HOURS` cutoff as for contacts).

- [ ] **Step 1: Write failing tests** in `data/tests/account_imports.rs`:
  - `stage_chunk_settle_finish_round_trip`: 3 rows staged; `next_chunk(2)` returns rows 1–2 and leaves all 3 staged; `settle_chunk(through 2)` deletes rows 1–2, raises `rows_done` to 2, and appends both entries; `finish` sets `completed`.
  - `unsettled_chunk_survives_a_crash`: `next_chunk(2)` in a transaction that is rolled back (or no transaction at all) leaves every row staged; a second `next_chunk` returns the same rows.
  - `rows_are_sealed_and_open_only_for_their_row`: bytes do not contain a staged cell's text; opening with another row's context fails.
  - `set_total_records_the_stop`: `stopped`, `last_row`, `last_line`, and `awaiting_decision` round-trip through `get`.
  - `resume_and_cancel_only_from_awaiting_decision`: `resume` moves a waiting import to `processing` and keeps its rows; `cancel` moves another to `canceled` with no rows; both return `false` on a `processing`, `completed`, or `canceled` import and change nothing.
  - `get_refuses_another_owner` (`NotFound`); `delegate_cannot_create_resume_or_cancel` (`OwnerOnly` refusal for every delegation level).
  - `stale_import_fails_and_loses_rows`: a `processing` import and an `awaiting_decision` import with `updated_at` 25 hours ago are both `failed` with no rows; fresh ones untouched.
  - `purge_deletes_finished_imports_only` (`completed`, `failed`, `canceled` past the cutoff; never `processing` or `awaiting_decision`).
  - Add the new tables to `scope_matrix.rs` per its pattern.
- [ ] **Step 2: Run** `cargo test -p postit-data --test account_imports -- --test-threads=4` — fails (no module).
- [ ] **Step 3: Implement** the ID, schema, repo, `SEALED_COLUMNS` entry (`account_import_rows.data`, `id`), retention functions, maintenance calls, the setting with docs; run `cargo sqlx prepare`.
- [ ] **Step 4: Run** `cargo test -p postit-data --test account_imports --test schema --test scope_matrix -- --test-threads=4` — pass; `cargo test -p postit-config` — pass.
- [ ] **Step 5: Commit** `feat: stage account imports in sealed rows`.

### Task 3: Account import as a background job

Picks up `docs/plans/2026-10-23-b3-credential-connection-deferred.md`, "Import progress for large files".

**Files:**
- Modify: `backend/crates/oauth/src/import.rs`, `jobs.rs`, `service.rs` (`AccountServiceDeps.queue: JobQueue`), `error.rs` (`AccountError::ImportNotWaiting`), `testkit.rs`, `lib.rs`
- Modify: `backend/crates/server/src/compose.rs` (`account_service` takes the jobs queue; register `AccountImportJob` and its console spec)
- Modify: `backend/crates/config/src/settings.rs` (`accounts.csv_timeout` doc comment: it now bounds the upload and staging only, not the platform checks), `docs/configuration.md` (same)
- Test: `backend/crates/oauth/tests/import.rs`, `tests/leak.rs`

**Interfaces:**
- Consumes: Task 2 repo.
- Produces:
  - `#[derive(Serialize, Deserialize)] pub struct AccountImportJob { pub import: Uuid, pub owner: Uuid }`, `JOB_TYPE = "account_import"`, `QUEUE = Queue::Default` (no accounts queue exists; `Maintenance` is for housekeeping); retry policy `IMPORT_RETRY` as in contacts (5 attempts, 30 s to 10 min).
  - `ImportRow` gains `line: u32` (the file line the record starts on, from `StringRecord::position()`); `ImportStatus` is unchanged. Both are the report entries.
  - `AccountService::start_import<R: AsyncRead + Unpin + Send>(&self, scope, platform: &PlatformId, body: R, commit: bool, audit: AuditContext) -> Result<AccountImport, AccountError>`:
    1. Owner-only; resolves the platform; refuses OAuth platforms (`ConnectMethodUnsupported`).
    2. Reads and checks the header with the existing `read_headers`. A bad or unreadable header returns `CsvInvalid` **before any import exists**.
    3. Creates the import, then reads records one at a time and stages them in chunks of 100, each row sealed with its own context. A record the CSV reader rejects (not an I/O error) is staged as `StagedCells::Malformed`, so it is reported in its place and never stops the file.
    4. Reading stops, keeping every row already read whole, when: the next record would exceed `accounts.csv_max_rows` (`stopped = max_rows`); the body reader fails with `io::ErrorKind::FileTooLarge` (`max_bytes`; the route's `LimitedReader` raises that kind); or it fails with any other I/O error (`unreadable`). A record cut off by the stop is not staged.
    5. In one transaction: stages the last partial chunk and calls `set_total` (rows staged, `stopped`, `last_row`, `last_line`). Then:
       - With no stop, or a dry run that stopped: status `processing`, and the job is enqueued in the same transaction (as `ContactService::stage`).
       - A commit run that stopped: status `awaiting_decision`, and nothing is enqueued, so no account is touched until the owner chooses.
       The upload succeeds either way.
    6. A database failure after the import exists abandons it (`failed`) and returns the error.
  - `AccountService::get_import(&self, scope, id) -> Result<AccountImport, AccountError>`.
  - `AccountService::continue_import(&self, scope, id) -> Result<AccountImport, AccountError>`: owner-only. In one transaction, `resume` and enqueue the job. `AccountError::ImportNotWaiting` when `resume` returns `false`; `NotFound` for an unknown or another owner's import.
  - `AccountService::cancel_import(&self, scope, id, audit) -> Result<AccountImport, AccountError>`: owner-only. `cancel` (staged rows deleted), then one `accounts_imported` audit event with `outcome: "canceled"` and the staged row count. `ImportNotWaiting` when `cancel` returns `false`.
  - `AccountService::run_import(&self, job: AccountImportJob) -> Result<(), JobError>`. A run for an import that is gone or not `processing` changes nothing; this includes one still `awaiting_decision`, though none is ever queued in that state. Otherwise, while rows remain:
    1. `next_chunk(csv_concurrency * 4)` (no row deleted).
    2. Open each row and run the existing `check_row` concurrently (`buffered(csv_concurrency)`), holding no database connection or transaction across a platform call.
    3. `settle_row` in file order, with `seen` loaded from `seen_remote_ids`; with `commit`, each row connects in its own transaction as today.
    4. One transaction: `settle_chunk` through the chunk's last `row_no` with the report entries and the newly seen IDs.
    5. Then `finish` and `record_import` (one `accounts_imported` audit event with `outcome: "completed"`, the counts, and `stopped`, as today).
    A crash between a row's connect transaction and its chunk's `settle_chunk` leaves the chunk staged; on resume the row is checked again, classified `already_connected`, and its connect updates the same account (no duplicate), so every row is reported once and connected at most once.
  - The synchronous `import_csv` and `ImportReport` are removed.

- [ ] **Step 1: Write failing tests** in `oauth/tests/import.rs` (rewrite the existing ones onto the new API; keep every case they cover):
  - `start_import_stages_and_queues`: a 3-row CSV gives an import `processing`, `rows_total == 3`, `last_row == 3`, `last_line == 4`, one queued `account_import` ledger row, and no account yet.
  - `run_import_dry_run_reports_without_connecting` (statuses as the old dry-run test, zero accounts); `run_import_connects_valid_rows` (as the old commit test).
  - `report_lines_follow_the_file`: a quoted cell spanning two lines shifts the next row's `line` by one.
  - `malformed_row_is_reported_and_skipped`: a row with the wrong column count is `invalid_fields` / `malformed_row` in place; the rows after it are processed.
  - `repeated_remote_account_reports_already_connected_across_chunks`: rows 1 and 9 reach the same account with the chunk size forced to 4; row 9 is `already_connected` in a dry run.
  - `crash_mid_chunk_resumes_without_loss_or_double_connect`: with commit, process a chunk's rows but stop before `settle_chunk` (drop the service), then run again; every row is reported exactly once, each account is connected once, and `rows_done == rows_total`.
  - `bad_header_answers_before_any_import`: `CsvInvalid`, zero `account_imports` rows.
  - `over_max_rows_stops_with_max_rows`: rows up to the limit are staged; `stopped = max_rows`; `last_row` equals the limit.
  - `body_over_byte_limit_stops_with_max_bytes`: a reader that fails with `FileTooLarge` after 2 whole rows and half of a third gives `rows_total == 2`, `stopped = max_bytes`, and `last_line` of row 2.
  - `unreadable_body_stops_with_unreadable` (another I/O error kind, same shape).
  - `stopped_dry_run_runs_at_once`: a dry run that stopped is `processing` with a queued job, and its report covers the rows read.
  - `stopped_commit_waits_and_connects_nothing`: a commit run that stopped is `awaiting_decision` with no queued job and no account; `run_import` on a forged job for it changes nothing.
  - `continue_connects_the_rows_read`: `continue_import` queues one job, and running it connects the staged rows only.
  - `cancel_deletes_staged_rows`: `cancel_import` leaves `canceled`, zero `account_import_rows`, no account, and one audit event with `outcome: "canceled"`.
  - `continue_or_cancel_refused_unless_waiting`: on a `processing`, `completed`, or `canceled` import both return `ImportNotWaiting`; a delegate at every level gets `OwnerOnly`.
  - `delegate_refused` (every level, `OwnerOnly`); `oauth_platform_refused`.
  - In `tests/leak.rs`: run an import whose cells hold a known token and assert the token is absent from logs, the report JSON, the audit row, the ledger payload, and the raw `account_import_rows.data` bytes.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test import --test leak` — fail.
- [ ] **Step 3: Implement** as in Interfaces; wire the queue and registration in `compose.rs`/`lib.rs`; console spec with `id_field("import", "import_id")` and `id_field("owner", "owner_id")`.
- [ ] **Step 4: Run** the same tests — pass; `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- [ ] **Step 5: Commit** `feat: run account CSV imports as a background job`.

### Task 4: Import routes

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs`, `dto_accounts.rs`, `routes/mod.rs`, `openapi.rs`, `error.rs`, `src/testkit.rs`
- Modify: `web/src/i18n/en-US/common.json`, `mobile/lib/l10n/app_en.arb`, `mobile/lib/ui/errors/messages.dart` (the new error code)
- Test: `backend/crates/api/tests/accounts.rs`, `src/error.rs` code table

**Interfaces:**
- Produces:
  - `POST /api/v1/accounts/import/{platform}?commit=…` answers `202` with `AccountImportDto` and `Location: /api/v1/accounts/imports/{id}`. The content-type check, the `credential_checks` charge (once per upload), the route's body limit and `accounts.csv_timeout` stay. `LimitedReader` now fails with `io::ErrorKind::FileTooLarge`. The answers:
    - A declared `Content-Length` over `accounts.csv_max_bytes` is refused by the layer (`413 payload_too_large`).
    - A header that cannot be read because the limit was hit (the `too_large` flag is set and the service returned `CsvInvalid`) answers `413 payload_too_large`.
    - Any other header failure answers `422 csv_invalid`.
    - A body that goes over the limit after the header answers `202`, with `stopped: "max_bytes"` (status `processing` for a dry run, `awaiting_decision` for a commit run).
  - `GET /api/v1/accounts/imports/{id}` (owner) answers `AccountImportDto { id, platform, commit: bool, status: "processing" | "awaiting_decision" | "completed" | "failed" | "canceled", rows_total, rows_done, stopped: null | "max_rows" | "max_bytes" | "unreadable", last_row, last_line, counts: { <ImportStatus>: n }, rows: ImportRowDto[] }` (`counts` computed from `rows`; `ImportRowDto { row, line, status, violations, field, account_ids, display_name_ignored }`). `operation_id = "get_account_import"`.
  - `POST /api/v1/accounts/imports/{id}/continue` (owner) answers `202` with `AccountImportDto` (now `processing`); `operation_id = "continue_account_import"`. `DELETE /api/v1/accounts/imports/{id}` (owner) answers `200` with `AccountImportDto` (now `canceled`); `operation_id = "cancel_account_import"`. Both answer `409 import_not_waiting` unless the import is `awaiting_decision`, `404` for an unknown or another owner's import, and `403 owner_only` for a delegate. Neither charges `credential_checks` (the upload already did).
  - `ErrorCode::ImportNotWaiting` (`import_not_waiting`, 409, title "Import not waiting for a decision") with its entries in both clients' error catalogs, in this task, so the regenerated contract never leaves a catalog incomplete: `web/src/i18n/en-US/common.json` ("This import is no longer waiting for your choice. Reload to see where it stands.") and `mobile/lib/l10n/app_en.arb` (`error_import_not_waiting`, with its `@` description) mapped in `mobile/lib/ui/errors/messages.dart`, then `flutter gen-l10n` per `mobile/CLAUDE.md`.
  - `TestApp::run_account_imports() -> usize`: reads the queued `account_import` ledger rows and runs each through `state.accounts.run_import`, as `contacts_leak.rs` drives `run_import` (no generic queue drainer).
- [ ] **Step 1: Write failing tests:** `import_answers_202_with_location`; `get_import_shows_progress_then_completion` (GET before `run_account_imports` shows `processing` and `rows_done == 0`, after it `completed` with the report); `oversized_dry_run_answers_202_stopped_max_bytes` (`processing`); `oversized_commit_waits_then_continue_connects` (`awaiting_decision`, no account; `continue` → `202 processing`; after `run_account_imports` the rows read are connected); `cancel_deletes_staged_rows` (`200 canceled`, no account, zero staged rows); `continue_and_cancel_refuse_unless_waiting` (`409 import_not_waiting` on `processing`, `completed`, `canceled`); `continue_and_cancel_isolation` (delegate `403 owner_only`, unrelated user `404`); `oversized_content_length_answers_413` (no `account_imports` row); `bad_header_answers_422_csv_invalid` (no `account_imports` row); `import_isolation_matrix` (owner 202; every delegate level `403 owner_only` on both routes; unrelated user `404` on GET); `get_import_never_shows_cells`; existing `import_*` route tests updated to the new shape.
- [ ] **Step 2: Run** `cargo test -p postit-api --test accounts` — fail.
- [ ] **Step 3: Implement**; regenerate with `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests, `cargo xtask openapi --check`, the web catalog tests (`pnpm exec vitest run src/i18n src/api`, PowerShell), and the mobile error-message tests (`flutter test` per `mobile/CLAUDE.md`) — pass. In `docs/plans/2026-10-23-b3-credential-connection-deferred.md`, mark "Import progress for large files" **resolved in D4a (Tasks 2–4)**; the Dart entry stays open, so the file keeps its name (its text still names the right generated method).
- [ ] **Step 5: Commit** `feat: answer account imports with an import ID and a progress route`.

### Task 5: One notification by ID

A Web Push click opens `/?notification=<id>` (`web/public/sw.js`), and the web app needs the notification's kind, owner, and payload to follow it.

**Files:**
- Modify: `backend/crates/api/src/routes/notifications.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/notifications.rs`

**Interfaces:**
- Produces: `GET /api/v1/notifications/{id}` (self: the `ActiveUser` extractor, so it refuses `X-Postit-Act-As` with `owner_only`), `operation_id = "get_notification"`, answers `NotificationDto` through `NotificationRepo::get_for_user`; `404 not_found` for an unknown ID or another user's. Documented like its neighbors (401, 403, 404, 422 for a malformed ID, 429, 500, 503).
- [ ] **Step 1: Write failing tests:** `get_notification_returns_own`, `get_notification_hides_another_users` (404), `get_notification_refuses_act_as` (403 `owner_only`).
- [ ] **Step 2: Run** `cargo test -p postit-api --test notifications` — fail.
- [ ] **Step 3: Implement**; `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests and `cargo xtask openapi --check` — pass.
- [ ] **Step 5: Commit** `feat: read one notification by ID`.

### Task 6: Workspace store, act-as header, cache isolation

**Files:**
- Create: `web/src/workspace/store.ts`, `web/src/workspace/keys.ts`, `web/src/workspace/WorkspaceProvider.tsx`, `web/src/api/actAs.ts`, `web/src/workspace/queryKeyGuard.test.ts`
- Modify: `web/src/api/authFetch.ts`, `web/src/api/client.ts`, `web/src/api/queryClient.ts`, `web/src/routes.tsx` (a `WorkspaceProvider` layout route between `Gate` and `Shell`), `web/src/components/Shell.tsx` (the routed page renders as `<Outlet key={workspaceKey} />`), `web/src/test/render.tsx` (render inside `WorkspaceProvider`, option `actAs?: string`). Every existing query key is self or admin data (`me`, `preferences`, `push`, `users`, `audit`, `jobs`), so no existing hook moves onto `wsKey`
- Test: `web/src/api/actAs.test.ts`, `web/src/api/authFetch.test.ts`, `web/src/workspace/WorkspaceProvider.test.tsx`

**Interfaces:**
- Produces:
  - `store.ts`: `ACT_AS_KEY = 'postit.actAs'`; `getActAs(): string | null`; `setActAs(owner: string | null): void`; `subscribeActAs(fn: () => void): () => void` (sessionStorage, wrapped in try/catch; in-memory fallback).
  - `actAs.ts`: `needsActAs(url: string): boolean`. It parses `new URL(url).pathname` (`authFetch` receives absolute URLs), returns false outside `/api/v1/`, and splits the rest into segments. False when the first segment is exactly one of `me`, `notifications`, `push`, `users` (the admin user routes and `users/lookup`), `admin`, `auth`, `platforms`; when the first two segments are `delegations` `received`; when they are `ai` `presets`. True otherwise. Whole segments only: `/api/v1/media` is true, `/api/v1/meta` would be true, `/api/v1/me` is false.
  - `createAuthFetch(source, baseFetch?, actAs: () => string | null = getActAs)` sets `X-Postit-Act-As` when `actAs()` is non-null and `needsActAs(request.url)`.
  - `keys.ts`: `SELF_KEYS = new Set(['me', 'workspaces', 'preferences', 'push', 'notifications', 'platforms', 'platformIcon', 'users', 'audit', 'jobs', 'admin'])`. These are the self and admin keys already in the tree (`me`, `preferences`, `push`, `users`, `audit`, `jobs`) plus this slice's. Extend it only for data that is the same in every workspace; `wsKey(owner: string | null, ...parts: unknown[]): readonly unknown[]` → `['ws', owner ?? 'own', ...parts]`; `useWsKey()` → `(...parts) => wsKey(actAs, ...parts)`. Every workspace-scoped query in later tasks uses it.
  - `WorkspaceProvider` (a layout route rendering `<Outlet />`, so it can navigate): context `{ actAs: string | null; workspaceKey: string; workspaces: WorkspaceDto[]; current: WorkspaceDto | undefined; switchTo(owner: string | null): void; endedNotice: string | null; clearEndedNotice(): void }`, exported as `useWorkspace()`. `workspaces` comes from `GET /api/v1/me/workspaces` (key `['workspaces']`); the own entry has `own: true`. A stored owner that is not in `workspaces` falls back to own.
  - `switchTo(owner)` does four things, in this order:
    1. `queryClient.cancelQueries` and then `removeQueries` for **every query whose key's first element is not in `SELF_KEYS`**, so a hook that forgot `wsKey` still loses its data on a switch.
    2. `setActAs(owner)`.
    3. Bumps `workspaceKey`. Shell keys the routed page on it, so every page, form, and dialog opened in the old workspace unmounts; the sidebar and the switcher stay mounted.
    4. Navigates: it stays on the current path when the matched route has `handle: { keepOnSwitch: true }` (list pages: `/`, `/accounts`, `/accounts/groups`, `/delegations`, `/notifications`, `/profile`), and otherwise goes to `/`, so a detail page from A never reloads as "not found" in B.
  - `createQueryClient({ retry, onDelegationInvalid })`: on any query or mutation error with code `delegation_invalid`, calls `onDelegationInvalid` once per stored owner (the provider switches to own and sets `endedNotice` to `t('common:workspace.ended')`).
  - `queryKeyGuard.test.ts` (static, like `kitGuard`): reads every non-test `.ts`/`.tsx` under `src/features` and `src/workspace` and fails on a `queryKey: [` literal whose first element is not a quoted member of `SELF_KEYS`; workspace keys are written `queryKey: ws(…)` / `wsKey(…)`, never as array literals. A key held in a named constant (as `PREFERENCES_KEY` is today) is not checked statically; the inverted removal in `switchTo` still covers it.
- [ ] **Step 1: Write failing tests:**
  - `actAs.test.ts` table: `/api/v1/accounts` true, `/api/v1/media` true, `/api/v1/media/uploads/x` true, `/api/v1/delegations` true, `/api/v1/target-groups` true, `/api/v1/me` false, `/api/v1/me/workspaces` false, `/api/v1/notifications/unread-count` false, `/api/v1/notifications/x` false, `/api/v1/push/vapid-public-key` false, `/api/v1/users` false, `/api/v1/users/x` false, `/api/v1/users/lookup?email=a` false, `/api/v1/delegations/received/x` false, `/api/v1/admin/platforms` false, `/api/v1/auth/config` false, `/api/v1/platforms/x/icon` false, `/api/v1/ai/presets` false, `/api/v1/ai/providers` true, `/config.json` false; all as absolute URLs on the API origin.
  - `actAs.test.ts` contract check: read `../backend/openapi.json` (relative to `web/`; skip with a clear message if absent). Every operation whose `403` description says "does not accept it" or "`owner_only` (`X-Postit-Act-As` sent)" must give false. Every operation whose `403` lists `delegation_invalid` must give true, except the documented workspace-independent pair (`/platforms…`, `/ai/presets`). Substitute path parameters with `x`.
  - `authFetch.test.ts`: `adds act-as only where needed`, `no header in own workspace`.
  - `WorkspaceProvider.test.tsx`:
    - `switching drops the old workspace's queries and late answers`: a slow MSW `/accounts` answer for A resolves after `switchTo(B)`, and the rendered list shows B's data only.
    - `A to B to A shows A's data`.
    - `switching remounts the page`: a dialog opened in A is gone after the switch.
    - `switching from a detail route goes home; from a list route stays`.
    - `an unscoped query key is dropped on switch too`.
    - `delegation_invalid returns to own workspace once`: two failing queries give one switch, and the notice shows.
    - `unknown stored owner falls back to own`; `act-as survives a remount (sessionStorage)`.
  - `queryKeyGuard.test.ts` (passes on the current tree once existing keys are moved).
- [ ] **Step 2: Run** `pnpm exec vitest run src/api src/workspace` (PowerShell, in `web/`) — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests — pass; run the whole web gate.
- [ ] **Step 5: Commit** `feat(web): act in a delegated workspace per tab`.

### Task 7: Access gating, switcher, banner, navigation

**Files:**
- Create: `web/src/workspace/useAccess.ts`, `WorkspaceSwitcher.tsx`, `WorkspaceBanner.tsx`
- Modify: `web/src/components/Shell.tsx`, `web/src/routes.tsx`, `web/src/i18n/en-US/common.json`
- Test: `web/src/workspace/useAccess.test.tsx`, `web/src/components/Shell.test.tsx`

**Interfaces:**
- Consumes: `useWorkspace()` (Task 6).
- Produces: `useAccess(): { own: boolean; level: 'owner' | 'view' | 'edit' | 'publish'; atLeast(l: 'view' | 'edit' | 'publish'): boolean; inLimit(accountId: string): boolean }` (`own` → `level 'owner'`, `atLeast` always true, `inLimit` always true; delegated → from `current.level`, `all_accounts`, `account_ids`). Also `<RequireOwn>` route element: renders the outlet in the own workspace, else `EmptyState` "Only the workspace owner can do this" with a button to switch to own.
- Nav per the spec's sketch, with only the items this slice builds: Home; Accounts; SETTINGS: Profile and preferences, Delegations (in every workspace; while delegated the page shows only Received, see Task 11); ADMIN: Users, Platforms, Publishing, Audit, Jobs. The switcher sits above the nav (own first, then each received workspace with the owner's name and level; a limited grant shows "{n} accounts"). The banner sits under the header while delegated: "You are working in {name}'s workspace ({level})" with "Back to my workspace". The bell slot is in the header (filled by Task 10).
- [ ] **Step 1: Write failing tests:** `useAccess` for own, view, edit, publish, limited publish (`inLimit` true only for listed IDs); Shell: `switcher lists own and received workspaces`, `banner shows while delegated and its button returns to own`, `Delegations shows in every workspace`, `admin section only for admins`, both directions (`dir="rtl"` render keeps the switcher at the start edge: assert logical classes `ms-`/`me-`, no `ml-`/`mr-`).
- [ ] **Step 2: Run** `pnpm exec vitest run src/workspace src/components/Shell.test.tsx` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): gate actions by workspace access and add the switcher`.

### Task 8: Kit additions: SchemaForm, PlatformBadge, AccountPicker, ProgressBar

**Files:**
- Create: `web/src/components/ui/popover.tsx`, `tabs.tsx`, `textarea.tsx` (`pnpm dlx shadcn add popover tabs textarea`; `components.json` already sets the `radix-nova` style and `rtl: true`, so check the output for any physical `ml-`/`mr-`/`left-`/`right-` class and change it to its logical form)
- Create: `web/src/components/kit/schema.ts`, `SchemaForm.tsx`, `PlatformBadge.tsx`, `AccountPicker.tsx`, `ProgressBar.tsx`
- Create: `web/src/i18n/agentText.ts`, `web/src/i18n/en-US/agents.json` (`{}`); register namespace `agents` in `web/src/i18n/index.ts` and `i18next.d.ts`
- Create: `web/src/features/platforms/queries.ts`
- Test: `web/src/components/kit/SchemaForm.test.tsx`, `kit.test.tsx` (extend), `web/src/features/platforms/queries.test.tsx`

**Interfaces:**
- Produces:
  - `agentText(m: { key: string; en: string }): string` — `i18next.exists('agents:' + key) ? t(...) : m.en`.
  - `schema.ts`: `type FieldKind = { type: 'string'; minLength?: number; maxLength?: number } | { type: 'multiline'; maxLength?: number } | { type: 'enum'; values: { value: string; label: AgentMessage }[] } | { type: 'boolean' } | { type: 'integer'; minimum?: number; maximum?: number } | { type: 'datetime' }`; `interface SchemaFieldSpec { name; label: AgentMessage; help?: AgentMessage; required: boolean; kind: FieldKind; default?: unknown; noDefault: boolean; secret: boolean; consentText?: AgentMessage; dependsOn?: { field: string; equals: unknown } }`; `parseSchema(schema: unknown): SchemaFieldSpec[]` (declaration order; a field with an unknown keyword is skipped with `console.warn` in development only); `initialValues(fields): Record<string, unknown>` (defaults, none for `noDefault` or secret); `submittedValues(fields, values): Record<string, unknown>` (drops fields whose `dependsOn` does not hold, and empty optional strings).
  - `SchemaForm({ fields, values, onChange, errors, disabled }: { fields: SchemaFieldSpec[]; values: Record<string, unknown>; onChange(next: Record<string, unknown>): void; errors?: Record<string, ReactNode>; disabled?: boolean })` — string → `Input`; multiline → `Textarea`; enum → `OptionSelect`; boolean → `Switch`; consent → `Checkbox` with the consent text as its label, required true; integer → `Input type="number"` with min/max; datetime → `Input type="datetime-local"` shown in the user's zone, value kept as RFC 3339 with offset; secret → password `Input` with a show/hide button, `autoComplete="off"`, never prefilled; help under the input; required marked.
  - `usePlatforms()` → `GET /api/v1/platforms`, `staleTime: 5 min`, key `['platforms']` (in `SELF_KEYS`; sent without act-as); `usePlatform(id)`; `usePlatformIcon(id): string | undefined` — key `['platformIcon', id]`, fetches the PNG through the API client (the route needs the bearer), returns an object URL, `staleTime: Infinity`, revokes on cache removal.
  - `PlatformBadge({ platform, size = 'md', showName = true })` — icon (or the first letter on the brand color while loading or when missing) and display name.
  - `AccountPicker({ value: string[]; onChange(ids: string[]): void; accounts: AccountDto[] })` — checkbox list grouped by platform with `PlatformBadge`; callers pass only accounts in the caller's limit.
  - `ProgressBar({ value, max, label })` — `role="progressbar"` with `aria-valuenow`/`aria-valuemax`, logical fill direction.
- [ ] **Step 1: Write failing tests:** `SchemaForm.test.tsx`, one test per widget (`renders string with limits`, `multiline`, `enum with labels`, `boolean`, `consent must be checked`, `integer bounds`, `datetime round trips in the user's zone`, `secret is masked, toggles, and is never prefilled`, `help text`, `noDefault starts empty and counts as required`, `depends-on hides and drops the field`, `unknown keyword skips the field`), each also rendered under `dir="rtl"`; `agentText falls back to en`; `usePlatformIcon returns an object URL and sends the bearer`; `PlatformBadge shows the initial while the icon loads`; `ProgressBar exposes aria values`; `AccountPicker groups by platform` (both directions).
- [ ] **Step 2: Run** `pnpm exec vitest run src/components/kit src/features/platforms src/i18n` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass; `kitGuard.test.ts` still passes.
- [ ] **Step 5: Commit** `feat(web): add the schema form and platform kit components`.

### Task 9: Preferences and the startup error

**Files:**
- Modify: `web/src/features/me/ProfilePage.tsx` (title "Profile and preferences"), add `web/src/features/me/TimeZoneSetting.tsx`; Modify: `web/src/config/runtime.ts`, `web/src/main.tsx`, `web/src/StartupError.tsx`, `web/src/i18n/en-US/{me,common}.json`
- Test: `web/src/features/me/ProfilePage.test.tsx`, `web/src/StartupError.test.tsx`, `web/src/config/runtime.test.ts`

**Interfaces:**
- Produces: `TimeZoneSetting` — an `OptionSelect` over `Intl.supportedValuesOf('timeZone')`, saving through `useUpdatePreferences({ timezone })`; when the saved zone differs from the browser's (`Intl.DateTimeFormat().resolvedOptions().timeZone`) and the `localStorage` flag `postit.tzAsked` is unset, a one-time prompt on the profile page offers the browser's zone; either answer sets the flag (a per-browser convenience, not a secret). The language row stays behind `MULTILINGUAL`. `NotificationSettings` stays as is.
- `runtime.ts` throws `class StartupFailure extends Error { reason: 'runtime_config' | 'auth_config' | 'identity_unreachable' }` instead of English messages; `StartupError({ reason, onRetry })` renders `t('startup.reason.<reason>')` (plus a generic fallback); `main.tsx` passes the reason. Keys in `common.json`: `startup.reason.runtime_config` ("The app's settings could not be loaded."), `startup.reason.auth_config` ("The sign-in settings could not be loaded from the server."), `startup.reason.identity_unreachable` ("The sign-in service cannot be reached."), `startup.reason.unknown`.
- [ ] **Step 1: Write failing tests:** `time zone saves the chosen zone`, `prompt offers the browser zone once`, `startup error shows the translated reason`, `runtime config failure carries runtime_config`, `auth config failure carries auth_config`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): time zone preference and a translated startup error`.

### Task 10: Notifications bell, list, and deep links

**Files:**
- Create: `web/src/features/notifications/queries.ts`, `NotificationBell.tsx`, `NotificationsPage.tsx`, `links.ts`, `useOpenNotification.ts`, `web/src/i18n/en-US/notifications.json` (registered in `index.ts` and `i18next.d.ts`)
- Modify: `web/src/components/Shell.tsx` (bell in the header; replace the `?notification` stub with the shared opener), `web/src/routes.tsx` (`/notifications`, `keepOnSwitch`)
- Test: `web/src/features/notifications/*.test.tsx`, `links.test.ts`

**Interfaces:**
- Consumes: Task 5's `GET /notifications/{id}`; `switchTo` (Task 6).
- Produces:
  - `useUnreadCount()` — `GET /api/v1/notifications/unread-count`, `refetchInterval: 60_000` while visible (`refetchIntervalInBackground: false`), `refetchOnWindowFocus: true`; key `['notifications', 'unread']` (self).
  - `useNotifications(page, pageSize)` (`GET /notifications`; the route has paging only, no unread filter, so the page shows read state per row instead), `useNotification(id)` (`GET /notifications/{id}`, key `['notifications', 'one', id]`), `useMarkRead()` (`POST /notifications/read` with `{ ids }` or `{ all: true }`; invalidates every `['notifications']` key).
  - `linkFor(n: NotificationDto, me: string): { path: string; scope: 'owner' | 'self' } | null`:
    - `account_needs_reconnect`, `account_revoked_by_platform`, `reconnect_by_passed`, `reconnect_reminder` → `{ path: '/accounts/{account_id}', scope: 'owner' }`.
    - `delegation_changed` → `{ path: '/delegations?tab=granted', scope: 'self' }` when `owner_id === me` (a delegate changed or left one of my grants), else `{ path: '/delegations?tab=received', scope: 'self' }`.
    - `user_pending` → `{ path: '/users', scope: 'self' }`.
    - Delivery, job, recurrence, and comment kinds → `null` in D4a (D4b and later fill them).
    - A notice with no `owner_id` is always `self`.
  - `useOpenNotification()` → `open(n: NotificationDto)`: marks it read; when the link's scope is `owner` and `n.owner_id` differs from the active workspace (own counts as `me`), calls `switchTo(owner_id === me ? null : owner_id)` first; a `self` link navigates in the current workspace without switching. Then it navigates to the path, or stays on the list for `null`.
  - The Shell reads `?notification=<id>`, removes it from the URL (`replace`), loads it with `useNotification(id)`, and calls `open`. A `404` sends the user to `/notifications`.
  - Bell: badge with the count (hidden at 0, "99+" above 99), a `Popover` with the newest 10 (`page_size=10`), "Mark all read", "See all". Page: `DataTable` with server paging via `useListParams`, read state, the kind label from the catalog (`notifications:kind.<kind>`, every `NotificationKind` present; a completeness test like `violations.test.ts`), severity.
- [ ] **Step 1: Write failing tests:** `bell shows unread badge and polls`, `opening an account notice from another workspace switches first`, `opening delegation_changed never switches workspace` (as a delegate, with the owner's `owner_id`), `linkFor covers every kind` (iterate the generated enum; `null` allowed only for the listed kinds), `mark all read clears the badge`, `push click ?notification=id loads and opens it`, `push click for an unknown id lands on the list`, catalog completeness for kinds.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): notifications bell and list`.

### Task 11: Delegations

**Files:**
- Create: `web/src/features/delegations/queries.ts`, `DelegationsPage.tsx`, `GrantDialog.tsx`, `web/src/i18n/en-US/delegations.json` (registered in `index.ts` and `i18next.d.ts`)
- Modify: `web/src/routes.tsx` (`/delegations`, `keepOnSwitch`; the tab is in the URL as `?tab=granted|received`)
- Test: `web/src/features/delegations/*.test.tsx`

**Interfaces:**
- Consumes: `AccountPicker`, `useAccess`, `wsKey`.
- Produces: one page, two tabs.
  - **Granted** (own workspace only; while delegated the tab is not shown and `?tab=granted` falls back to Received): `GET/POST /delegations`, `PATCH/DELETE /delegations/{id}`. A table of delegate, level, accounts (all or count), expiry, status.
    - Grant dialog: email (exact match through `GET /users/lookup?email=`, showing the found name or "No postit user has this email"), level (`view`, `edit`, `publish` with one-line explanations), all accounts or chosen (`AccountPicker`), optional expiry date.
    - Edit opens the same dialog prefilled; the PATCH sends every field, `expires_at` included, so an unchanged expiry is never cleared.
    - Revoke sits in a `DangerZone` at the bottom of the edit dialog (the kit's section for irreversible actions), behind a `ConfirmDialog`.
  - **Received** (any workspace): `GET /delegations/received`, `DELETE /delegations/received/{id}`. Owner, level, accounts, expiry, "Open workspace" (`switchTo`), and "Leave" with confirmation; leaving the active workspace switches to own.
- [ ] **Step 1: Write failing tests:** `grant by exact email`, `lookup miss shows the message and blocks submit`, `edit keeps expires_at`, `revoke asks first`, `leave the active workspace returns to own`, `while delegated only Received shows`, `tab=granted while delegated falls back to Received`, `rate_limited lookup shows the wait`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): grant, edit, revoke, and leave delegations`.

### Task 12: Accounts list and account detail

**Files:**
- Create: `web/src/features/accounts/queries.ts`, `AccountsPage.tsx`, `AccountDetailPage.tsx`, `WritingStyleForm.tsx`, `PendingConnections.tsx`, `web/src/i18n/en-US/accounts.json` (registered in `index.ts` and `i18next.d.ts`)
- Modify: `web/src/routes.tsx` (`/accounts` with `keepOnSwitch`, `/accounts/:id`)
- Test: `web/src/features/accounts/AccountsPage.test.tsx`, `AccountDetailPage.test.tsx`

**Interfaces:**
- Consumes: `useAccess`, `PlatformBadge`, `wsKey`, `usePlatforms`.
- Produces:
  - `useAccounts({ platform?, includeDisconnected?, page, pageSize })`, `useAccount(id)`, `useAccountActions()` with `rename`, `setTestAccount`, `check`, `disconnect` (owner-only routes), `useWritingStyle(id)`, `useSaveWritingStyle(id)`, `usePendingConnections()`, `useCancelPending()`.
  - Accounts page: `PageHeader` with "Connect an account" and "Import from CSV" (owner only); rows grouped by platform (`PlatformBadge`), status badge (`active`, `needs_reconnect`, `revoked`, `disconnected`), test flag, last checked; "Show disconnected" filter in the URL. Unfinished connections (owner only) from `GET /accounts/pending-connections`: "Unfinished connection to {platform}, expires in {n} min" with **Cancel** (`DELETE /accounts/pending-connections/{pending_id}`) and **Start again** (opens the connect flow for that platform); never Confirm.
  - Detail page:
    - `FactList` (platform, remote ID, status, connected, last checked).
    - Rename and the test flag (owner); "Check now" (owner; shows the result status).
    - Writing style form, fields `tone`, `emoji`, `hashtags`, `language`, `notes` (as `WritingStyleDto` defines them): read-only at `view`; editable at `edit` when the account is in the limit.
    - Disconnect in `DangerZone` (owner), with a `ConfirmDialog` saying scheduled deliveries to this account are canceled and history is kept.
    - An account outside the limit answers `404` → "Account not found".
- [ ] **Step 1: Write failing tests** at each access level (owner, view, edit, publish, limited): `owner sees connect and import`, `delegate sees no owner actions`, `pending connection shows cancel and start again, never confirm`, `writing style read-only for view`, `writing style editable for edit in limit`, `disconnect asks first and returns to the list`, `check now shows the new status`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): accounts list and account detail`.

### Task 13: Connecting accounts: credentials, OAuth, discovery checklist

**Files:**
- Create: `web/src/features/accounts/connect/ConnectPage.tsx`, `CredentialsStep.tsx`, `DiscoveryChecklist.tsx`, `OAuthDonePage.tsx`, `pendingStore.ts`
- Modify: `web/src/routes.tsx` (`/accounts/connect`, `/accounts/connect/:platform` under `RequireOwn`; `/oauth-done` inside the session gate, outside the Shell's own-workspace check: the connection belongs to whoever started it)
- Test: `web/src/features/accounts/connect/*.test.tsx`, `pendingStore.test.ts`

**Interfaces:**
- Consumes: `SchemaForm`, `parseSchema`, `submittedValues`, `PlatformBadge`, `agentText`, `usePlatforms`.
- Produces:
  - `pendingStore.ts`: `PENDING_KEY = 'postit.pendingConnection'`; `savePending(p: { connectId: string; pendingId: string | null; platform: string; userId: string; expiresAt: string }): void`; `loadPending(userId: string, now = Date.now()): PendingEntry | null` (drops and returns null when expired or another user's); `clearPending(): void`. sessionStorage only, try/catch.
  - Connect page: a platform grid from `usePlatforms()` (`PlatformBadge`, kind, connect method).
    - Credentials platform → `CredentialsStep`: the guide steps (`agentText`) and the guide link, `SchemaForm` over the credentials schema, submit `POST /accounts/credentials/{platform}`.
    - A `422 validation_failed` maps each violation's `path` (`options.<field>`) to `errors[field]` through the violation catalog; `credentials_invalid` marks `field`.
    - Success saves the pending entry and shows the checklist.
    - OAuth platform → `POST /accounts/connect/{platform}` with `{ return_to: window.location.origin + '/oauth-done' }`, then `window.location.assign(authorize_url)`. The server's allowlist matches `{app.public_url}/oauth-done` exactly as a string (`backend/crates/oauth/src/return_to.rs`), so this works only where `app.public_url` is the web app's origin with no path (true in every `backend/config/*.toml`; say so in a comment next to the call).
  - `DiscoveryChecklist({ connectId, pendingId, discovered, onDone })`: each account with a checkbox (all checked), avatar and name.
    - **Confirm** → `POST /accounts/connect/{connect_id}/confirm` with `{ account_ids }`, then clears the entry and goes to `/accounts` with a message built from the response's counts (`skipped_elsewhere` shown when non-zero).
    - **Cancel** → `DELETE /accounts/pending-connections/{pending_id}` when `pendingId` is set, then clears.
    - A countdown shows the minutes left; at zero the checklist closes with "This connection expired. Start again."
  - `OAuthDonePage`: reads `result`, `connect_id`, `code`.
    - `result=ok` → `GET /accounts/connect/{connect_id}` (gives `discovered`, `pending_id`, `expires_at`), saves the pending entry, shows the checklist.
    - `result=error` → the translated `accounts:oauthResult.<code>` for every code in the callback's doc (`access_denied`, `provider_error`, `exchange_failed`, `scope_missing`, `discovery_failed`, `no_accounts`, `platform_unavailable`, `state_invalid`, `account_connected_elsewhere`), with "Back to accounts".
    - After reading, the query string is removed with `history.replaceState`, so `connect_id` does not stay in the address bar or history.
  - The Accounts page (Task 12) resumes a valid pending entry on load and shows the checklist.
- [ ] **Step 1: Write failing tests:** `credentials form renders from the schema with guide`, `violations map to fields`, `credentials_invalid marks the field`, `checklist confirms the chosen accounts`, `cancel deletes the pending row`, `credential check with no pending_id cancels locally`, `reload within 10 minutes resumes the checklist`, `expired entry is dropped`, `another user's entry is ignored`, `oauth start sends return_to and navigates`, `oauth-done ok shows the checklist and strips the query`, `oauth-done error codes are translated (every code)`, `connect_id never written to localStorage` (spy on `Storage.prototype.setItem` and assert no `localStorage` key holds it).
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): connect accounts by credentials and OAuth`.

### Task 14: Target groups

**Files:**
- Create: `web/src/features/accounts/TargetGroupsPage.tsx`, `TargetGroupDialog.tsx`; Modify: `queries.ts`, `routes.tsx` (`/accounts/groups`, `keepOnSwitch`), the Accounts page header (a "Target groups" link)
- Test: `web/src/features/accounts/TargetGroupsPage.test.tsx`

**Interfaces:**
- Produces: `useTargetGroups(page)`, `useSaveTargetGroup()` (`POST` or `PUT /target-groups/{id}` with `{ name, account_ids }`), `useDeleteTargetGroup()`. Page: `DataTable` of name and account badges; create and edit (edit level) in a dialog with `AccountPicker` over the accounts in the caller's limit; delete with confirmation. Hidden accounts are not shown and the server keeps them. `TargetGroupDto` (`id`, `name`, `account_ids`, `updated_at`) carries no count of hidden accounts, so the dialog shows no note about them.
- [ ] **Step 1: Write failing tests:** `create a group`, `edit replaces visible accounts`, `view level sees no edit actions`, `limited delegate picks only accounts in limit`, `403 account_not_in_scope is shown`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): target groups`.

### Task 15: Account CSV import screen

**Files:**
- Create: `web/src/features/accounts/import/ImportPage.tsx`, `ImportReport.tsx`, `queries.ts`
- Modify: `web/src/routes.tsx` (`/accounts/import` under `RequireOwn`)
- Test: `web/src/features/accounts/import/ImportPage.test.tsx`

**Interfaces:**
- Consumes: Task 4 routes, `ProgressBar`, `DataTable`, `usePlatforms`.
- Produces: `useStartImport()` (`POST /accounts/import/{platform}?commit=…` with the `File` as body and `Content-Type: text/csv`; returns `AccountImportDto`), `useImport(id)` (`refetchInterval: 2000` while `processing`, stops in every other state), `useContinueImport()` (`POST /accounts/imports/{id}/continue`), `useCancelImport()` (`DELETE /accounts/imports/{id}`). The import ID of the current run is kept in the URL (`?import=<id>`), so a reload shows a waiting import's choice again.
- Page:
  - A platform select (credential platforms only); the expected columns (schema field names plus optional `display_name`) with a "Download template" link that builds a header-only CSV in the browser; a file picker.
  - "Check file" (dry run); progress (`rows_done` of `rows_total`).
  - The per-row report table: line, status label from `accounts:importStatus.<status>` for every `ImportStatus`, violation codes through the violation catalog, field. Then the counts.
  - When `stopped` is set, a warning with the reason (`accounts:importStopped.<stopped>`, every value present) and the last line taken:
    - Dry run: "The check stopped after line {last_line}: {reason}. The rows up to there are checked below."
    - Commit run, `awaiting_decision`: "The file stopped after line {last_line}: {reason}. Nothing is connected yet." with two buttons:
      - **Connect the {rows_total} rows read** (`continue`). Once it completes, the page says: "Remove lines 2 to {last_line} from your file and import it again to continue with the rest."
      - **Cancel** (`cancel`, behind a `ConfirmDialog`): nothing is connected and the read rows are deleted.
      Under the buttons: "If you do neither, this import is canceled after 24 hours."
    - `canceled`: "Import canceled. Nothing was connected."
    - `failed`: "This import did not finish." (a waiting import the sweep failed reads the same).
  - A `409 import_not_waiting` from either button reloads the import and shows its current state.
  - "Connect valid rows" (commit) re-sends the same `File` held in component state; if the page was reloaded (no `File`), the button reads "Pick the file again to connect".
  - `413 payload_too_large` and `422 csv_invalid` show at once.
- [ ] **Step 1: Write failing tests:** `dry run shows progress then the report`, `commit resends the held file`, `after reload asks to pick the file again`, `bad header answers at once`, `oversized declared file answers at once`, `stopped import names the last line and the reason` (each `stopped` value), `stopped commit asks before connecting` (no polling while `awaiting_decision`; both buttons shown), `continue connects the rows read and polls to completion`, `cancel asks first and shows canceled`, `a waiting import survives a reload (import ID in the URL)`, `import_not_waiting reloads the state`, `polling stops when completed`, `every import status has a label`, `every stop reason has a label`, `a 1,000-row report renders and paginates`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): account CSV import with progress`.

### Task 16: Admin platforms, live switch, per-user limits

**Files:**
- Create: `web/src/features/admin/queries.ts`, `PlatformsPage.tsx`, `PublishingPage.tsx`, `UserLimitsDialog.tsx`, `web/src/i18n/en-US/admin.json` (registered in `index.ts` and `i18next.d.ts`)
- Modify: `web/src/features/users/UsersPage.tsx` (a "Limits" action per row), `web/src/routes.tsx` (`/admin/platforms`, `/admin/publishing` under `RequireAdmin`)
- Test: `web/src/features/admin/*.test.tsx`, `web/src/features/users/UsersPage.test.tsx`

**Interfaces:**
- Produces: query keys under `['admin', …]` (in `SELF_KEYS`; admin routes never carry act-as).
  - Platforms page: a `DataTable` of `AdminPlatformDto` (name with `PlatformBadge` when listed by `GET /platforms`, compiled in, configured, an enabled `Switch` → `PUT /admin/platforms/{id}` `{ enabled }`, redirect URI with a copy button, shown only when set).
  - Publishing page: the live switch (`GET/PUT /admin/settings/publishing`) with the native-scheduled count and the warning "{n} posts are already scheduled on the platforms themselves and will still go out"; turning live off asks for confirmation.
  - Limits dialog: the three editable `UserLimitsDto` fields (`contacts_cap`, `email_daily_cap`, `storage_quota_bytes`) as integer inputs with `GET/PUT /admin/users/{id}/limits`.
- [ ] **Step 1: Write failing tests:** `enable toggle calls PUT`, `redirect URI copies`, `turning live off asks first and shows the native count`, `limits dialog saves all three fields`, `member cannot reach the admin pages`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): admin platforms, live switch, and user limits`.

### Task 17: Guards, end-to-end spec, docs, and wrap-up

**Files:**
- Create: `web/src/features/noPlatformIds.test.ts`, `web/e2e/support.ts`, `web/e2e/d4a.spec.ts`, `docs/plans/2026-11-27-d4a-workspace-accounts-verification.md`
- Modify: `web/e2e/smoke.spec.ts` (import `login` from `support.ts`), `CLAUDE.md` ("Project state": D4a built), `docs/contracts/03. publishing roadmap.md` (an "As built (D4a)" note under Phase D4, including the decisions listed at the top of this plan), `docs/plans/2026-11-01-c3-telegram-agent-deferred.md` (connection guide: web half resolved in D4a Tasks 1 and 13; stays open for D6)

**Interfaces:**
- `noPlatformIds.test.ts` reads every `.ts`/`.tsx` under `src/features` and `src/components`, except `*.test.*` and `src/test/`. The IDs are the backend's `PLATFORM_ORDER` (`backend/crates/oauth/src/catalog.rs`), hard-coded in the test with a comment pointing there: `email`, `telegram`, `slack`, `discord`, `bluesky`, `mastodon`, `x`, `youtube`, `linkedin`, `facebook`, `instagram`, `threads`, `tiktok`. Two rules:
  - The distinctive IDs (all but `email` and `x`) fail as any quoted string literal.
  - `email` and `x` are ordinary words (`type="email"`, `autoComplete="email"`, a chart axis `'x'`), so they fail only where they name a platform: in a comparison (`=== 'x'`, `!== "email"`), a `case` label, or an object value under a key named `platform` or `platform_id`.
  - The test includes a fixture-string self-check showing each rule catches its target and lets `type="email"` through.
- `d4a.spec.ts` (opt-in, `POSTIT_E2E`, reuses `login` from `e2e/support.ts`):
  1. The admin enables `mock`.
  2. The owner connects a mock account by credentials (checklist, confirm) and one through mock OAuth.
  3. The owner imports a 3-row mock CSV (dry run, then connect) and creates a target group.
  4. The owner grants `member@postit.com` `publish` on one account.
  5. The member switches to the owner's workspace and sees only that account, no Connect button, and the banner.
  6. The owner revokes, and the member's tab returns to their own workspace with the notice.
- [ ] **Step 1: Write** `noPlatformIds.test.ts`; run it; fix any hit.
- [ ] **Step 2: Write** `support.ts` and `d4a.spec.ts`; run `pnpm exec playwright test --list` to prove they parse.
- [ ] **Step 3: Live check.** If the Playwright plugin's browser tools are available, start `cargo dev` (from `backend/`) and `pnpm dev`, ask the maintainer to sign in as the owner in the driven browser, and walk the D4a "done when" flow (connect by credentials and mock OAuth, import with progress, act as a delegate) on `https://postit.local:44315`, noting any defect and fixing it in this task. Whatever could not be checked (the delegate's session, the full spec run) goes to the verification file.
- [ ] **Step 4: Write** `2026-11-27-d4a-workspace-accounts-verification.md`: (1) run `POSTIT_E2E=1 pnpm exec playwright test e2e/d4a.spec.ts` against the dev stack — why: needs the Zitadel passwords; trigger: before merging D4a. (2) any item Step 3 could not cover, each with why and trigger. Remind the maintainer that `2026-11-27-consolidated-verification.md` has no D4a-triggered items (D2 checks wait for D4c, Papercut for D4d).
- [ ] **Step 5: Run** the full web gate and the backend gates (`fmt`, `clippy`, `check`, `xtask openapi --check`); all pass.
- [ ] **Step 6: Commit** `chore: guard against platform IDs in the web app and document D4a`.

Any agent work found during execution that cannot be finished inside this plan goes to `docs/plans/2026-11-27-d4a-workspace-accounts-deferred.md` with the issue, why, and its trigger; human-only checks go to the verification file.
