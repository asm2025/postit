# D4a Workspace and accounts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first D4 slice: a web user switches workspaces, manages delegations, preferences, and notifications, connects accounts by credentials, by OAuth, and by CSV import (now a background job), manages accounts, writing styles, and target groups, and an admin manages platforms, the live switch, and per-user limits.

**Architecture:** Two small backend changes (connection guides in the agent descriptor; the account CSV import moved onto the jobs ledger with sealed staging rows, after the contact import's pattern), then web screens over the D3 REST surface. One workspace module owns act-as (header, cache keys, ended grants); one `useAccess` hook gates actions; one `SchemaForm` kit component renders every credentials form. No platform-specific code in the web app.

**Tech Stack:** Rust (axum, sqlx, apalis behind `postit-jobs`, `postit-vault`), React 19, React Router 8, TanStack Query 5, `openapi-fetch`, i18next, Radix/shadcn, Tailwind 4, Vitest + MSW, Playwright.

**Spec:** `docs/specs/2026-11-27-d4-web-publishing-ui-design.md` (sections "Shared decisions", "Backend changes D4 needs: D4a", "Slice details: D4a"). Also read plan 03 Phase D4 and the REST surface table in `docs/contracts/03. publishing roadmap.md`.

## Global Constraints

- US English everywhere (code, catalogs, docs, commits).
- Web: pnpm only, run from PowerShell (the Git Bash pnpm shim is broken). Gate: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build` in `web/`.
- Pages use `web/src/components/kit/` and `web/src/components/ui/`; `kitGuard.test.ts` must stay green. A missing widget becomes a kit component.
- Every user-visible string is in an `en-US` catalog under `web/src/i18n/en-US/`; layouts use logical properties and mirror under `dir="rtl"`.
- No platform ID literal (`telegram`, `email`, `slack`, …) in `web/src/features/**` or `web/src/components/**` outside tests; platform names, colors, icons, forms, and guides come from `GET /platforms`.
- No credential value, API key, or `connect_id` in `localStorage`, logs, or error reports. `connect_id` lives in `sessionStorage` for at most its 10-minute lifetime.
- Backend gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, targeted `cargo test -p <crate>` for crates whose behavior changed (not the full workspace by reflex), `cargo xtask openapi --check` (PowerShell; regenerates `web/src/api/schema.d.ts` and `mobile/lib/api/`).
- `CARGO_TARGET_DIR=K:/cargo-target/postit`; DB tests use `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, quotes stripped; `-- --test-threads=4` for `postit-data`.
- Schema changes edit `backend/crates/data/migrations/0001_InitialSchema.sql` in place (single-migration policy); new `query!` macros need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data`.
- Commit trailer: `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`.

## Review Focus

1. **Workspace switch with data in flight:** a response for workspace A arriving after the switch to B must never render in B. Test in Task 5 (`switching drops the old workspace's queries and late answers`).
2. **Act-as on self and admin routes:** `/me/*`, `/notifications*`, `/push/*`, `/delegations/received*`, `/users/lookup`, `/ai/presets`, `/auth/config`, and `/admin/*` must never carry `X-Postit-Act-As`, or the server answers `owner_only`. Test in Task 5 (`needsActAs` table test).
3. **A grant that ends mid-session:** any `403 delegation_invalid` switches the tab to the own workspace once and shows why, without a retry loop. Test in Task 5.
4. **A stale or foreign pending connection:** a `connect_id` entry past `expires_at`, or one left from another user in the same tab, is dropped and no checklist is shown; a reload inside 10 minutes resumes. Test in Task 12.
5. **CSV import edge cases:** a header error or an oversized file answers at once (422/413) with no import created; a commit after a reload (the file no longer in memory) asks the user to pick the file again; a 2,000-row file shows progress, not a frozen page. Tests in Tasks 3 and 14.

---

## File Structure

Backend:
- `backend/crates/core/src/descriptor.rs` — `ConnectGuide`, `AgentDescriptor.guide`.
- `backend/crates/agents/{telegram,email,mock}/src/descriptor.rs` — guide steps.
- `backend/crates/data/migrations/0001_InitialSchema.sql` — `account_imports`, `account_import_rows`.
- `backend/crates/data/src/account_imports.rs` (new) — `AccountImportRepo`.
- `backend/crates/data/src/vault.rs`, `retention.rs` — sealed column entry; purge and stale sweep.
- `backend/crates/oauth/src/import.rs` — staged import service; `backend/crates/oauth/src/jobs.rs` — `AccountImportJob`.
- `backend/crates/api/src/routes/accounts.rs`, `dto_accounts.rs`, `router.rs` — `202` import, `GET /accounts/imports/{id}`.
- `backend/crates/server/src/compose.rs`, `lib.rs` — wiring.

Web (new folders follow `features/jobs/`):
- `web/src/workspace/` — `store.ts`, `WorkspaceProvider.tsx`, `useAccess.ts`, `WorkspaceSwitcher.tsx`, `WorkspaceBanner.tsx`, `keys.ts`.
- `web/src/api/actAs.ts` — the self/admin prefix list.
- `web/src/components/kit/` — `SchemaForm.tsx`, `schema.ts`, `PlatformBadge.tsx`, `AccountPicker.tsx`, `ProgressBar.tsx`; `web/src/components/ui/` — `popover.tsx`, `tabs.tsx`, `textarea.tsx` (shadcn, Radix style).
- `web/src/i18n/agentText.ts`, `web/src/i18n/en-US/{agents,accounts,delegations,notifications,admin}.json`.
- `web/src/features/platforms/queries.ts` — `usePlatforms`, `usePlatformIcon`.
- `web/src/features/notifications/`, `web/src/features/delegations/`, `web/src/features/accounts/` (list, detail, connect, checklist, oauth-done, import, target groups), `web/src/features/admin/` (platforms, publishing, limits).
- `web/e2e/d4a.spec.ts`.

---

### Task 1: Connection guides in the agent descriptor

Picks up `docs/plans/2026-11-01-c3-telegram-agent-deferred.md`, "The connection guide" (web half).

**Files:**
- Modify: `backend/crates/core/src/descriptor.rs`, every `AgentDescriptor { … }` literal (`rg "AgentDescriptor \{" backend/crates` lists them: core tests, agent-kit echo, drafter testkit, oauth catalog tests, publishing outcome tests, core wire_format test, the three agents)
- Modify: `backend/crates/agents/telegram/src/descriptor.rs`, `backend/crates/agents/email/src/descriptor.rs`, `backend/crates/agents/mock/src/descriptor.rs`
- Test: `backend/crates/core/tests/wire_format.rs`, each agent's descriptor tests

**Interfaces:**
- Produces: `pub struct ConnectGuide { pub steps: Vec<Message>, pub url: Option<Url> }` (`Default`, `Serialize`, `Deserialize`, `ToSchema` under `openapi`); `AgentDescriptor.guide: ConnectGuide` with `#[serde(default)]`. On the wire: `"guide": { "steps": [{ "key", "en" }], "url": null }`.

- [ ] **Step 1: Write failing tests**
  - `wire_format.rs::descriptor_serializes_guide`: a descriptor with two steps and a URL serializes `guide.steps[0] == {"key":"x.guide.1","en":"…"}` and `guide.url`; a JSON descriptor without `guide` deserializes to `ConnectGuide::default()`.
  - `telegram` descriptor test `guide_covers_the_c3_steps`: 5 steps, keys `telegram.guide.1`…`telegram.guide.5`, English text covering, in order: create a dedicated bot with BotFather; add it as a channel or group admin with "Post messages"; add a private channel by its numeric ID (an invite link does not work); when replacing a revoked token, name the chats again (discovery only sees the last 24 hours of updates); to disconnect for good, revoke the token in BotFather. `url` = `https://core.telegram.org/bots/features#botfather`.
  - `email` descriptor test `guide_has_smtp_steps`: 3 steps (`email.guide.1`…`3`): find the SMTP host, port, and TLS mode in your provider's help; use an app password where the provider offers one, not your mailbox password; the postal address is required by anti-spam law and appears in every footer. `url` = `None`.
  - `mock` descriptor tests: broadcast, direct, and OAuth mocks each have exactly one step (`mock.guide.1`, "Any value works; this is a test platform.").
- [ ] **Step 2: Run** `cargo test -p postit-core -p postit-agent-telegram -p postit-agent-email -p postit-agent-mock` — expect compile failure (no `guide`).
- [ ] **Step 3: Implement** the type and field; fill the three agents; add `guide: ConnectGuide::default()` to every other literal.
- [ ] **Step 4: Run** the same tests (pass), then `cargo xtask openapi` (PowerShell) and confirm `schema.d.ts` has `ConnectGuide` and `AgentDescriptor.guide`.
- [ ] **Step 5: Commit** `feat: carry connection guides in the agent descriptor`.

### Task 2: Account import staging schema and repository

**Files:**
- Modify: `backend/crates/data/migrations/0001_InitialSchema.sql` (after `contact_import_rows`)
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
    connect BOOLEAN NOT NULL,              -- false: check only (the dry run)
    status TEXT NOT NULL DEFAULT 'processing'
        CHECK (status IN ('processing', 'completed', 'failed')),
    rows_total INTEGER NOT NULL DEFAULT 0,
    rows_done INTEGER NOT NULL DEFAULT 0,
    truncated BOOLEAN NOT NULL DEFAULT false,
    -- One entry per processed row in file order: row number, status, violation codes, field
    -- name, account IDs. Never a cell value.
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
CREATE INDEX account_imports_status_idx ON account_imports (updated_at) WHERE status = 'processing';

CREATE TABLE account_import_rows (
    id UUID PRIMARY KEY,
    import_id UUID NOT NULL,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    row_no INTEGER NOT NULL,
    data BYTEA NOT NULL, -- postit:sealed: the row's cells and display name
    UNIQUE (import_id, row_no),
    FOREIGN KEY (owner_id, import_id) REFERENCES account_imports (owner_id, id) ON DELETE CASCADE
);
```

**Interfaces:**
- Produces in `postit_data::account_imports`:
  - `AccountImportId` (newtype over `Uuid`, in `postit-core` ids if the crate keeps IDs there, else local).
  - `pub struct AccountImport { id, platform: PlatformId, connect: bool, status: ImportState, rows_total: i32, rows_done: i32, truncated: bool, report: Value, created_at, updated_at }`; `pub enum ImportState { Processing, Completed, Failed }`.
  - `pub struct StagedAccountRow { pub id: Uuid, pub row_no: i32, pub data: Sealed<StagedCells> }`; `#[derive(Serialize, Deserialize)] pub struct StagedCells { pub cells: Vec<(String, String)>, pub display_name: Option<String> }` (names and raw cells; the header order).
  - `AccountImportRepo::create(conn, scope, id, platform, connect) -> Result<(), DataError>` (requires `OwnerOnly`)
  - `::stage(conn, scope, id, rows: &[StagedAccountRow]) -> Result<(), DataError>`; `::set_total(conn, scope, id, total, truncated)`
  - `::take_chunk(conn, owner, id, limit: i64) -> Result<Vec<StagedAccountRow>, DataError>` (`DELETE … RETURNING`, lowest `row_no` first, `FOR UPDATE SKIP LOCKED` not needed: one job per import)
  - `::append(conn, owner, id, entries: &[Value], seen: &[String]) -> Result<(), DataError>` (appends to `report` and `seen_remote_ids`, adds to `rows_done`, touches `updated_at`)
  - `::seen(conn, owner, id) -> Result<Vec<String>, DataError>`; `::finish(conn, owner, id)` (`completed`); `::abandon(conn, owner, id)` (`failed`, rows deleted)
  - `::get(conn, scope, id) -> Result<AccountImport, DataError>` (owner-only; `NotFound` for another owner's)
  - `AccountImportRepo::seal_context(row: Uuid) -> SealContext { table: "account_import_rows", column: "data", row }`
  - `retention::purge_account_imports(conn, cutoff) -> Result<u64, DataError>`; `retention::fail_stale_account_imports(conn, cutoff) -> Result<u64, DataError>` (24 hours without progress, rows deleted in the same statement, like `fail_stale_imports`).

- [ ] **Step 1: Write failing tests** in `data/tests/account_imports.rs`: `stage_take_append_finish_round_trip` (3 rows staged, `take_chunk(2)` returns rows 1–2 and deletes them, `append` raises `rows_done` to 2, `finish` sets `completed`); `rows_are_sealed_and_open_only_for_their_row` (bytes do not contain a staged cell's text; opening with another row's context fails); `get_refuses_another_owner` (`NotFound`); `delegate_cannot_create` (`OwnerOnly` refusal for every delegation level); `stale_import_fails_and_loses_rows` (an import with `updated_at` 25 hours ago is `failed` with no rows; a fresh one untouched); `purge_deletes_finished_imports_only`. Add the new table to `scope_matrix.rs` per its pattern.
- [ ] **Step 2: Run** `cargo test -p postit-data --test account_imports -- --test-threads=4` — fails (no module).
- [ ] **Step 3: Implement** schema, repo, `SEALED_COLUMNS` entry (`account_import_rows.data`, `id`), retention functions, maintenance calls, the setting with docs; run `cargo sqlx prepare`.
- [ ] **Step 4: Run** `cargo test -p postit-data --test account_imports --test schema --test scope_matrix -- --test-threads=4` — pass; `cargo test -p postit-config` — pass.
- [ ] **Step 5: Commit** `feat: stage account imports in sealed rows`.

### Task 3: Account import as a background job

Picks up `docs/plans/2026-10-23-b3-credential-connection-deferred.md`, "Import progress for large files".

**Files:**
- Modify: `backend/crates/oauth/src/import.rs`, `jobs.rs`, `service.rs` (`AccountServiceDeps.queue: JobQueue`), `testkit.rs`, `lib.rs`
- Modify: `backend/crates/server/src/compose.rs` (`account_service` takes the jobs queue; register `AccountImportJob` and its console spec)
- Test: `backend/crates/oauth/tests/import.rs`, `tests/leak.rs`

**Interfaces:**
- Consumes: Task 2 repo.
- Produces:
  - `#[derive(Serialize, Deserialize)] pub struct AccountImportJob { pub import: Uuid, pub owner: Uuid }`, `JOB_TYPE = "account_import"`, `QUEUE = Queue::Default` (no accounts queue exists; `Maintenance` is for housekeeping); retry policy `IMPORT_RETRY` as in contacts (5 attempts, 30 s to 10 min).
  - `AccountService::start_import<R: AsyncRead + Unpin + Send>(&self, scope, platform: &PlatformId, body: R, connect: bool, audit: AuditContext) -> Result<AccountImport, AccountError>`: owner-only, resolves the platform, refuses OAuth platforms (`ConnectMethodUnsupported`), reads and checks the header synchronously (the existing `read_headers`), creates the import, stages rows in chunks of 100 sealed per row, stops at `accounts.csv_max_rows` and sets `truncated`, sets the total and enqueues the job in one transaction (as `ContactService::stage`). A body read failure after the import exists abandons it and returns `CsvInvalid`.
  - `AccountService::get_import(&self, scope, id) -> Result<AccountImport, AccountError>`.
  - `AccountService::run_import(&self, job: AccountImportJob) -> Result<(), JobError>`: while rows remain, takes a chunk of `csv_concurrency * 4` rows, opens each, runs the existing `check_row` concurrently (`buffered(csv_concurrency)`) and `settle_row` in file order with `seen` loaded from `seen_remote_ids`, then appends the report entries and the newly seen IDs in one transaction; finally `finish` and `record_import` (one `accounts_imported` audit event with the counts, as today). A run for an import that is gone or not `processing` changes nothing. A committed row's connect transaction and its `append` are separate; a crash between them may report that row as `already_connected` on resume, which is correct (it is connected).
  - The synchronous `import_csv` and `ImportReport` are removed; `ImportRow` and `ImportStatus` stay (they are the report entries).

- [ ] **Step 1: Write failing tests** in `oauth/tests/import.rs` (rewrite the existing ones onto the new API; keep every case they cover): `start_import_stages_and_queues` (3-row CSV: import `processing`, `rows_total == 3`, one queued `account_import` ledger row, no account yet); `run_import_dry_run_reports_without_connecting` (statuses as the old dry-run test, zero accounts); `run_import_connects_valid_rows` (as the old commit test); `repeated_remote_account_reports_already_connected_across_chunks` (rows 1 and 9 reach the same account with chunk size forced to 4: row 9 `already_connected` in a dry run); `resume_after_crash_does_not_connect_twice` (run one chunk, drop the service, run again: each account connected once, `rows_done == rows_total`); `bad_header_answers_before_any_import` (`CsvInvalid`, zero `account_imports` rows); `over_max_rows_truncates`; `delegate_refused` (every level, `OwnerOnly`); `oauth_platform_refused`. In `tests/leak.rs`: run an import whose cells hold a known token and assert the token is absent from logs, the report JSON, the audit row, and the ledger payload.
- [ ] **Step 2: Run** `cargo test -p postit-oauth --test import --test leak` — fail.
- [ ] **Step 3: Implement** as in Interfaces; wire the queue and registration in `compose.rs`/`lib.rs`; console spec with `id_field("import", "import_id")` and `id_field("owner", "owner_id")`.
- [ ] **Step 4: Run** the same tests — pass; `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- [ ] **Step 5: Commit** `feat: run account CSV imports as a background job`.

### Task 4: Import routes

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs`, `dto_accounts.rs`, `router.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/accounts.rs`

**Interfaces:**
- Produces:
  - `POST /api/v1/accounts/import/{platform}?commit=…` answers `202` with `AccountImportDto` and `Location: /api/v1/accounts/imports/{id}`. Content-type check, `credential_checks` charge, and the size limit (`413 payload_too_large`) stay as today.
  - `GET /api/v1/accounts/imports/{id}` (owner) answers `AccountImportDto { id, platform, commit: bool, status: "processing" | "completed" | "failed", rows_total, rows_done, truncated, counts: { <ImportStatus>: n }, rows: ImportRowDto[] }` (`counts` computed from `rows`). `operation_id = "get_account_import"`.
- [ ] **Step 1: Write failing tests:** `import_answers_202_with_location`; `get_import_shows_progress_then_completion` (drain the job through the API testkit's job runner, as D3's `drain_publishing` does for publishing: add `TestApp::drain_queue(job_type: &str)` if no generic helper exists); `import_isolation_matrix` (owner 202; every delegate level `403 owner_only` on both routes; unrelated user `404` on GET); `get_import_never_shows_cells`; existing `import_*` route tests updated to the new shape.
- [ ] **Step 2: Run** `cargo test -p postit-api --test accounts` — fail.
- [ ] **Step 3: Implement**; regenerate with `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests and `cargo xtask openapi --check` — pass. In `docs/plans/2026-10-23-b3-credential-connection-deferred.md`, mark "Import progress for large files" **resolved in D4a (Tasks 2–4)**; the Dart entry stays open, so the file keeps its name.
- [ ] **Step 5: Commit** `feat: answer account imports with an import ID and a progress route`.

### Task 5: Workspace store, act-as header, cache isolation

**Files:**
- Create: `web/src/workspace/store.ts`, `web/src/workspace/keys.ts`, `web/src/workspace/WorkspaceProvider.tsx`, `web/src/api/actAs.ts`
- Modify: `web/src/api/authFetch.ts`, `web/src/api/client.ts`, `web/src/api/queryClient.ts`, `web/src/App.tsx`, `web/src/test/render.tsx` (render inside `WorkspaceProvider`, option `actAs?: string`)
- Test: `web/src/api/actAs.test.ts`, `web/src/api/authFetch.test.ts`, `web/src/workspace/WorkspaceProvider.test.tsx`

**Interfaces:**
- Produces:
  - `store.ts`: `ACT_AS_KEY = 'postit.actAs'`; `getActAs(): string | null`; `setActAs(owner: string | null): void`; `subscribeActAs(fn: () => void): () => void` (sessionStorage, wrapped in try/catch; in-memory fallback).
  - `actAs.ts`: `needsActAs(url: string): boolean` — false for paths under `/api/v1/` starting with `me`, `notifications`, `push`, `delegations/received`, `users/lookup`, `ai/presets`, `auth/config`, `admin/`; true for everything else under `/api/v1/`; false outside it.
  - `createAuthFetch(source, baseFetch?, actAs: () => string | null = getActAs)` sets `X-Postit-Act-As` when `actAs()` is non-null and `needsActAs(request.url)`.
  - `keys.ts`: `wsKey(owner: string | null, ...parts: unknown[]): readonly unknown[]` → `['ws', owner ?? 'own', ...parts]`. Every workspace-scoped query in later tasks uses it via `useWsKey()`.
  - `WorkspaceProvider`: context `{ actAs: string | null; workspaces: WorkspaceDto[]; current: WorkspaceDto | undefined; switchTo(owner: string | null): void; endedNotice: string | null; clearEndedNotice(): void }`, exported as `useWorkspace()`. `workspaces` from `GET /api/v1/me/workspaces` (key `['workspaces']`). `switchTo` calls `setActAs`, then `queryClient.removeQueries({ queryKey: ['ws'] })`. A stored owner that is not in `workspaces` falls back to own.
  - `createQueryClient({ retry, onDelegationInvalid })`: on any query or mutation error with code `delegation_invalid`, calls `onDelegationInvalid` once per stored owner (the provider switches to own and sets `endedNotice` to `t('common:workspace.ended')`).
- [ ] **Step 1: Write failing tests:** `actAs.test.ts` table: `/api/v1/accounts` true, `/api/v1/me/workspaces` false, `/api/v1/me` false, `/api/v1/notifications/unread-count` false, `/api/v1/delegations` true, `/api/v1/delegations/received/x` false, `/api/v1/admin/platforms` false, `/api/v1/users/lookup?email=a` false, `/api/v1/ai/presets` false, `/config.json` false. `authFetch.test.ts`: `adds act-as only where needed`, `no header in own workspace`. `WorkspaceProvider.test.tsx`: `switching drops the old workspace's queries and late answers` (a slow MSW `/accounts` answer for A resolves after `switchTo(B)`; the rendered list shows B's data only), `delegation_invalid returns to own workspace once` (two failing queries → one switch, notice shown), `unknown stored owner falls back to own`, `act-as survives a remount (sessionStorage)`.
- [ ] **Step 2: Run** `pnpm exec vitest run src/api src/workspace` (PowerShell, in `web/`) — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests — pass; run the whole web gate.
- [ ] **Step 5: Commit** `feat(web): act in a delegated workspace per tab`.

### Task 6: Access gating, switcher, banner, navigation

**Files:**
- Create: `web/src/workspace/useAccess.ts`, `WorkspaceSwitcher.tsx`, `WorkspaceBanner.tsx`
- Modify: `web/src/components/Shell.tsx`, `web/src/routes.tsx`, `web/src/i18n/en-US/common.json`
- Test: `web/src/workspace/useAccess.test.tsx`, `web/src/components/Shell.test.tsx`

**Interfaces:**
- Consumes: `useWorkspace()` (Task 5).
- Produces: `useAccess(): { own: boolean; level: 'owner' | 'view' | 'edit' | 'publish'; atLeast(l: 'view' | 'edit' | 'publish'): boolean; inLimit(accountId: string): boolean }` (`own` → `level 'owner'`, `atLeast` always true, `inLimit` always true; delegated → from `current.level`, `all_accounts`, `account_ids`). Also `<RequireOwn>` route element: renders the outlet in the own workspace, else `EmptyState` "Only the workspace owner can do this" with a button to switch to own.
- Nav per the spec's sketch, with only the items this slice builds: Home; Accounts; SETTINGS: Profile and preferences, Delegations (own only); ADMIN: Users, Platforms, Publishing, Audit, Jobs. The switcher sits above the nav (own first, then each received workspace with the owner's name and level; a limited grant shows "{n} accounts"). The banner sits under the header while delegated: "You are working in {name}'s workspace ({level})" with "Back to my workspace". The bell slot is in the header (filled by Task 9).
- [ ] **Step 1: Write failing tests:** `useAccess` for own, view, edit, publish, limited publish (`inLimit` true only for listed IDs); Shell: `switcher lists own and received workspaces`, `banner shows while delegated and its button returns to own`, `Delegations hidden while delegated`, `admin section only for admins`, both directions (`dir="rtl"` render keeps the switcher at the start edge: assert logical classes `ms-`/`me-`, no `ml-`/`mr-`).
- [ ] **Step 2: Run** `pnpm exec vitest run src/workspace src/components/Shell.test.tsx` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): gate actions by workspace access and add the switcher`.

### Task 7: Kit additions: SchemaForm, PlatformBadge, AccountPicker, ProgressBar

**Files:**
- Create: `web/src/components/ui/popover.tsx`, `tabs.tsx`, `textarea.tsx` (`pnpm dlx shadcn add popover tabs textarea`, Radix style, then logical-property fixes)
- Create: `web/src/components/kit/schema.ts`, `SchemaForm.tsx`, `PlatformBadge.tsx`, `AccountPicker.tsx`, `ProgressBar.tsx`
- Create: `web/src/i18n/agentText.ts`, `web/src/i18n/en-US/agents.json` (`{}`); register namespace `agents` in `web/src/i18n/index.ts`
- Create: `web/src/features/platforms/queries.ts`
- Test: `web/src/components/kit/SchemaForm.test.tsx`, `kit.test.tsx` (extend), `web/src/features/platforms/queries.test.tsx`

**Interfaces:**
- Produces:
  - `agentText(m: { key: string; en: string }): string` — `i18next.exists('agents:' + key) ? t(...) : m.en`.
  - `schema.ts`: `type FieldKind = { type: 'string'; minLength?: number; maxLength?: number } | { type: 'multiline'; maxLength?: number } | { type: 'enum'; values: { value: string; label: AgentMessage }[] } | { type: 'boolean' } | { type: 'integer'; minimum?: number; maximum?: number } | { type: 'datetime' }`; `interface SchemaFieldSpec { name; label: AgentMessage; help?: AgentMessage; required: boolean; kind: FieldKind; default?: unknown; noDefault: boolean; secret: boolean; consentText?: AgentMessage; dependsOn?: { field: string; equals: unknown } }`; `parseSchema(schema: unknown): SchemaFieldSpec[]` (declaration order; a field with an unknown keyword is skipped with `console.warn` in development only); `initialValues(fields): Record<string, unknown>` (defaults, none for `noDefault` or secret); `submittedValues(fields, values): Record<string, unknown>` (drops fields whose `dependsOn` does not hold, and empty optional strings).
  - `SchemaForm({ fields, values, onChange, errors, disabled }: { fields: SchemaFieldSpec[]; values: Record<string, unknown>; onChange(next: Record<string, unknown>): void; errors?: Record<string, ReactNode>; disabled?: boolean })` — string → `Input`; multiline → `Textarea`; enum → `OptionSelect`; boolean → `Switch`; consent → `Checkbox` with the consent text as its label, required true; integer → `Input type="number"` with min/max; datetime → `Input type="datetime-local"` shown in the user's zone, value kept as RFC 3339 with offset; secret → password `Input` with a show/hide button, `autoComplete="off"`, never prefilled; help under the input; required marked.
  - `usePlatforms()` → `GET /api/v1/platforms`, `staleTime: 5 min`, key `['platforms']` (not workspace-scoped); `usePlatform(id)`; `usePlatformIcon(id): string | undefined` — fetches the PNG through the API client (it needs the bearer), returns an object URL, `staleTime: Infinity`, revokes on cache removal.
  - `PlatformBadge({ platform, size = 'md', showName = true })` — icon (or the first letter on the brand color while loading or when missing) and display name.
  - `AccountPicker({ value: string[]; onChange(ids: string[]): void; accounts: AccountDto[] })` — checkbox list grouped by platform with `PlatformBadge`; callers pass only accounts in the caller's limit.
  - `ProgressBar({ value, max, label })` — `role="progressbar"` with `aria-valuenow`/`aria-valuemax`, logical fill direction.
- [ ] **Step 1: Write failing tests:** `SchemaForm.test.tsx`, one test per widget (`renders string with limits`, `multiline`, `enum with labels`, `boolean`, `consent must be checked`, `integer bounds`, `datetime round trips in the user's zone`, `secret is masked, toggles, and is never prefilled`, `help text`, `noDefault starts empty and counts as required`, `depends-on hides and drops the field`, `unknown keyword skips the field`), each also rendered under `dir="rtl"`; `agentText falls back to en`; `usePlatformIcon returns an object URL and sends the bearer`; `PlatformBadge shows the initial while the icon loads`; `ProgressBar exposes aria values`.
- [ ] **Step 2: Run** `pnpm exec vitest run src/components/kit src/features/platforms src/i18n` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass; `kitGuard.test.ts` still passes.
- [ ] **Step 5: Commit** `feat(web): add the schema form and platform kit components`.

### Task 8: Preferences and the startup error

**Files:**
- Modify: `web/src/features/me/ProfilePage.tsx` (title "Profile and preferences"), add `web/src/features/me/TimeZoneSetting.tsx`; Modify: `web/src/config/runtime.ts`, `web/src/main.tsx`, `web/src/StartupError.tsx`, `web/src/i18n/en-US/{me,common}.json`
- Test: `web/src/features/me/ProfilePage.test.tsx`, `web/src/StartupError.test.tsx`, `web/src/config/runtime.test.ts`

**Interfaces:**
- Produces: `TimeZoneSetting` — an `OptionSelect` over `Intl.supportedValuesOf('timeZone')`, saving through `useUpdatePreferences({ timezone })`; when the saved zone differs from the browser's (`Intl.DateTimeFormat().resolvedOptions().timeZone`) and the `localStorage` flag `postit.tzAsked` is unset, a one-time prompt on the profile page offers the browser's zone; either answer sets the flag (a per-browser convenience, not a secret). The language row stays behind `MULTILINGUAL`. `NotificationSettings` stays as is.
- `runtime.ts` throws `class StartupFailure extends Error { reason: 'runtime_config' | 'auth_config' | 'identity_unreachable' }` instead of English messages; `StartupError({ reason, onRetry })` renders `t('startup.reason.<reason>')` (plus a generic fallback); `main.tsx` passes the reason. Keys in `common.json`: `startup.reason.runtime_config` ("The app's settings could not be loaded."), `startup.reason.auth_config` ("The sign-in settings could not be loaded from the server."), `startup.reason.identity_unreachable` ("The sign-in service cannot be reached."), `startup.reason.unknown`.
- [ ] **Step 1: Write failing tests:** `time zone saves the chosen zone`, `prompt offers the browser zone once`, `startup error shows the translated reason`, `runtime config failure carries runtime_config`, `auth config failure carries auth_config`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): time zone preference and a translated startup error`.

### Task 9: Notifications bell, list, and deep links

**Files:**
- Create: `web/src/features/notifications/queries.ts`, `NotificationBell.tsx`, `NotificationsPage.tsx`, `links.ts`, `web/src/i18n/en-US/notifications.json`
- Modify: `web/src/components/Shell.tsx` (bell in the header; replace the `?notification` stub with the shared opener), `web/src/routes.tsx` (`/notifications`)
- Test: `web/src/features/notifications/*.test.tsx`, `links.test.ts`

**Interfaces:**
- Produces:
  - `useUnreadCount()` — `GET /api/v1/notifications/unread-count`, `refetchInterval: 60_000` while visible (`refetchIntervalInBackground: false`), `refetchOnWindowFocus: true`; key `['notifications', 'unread']` (self, not workspace-scoped).
  - `useNotifications(page)`, `useMarkRead()` (`POST /notifications/read` with `{ ids }` or `{ all: true }` as the route takes; invalidates both keys).
  - `linkFor(n: NotificationDto): string | null` — `account_needs_reconnect`, `account_revoked_by_platform`, `reconnect_by_passed`, `reconnect_reminder` → `/accounts/{account_id}`; `delegation_changed` → `/delegations`; `user_pending` → `/users`; delivery, job, recurrence, and comment kinds → `null` in D4a (D4b and later fill them).
  - `useOpenNotification()` — marks read, switches workspace to `owner_id` when it differs from the active one (Task 5 `switchTo`), then navigates to `linkFor`, or stays on the list.
  - Bell: badge with the count (hidden at 0, "99+" above 99), a `Popover` with the newest 10, "Mark all read", "See all". Page: `DataTable` with server paging via `useListParams`, unread filter, kind label from the catalog (`notifications:kind.<kind>`, every `NotificationKind` present; a completeness test like `violations.test.ts`), severity.
- [ ] **Step 1: Write failing tests:** `bell shows unread badge and polls`, `opening a notification from another workspace switches first`, `linkFor covers every kind` (iterate the generated enum; `null` allowed only for the listed kinds), `mark all read clears the badge`, `push click ?notification=id opens it`, catalog completeness for kinds.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): notifications bell and list`.

### Task 10: Delegations

**Files:**
- Create: `web/src/features/delegations/queries.ts`, `DelegationsPage.tsx`, `GrantDialog.tsx`, `web/src/i18n/en-US/delegations.json`
- Modify: `web/src/routes.tsx` (`/delegations` under `RequireOwn` for the "Granted" tab; "Received" works in any workspace)
- Test: `web/src/features/delegations/*.test.tsx`

**Interfaces:**
- Consumes: `AccountPicker`, `useAccess`, `wsKey`.
- Produces: two tabs. **Granted** (`GET/POST /delegations`, `PATCH/DELETE /delegations/{id}`): table of delegate, level, accounts (all or count), expiry, status; Grant dialog: email (exact match through `GET /users/lookup?email=`, showing the found name or "No postit user has this email"), level (`view`, `edit`, `publish` with one-line explanations), all accounts or chosen (`AccountPicker`), optional expiry date; edit opens the same dialog prefilled (PATCH sends every field, `expires_at` included); revoke in a `ConfirmDialog`. **Received** (`GET /delegations/received`, `DELETE /delegations/received/{id}`): owner, level, accounts, expiry, "Open workspace" (`switchTo`), "Leave" with confirmation; leaving the active workspace switches to own.
- [ ] **Step 1: Write failing tests:** `grant by exact email`, `lookup miss shows the message and blocks submit`, `edit keeps expires_at`, `revoke asks first`, `leave the active workspace returns to own`, `granted tab hidden while delegated`, `rate_limited lookup shows the wait`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): grant, edit, revoke, and leave delegations`.

### Task 11: Accounts list and account detail

**Files:**
- Create: `web/src/features/accounts/queries.ts`, `AccountsPage.tsx`, `AccountDetailPage.tsx`, `WritingStyleForm.tsx`, `PendingConnections.tsx`, `web/src/i18n/en-US/accounts.json`
- Modify: `web/src/routes.tsx` (`/accounts`, `/accounts/:id`)
- Test: `web/src/features/accounts/AccountsPage.test.tsx`, `AccountDetailPage.test.tsx`

**Interfaces:**
- Consumes: `useAccess`, `PlatformBadge`, `wsKey`, `usePlatforms`.
- Produces:
  - `useAccounts({ platform?, includeDisconnected?, page, pageSize })`, `useAccount(id)`, `useAccountActions()` with `rename`, `setTestAccount`, `check`, `disconnect` (owner-only routes), `useWritingStyle(id)`, `useSaveWritingStyle(id)`, `usePendingConnections()`, `useCancelPending()`.
  - Accounts page: `PageHeader` with "Connect an account" and "Import from CSV" (owner only); rows grouped by platform (`PlatformBadge`), status badge (`active`, `needs_reconnect`, `revoked`, `disconnected`), test flag, last checked; "Show disconnected" filter in the URL. Unfinished connections (owner only) from `GET /accounts/pending-connections`: "Unfinished connection to {platform}, expires in {n} min" with **Cancel** (`DELETE /accounts/pending-connections/{pending_id}`) and **Start again** (opens the connect flow for that platform); never Confirm.
  - Detail page: `FactList` (platform, remote ID, status, connected, last checked), rename and test flag (owner), "Check now" (owner; shows the result status), writing style form (view: read-only; edit with the account in limit: editable; fields tone, emoji use, hashtag habits, language override, notes as the DTO defines them), disconnect in `DangerZone` (owner; `ConfirmDialog` saying scheduled deliveries to this account are canceled and history is kept). An account outside the limit is `404` from the server → "Account not found".
- [ ] **Step 1: Write failing tests** at each access level (owner, view, edit, publish, limited): `owner sees connect and import`, `delegate sees no owner actions`, `pending connection shows cancel and start again, never confirm`, `writing style read-only for view`, `writing style editable for edit in limit`, `disconnect asks first and returns to the list`, `check now shows the new status`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): accounts list and account detail`.

### Task 12: Connecting accounts: credentials, OAuth, discovery checklist

**Files:**
- Create: `web/src/features/accounts/connect/ConnectPage.tsx`, `CredentialsStep.tsx`, `DiscoveryChecklist.tsx`, `OAuthDonePage.tsx`, `pendingStore.ts`
- Modify: `web/src/routes.tsx` (`/accounts/connect`, `/accounts/connect/:platform` under `RequireOwn`; `/oauth-done` inside the session gate, outside the Shell's own-workspace check: the connection belongs to whoever started it)
- Test: `web/src/features/accounts/connect/*.test.tsx`, `pendingStore.test.ts`

**Interfaces:**
- Consumes: `SchemaForm`, `parseSchema`, `submittedValues`, `PlatformBadge`, `agentText`, `usePlatforms`.
- Produces:
  - `pendingStore.ts`: `PENDING_KEY = 'postit.pendingConnection'`; `savePending(p: { connectId: string; pendingId: string | null; platform: string; userId: string; expiresAt: string }): void`; `loadPending(userId: string, now = Date.now()): PendingEntry | null` (drops and returns null when expired or another user's); `clearPending(): void`. sessionStorage only, try/catch.
  - Connect page: platform grid from `usePlatforms()` (`PlatformBadge`, kind, connect method). Credentials platform → `CredentialsStep`: guide steps (`agentText`) and the guide link, `SchemaForm` over the credentials schema, submit `POST /accounts/credentials/{platform}`; a `422 validation_failed` maps each violation's `path` (`options.<field>`) to `errors[field]` through the violation catalog; `credentials_invalid` marks `field`; success saves the pending entry and shows the checklist. OAuth platform → `POST /accounts/connect/{platform}` with `{ return_to: window.location.origin + '/oauth-done' }`, then `window.location.assign(authorize_url)`.
  - `DiscoveryChecklist({ connectId, pendingId, discovered, onDone })`: each account with a checkbox (all checked), avatar and name; **Confirm** → `POST /accounts/connect/{connect_id}/confirm` with `{ account_ids }`, then clears the entry and goes to `/accounts` with a toast-like message using the response's counts (`skipped_elsewhere` shown when non-zero); **Cancel** → `DELETE /accounts/pending-connections/{pending_id}` when `pendingId` is set, then clears. A countdown shows the minutes left; at zero the checklist closes with "This connection expired. Start again."
  - `OAuthDonePage`: reads `result`, `connect_id`, `code`; `result=ok` → `GET /accounts/connect/{connect_id}` (gives `discovered`, `pending_id`, `expires_at`), saves the pending entry, shows the checklist; `result=error` → translated `accounts:oauthResult.<code>` for every code in the callback's doc (`access_denied`, `provider_error`, `exchange_failed`, `scope_missing`, `discovery_failed`, `no_accounts`, `platform_unavailable`, `state_invalid`, `account_connected_elsewhere`) with "Back to accounts". After reading, the query string is removed with `history.replaceState`, so `connect_id` does not stay in the address bar or history.
  - The Accounts page (Task 11) resumes a valid pending entry on load and shows the checklist.
- [ ] **Step 1: Write failing tests:** `credentials form renders from the schema with guide`, `violations map to fields`, `credentials_invalid marks the field`, `checklist confirms the chosen accounts`, `cancel deletes the pending row`, `credential check with no pending_id cancels locally`, `reload within 10 minutes resumes the checklist`, `expired entry is dropped`, `another user's entry is ignored`, `oauth start sends return_to and navigates`, `oauth-done ok shows the checklist and strips the query`, `oauth-done error codes are translated (every code)`, `connect_id never written to localStorage` (spy on `Storage.prototype.setItem` and assert no `localStorage` key holds it).
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): connect accounts by credentials and OAuth`.

### Task 13: Target groups

**Files:**
- Create: `web/src/features/accounts/TargetGroupsPage.tsx`, `TargetGroupDialog.tsx`; Modify: `queries.ts`, `routes.tsx` (`/accounts/groups`), the Accounts page header (a "Target groups" link)
- Test: `web/src/features/accounts/TargetGroupsPage.test.tsx`

**Interfaces:**
- Produces: `useTargetGroups(page)`, `useSaveTargetGroup()` (`POST` or `PUT /target-groups/{id}` with `{ name, account_ids }`), `useDeleteTargetGroup()`. Page: `DataTable` of name and account badges; create and edit (edit level) in a dialog with `AccountPicker` over the accounts in the caller's limit; delete with confirmation. Hidden accounts are not shown and the server keeps them; the dialog says "Some accounts in this group are outside your access and stay in it" when the group's server count exceeds the visible IDs (use the field the DTO provides; if it has none, omit the note).
- [ ] **Step 1: Write failing tests:** `create a group`, `edit replaces visible accounts`, `view level sees no edit actions`, `limited delegate picks only accounts in limit`, `403 account_not_in_scope is shown`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): target groups`.

### Task 14: Account CSV import screen

**Files:**
- Create: `web/src/features/accounts/import/ImportPage.tsx`, `ImportReport.tsx`, `queries.ts`
- Modify: `web/src/routes.tsx` (`/accounts/import` under `RequireOwn`)
- Test: `web/src/features/accounts/import/ImportPage.test.tsx`

**Interfaces:**
- Consumes: Task 4 routes, `ProgressBar`, `DataTable`, `usePlatforms`.
- Produces: `useStartImport()` (`POST /accounts/import/{platform}?commit=…` with the `File` as body and `Content-Type: text/csv`; returns `AccountImportDto`), `useImport(id)` (`refetchInterval: 2000` while `processing`, stops after). Page: platform select (credential platforms only), the expected columns (schema field names plus optional `display_name`) with a "Download template" link that builds a header-only CSV in the browser, file picker, "Check file" (dry run), progress (`rows_done` of `rows_total`), the per-row report table (row, status label from `accounts:importStatus.<status>` for every `ImportStatus`, violation codes through the violation catalog, field), counts, `truncated` warning; "Connect valid rows" (commit) re-sends the same `File` held in component state; if the page was reloaded (no `File`), the button reads "Pick the file again to connect". `413 payload_too_large` and `422 csv_invalid` show at once.
- [ ] **Step 1: Write failing tests:** `dry run shows progress then the report`, `commit resends the held file`, `after reload asks to pick the file again`, `bad header answers at once`, `oversized file answers at once`, `polling stops when completed`, `every import status has a label`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): account CSV import with progress`.

### Task 15: Admin platforms, live switch, per-user limits

**Files:**
- Create: `web/src/features/admin/queries.ts`, `PlatformsPage.tsx`, `PublishingPage.tsx`, `UserLimitsDialog.tsx`, `web/src/i18n/en-US/admin.json`
- Modify: `web/src/features/users/UsersPage.tsx` (a "Limits" action per row), `web/src/routes.tsx` (`/admin/platforms`, `/admin/publishing` under `RequireAdmin`)
- Test: `web/src/features/admin/*.test.tsx`, `web/src/features/users/UsersPage.test.tsx`

**Interfaces:**
- Produces: Platforms page: `DataTable` of `AdminPlatformDto` (name with `PlatformBadge` when listed by `GET /platforms`, compiled in, configured, enabled `Switch` → `PUT /admin/platforms/{id}` `{ enabled }`, redirect URI with a copy button, shown only when set). Publishing page: the live switch (`GET/PUT /admin/settings/publishing`) with the native-scheduled count and the warning "{n} posts are already scheduled on the platforms themselves and will still go out"; turning live off asks for confirmation. Limits dialog: the three `UserLimitsDto` fields as integer inputs with `GET/PUT /admin/users/{id}/limits`.
- [ ] **Step 1: Write failing tests:** `enable toggle calls PUT`, `redirect URI copies`, `turning live off asks first and shows the native count`, `limits dialog saves all three fields`, `member cannot reach the admin pages`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): admin platforms, live switch, and user limits`.

### Task 16: Guards, end-to-end spec, docs, and wrap-up

**Files:**
- Create: `web/src/features/noPlatformIds.test.ts`, `web/e2e/d4a.spec.ts`, `docs/plans/2026-11-27-d4a-workspace-accounts-verification.md`
- Modify: `CLAUDE.md` ("Project state": D4a built), `docs/contracts/03. publishing roadmap.md` (an "As built (D4a)" note under Phase D4), `docs/plans/2026-11-01-c3-telegram-agent-deferred.md` (connection guide: web half resolved in D4a Task 1 and 12; stays open for D6)

**Interfaces:**
- `noPlatformIds.test.ts`: reads every `.ts`/`.tsx` under `src/features` and `src/components` except `*.test.*` and `src/test/`, and fails on a string literal equal to a platform ID from `PLATFORM_ORDER` (`email`, `telegram`, `slack`, `discord`, `bluesky`, `mastodon`, `x`, `youtube`, `linkedin`, `facebook`, `instagram`, `threads`, `tiktok`), matching quoted literals only (`'x'` is a literal; the word `x` in an identifier is not).
- `d4a.spec.ts` (opt-in, `POSTIT_E2E`, reuses `login` from `smoke.spec.ts` after moving it to `e2e/support.ts`): admin enables `mock`; the owner connects a mock account by credentials (checklist, confirm), one through mock OAuth, imports a 3-row mock CSV (dry run then connect), creates a target group, grants `member@postit.com` `publish` on one account; the member switches to the owner's workspace, sees only that account, sees no Connect button, and the banner; the owner revokes and the member's tab returns to their own workspace with the notice.
- [ ] **Step 1: Write** `noPlatformIds.test.ts`; run it; fix any hit.
- [ ] **Step 2: Write** `d4a.spec.ts`; run `pnpm exec playwright test --list` to prove it parses.
- [ ] **Step 3: Live check.** If the Playwright plugin's browser tools are available, start `cargo dev` (from `backend/`) and `pnpm dev`, ask the maintainer to sign in as the owner in the driven browser, and walk the D4a "done when" flow (connect by credentials and mock OAuth, import with progress, act as a delegate) on `https://postit.local:44315`, noting any defect and fixing it in this task. Whatever could not be checked (the delegate's session, the full spec run) goes to the verification file.
- [ ] **Step 4: Write** `2026-11-27-d4a-workspace-accounts-verification.md`: (1) run `POSTIT_E2E=1 pnpm exec playwright test e2e/d4a.spec.ts` against the dev stack — why: needs the Zitadel passwords; trigger: before merging D4a. (2) any item Step 3 could not cover, each with why and trigger. Remind the maintainer that `2026-11-27-consolidated-verification.md` has no D4a-triggered items (D2 checks wait for D4c, Papercut for D4d).
- [ ] **Step 5: Run** the full web gate and the backend gates (`fmt`, `clippy`, `check`, `xtask openapi --check`); all pass.
- [ ] **Step 6: Commit** `chore: guard against platform IDs in the web app and document D4a`.

Any agent work found during execution that cannot be finished inside this plan goes to `docs/plans/2026-11-27-d4a-workspace-accounts-deferred.md` with the issue, why, and its trigger; human-only checks go to the verification file.
