# Pending connections: list, cancel, and one cleanup owner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

Status: done 2026-11-24 (proposed and revised after review the same day). Follow-up to B3 (credential connection) and B7 (OAuth connection). Raised by the author while testing B3: a `connect_id` that was not copied cannot be read back.

**Goal:** Let a user see their own unfinished connections and cancel them, without making a `connect_id` recoverable, and leave the hourly `oauth_state_purge` job as the one owner of expired-connection cleanup.

**Architecture:** Two owner-only routes on a new `pending-connections` resource read and end rows of `pending_connections` and pending results of `oauth_states` by row ID, never by `connect_id`, and never confirm. The duplicate expired-connection purge in the daily `data_retention` job goes away.

**Tech Stack:** Rust (axum, sqlx, utoipa), Postgres; `cargo xtask openapi` regenerates the web and mobile clients.

**Spec:** contract `docs/contracts/03. publishing roadmap.md`, sections "Connecting an account with credentials", "Connecting an account with OAuth" (with "Why the confirm step"), and the Accounts rows of the REST surface. This plan amends those sections (Task 4).

## Findings (why this plan looks the way it does)

1. **The rows are in the database; the `connect_id` is not.** A credential check stores a row in `pending_connections` (sealed credential, discovered accounts, `expires_at` = now + 10 minutes, `backend/crates/oauth/src/connect.rs:35`). No row is stored when the credential reaches no account. An OAuth callback stores its result on the `oauth_states` row (sealed tokens, discovered accounts, same 10 minutes, `backend/crates/oauth/src/flow.rs:34`). Both keep only `connect_id_hash` (SHA-256). The `connect_id` itself is never stored, so no endpoint can return it, by design.
2. **It must stay that way, and confirm must stay keyed on `connect_id`.** The confirm step exists so the user who *started* an OAuth flow cannot attach someone else's account: an attacker starts a connection, sends the `authorize_url` to a victim, and the victim's consent lands on the attacker's `oauth_states` row. Only the browser that completed the consent holds the `connect_id`. A route that let the starter list their pending results and confirm one by row ID would hand the victim's account to the attacker. So the new list shows no `connect_id`, no discovered account names, and no account count (a count would tell the attacker the victim consented), and offers no confirm; it only lets the user see and cancel.
3. **Cleanup is already hourly, but it has two owners.** `oauth_state_purge` (`0 0 * * * *`, `backend/crates/oauth/src/jobs.rs:240`) revokes expired OAuth results with tokens, then calls `OAuthStateRepo::purge_expired` (`backend/crates/data/src/oauth_states.rs:508`), a one-line wrapper over `retention::purge_expired_connections` (`backend/crates/data/src/retention.rs:258`), which deletes expired `pending_connections` and token-free `oauth_states`. `backend/crates/oauth/tests/purge.rs` (`purge_job_deletes_expired_rows`) already checks this, so an abandoned credential check keeps its sealed credential for at most about 70 minutes (10 minutes to expire, up to an hour to the next run). The daily `data_retention` job (`0 40 3 * * *`, `backend/crates/jobs/src/maintenance.rs:280`) runs the same purge again: harmless, but two owners for one concern. `oauth_state_purge` is registered unconditionally (`backend/crates/server/src/compose.rs:356`), so the daily backstop is not needed in any build.
4. **The UI never needs to read a `connect_id` back.** The client receives it once (the credential check response, or the OAuth redirect's `connect_id` query parameter) and holds it for the 10-minute checklist. A lost `connect_id` means the user starts again: paste the credentials again, or sign in to the platform again. Cancelling a credential row deletes the sealed credential at once. Cancelling an OAuth result only discards it: the tokens are revoked by the next hourly purge, so a cancel revokes at most 10 minutes earlier than letting the result expire. The D4 and D6 screens are not built yet, so Task 4 writes this into the contract where those phases read it.

## Global Constraints

- Every new route is owner-only (`Capability::OwnerOnly`), like every other connection route: a delegate through `X-Postit-Act-As` gets 403 `owner_only`.
- No response, log line, or error carries a `connect_id`, a hash of one, a credential, a token, or a discovered account's name, remote ID, or count.
- A row of another user answers 404 `not_found`, identical to an unknown ID; unlike the `connect_id` routes, it does **not** end that row (a row ID is not a secret, so a guess must not let anyone cancel someone else's connection).
- A path `id` that is not a UUID answers 422 `validation_failed` (the `ApiPath` extractor, `backend/crates/api/src/json.rs:63`).
- Cancelling an OAuth result never deletes its tokens directly: it discards the result (clears `connect_id_hash`, expires it at now), so `oauth_state_purge` revokes them at the provider first.
- Service and API types name the row ID `pending_id` everywhere (list entry, `PendingView`, `CredentialCheck`, DTOs); repository row types keep `id`.
- One migration file until G1 (memory: single migration policy); this plan needs no schema change.
- After DTO changes, run `cargo xtask openapi` (never `--skip-dart` for the committed result).
- US English in code, comments, and docs.

## Review Focus

1. **A listed OAuth result is the attack surface of Finding 2.** Expect: the list entry carries `pending_id`, `platform`, `connect`, `created_at`, `expires_at` only; no route accepts a row ID for confirm. Test in Task 2 (`list_never_exposes_discovered_accounts`).
2. **Cancel by another user.** Expect: 404 and the owner's row is still listed and still confirmable afterward. Test in Task 2 (`cancel_foreign_row_is_not_found_and_keeps_it`).
3. **Cancel races confirm.** The user confirms on one device while cancelling on another. Expect: exactly one wins; cancel after confirm is 404, confirm after cancel is 404, never a grant plus a revoke. The credential side races two `DELETE`s on one row; the OAuth side races `confirm`'s `lock_result` (`… WHERE connect_id_hash = $1 FOR UPDATE`) against the guarded discard `UPDATE`, and under read committed the loser re-checks its `WHERE` against the winner's row version. Test in Task 1 (`cancel_after_confirm_is_not_found`).
4. **Expired and spent rows.** An expired row, an OAuth state not yet at its callback (no `connect_id_hash`, no `discovered`), and an already discarded result are not listed and cancel answers 404. Test in Task 1 (`list_skips_expired_unfinished_and_discarded`).
5. **Cleanup ownership after the move.** Expect: `data_retention` no longer touches either table, and `oauth_state_purge` reports token-free states and pending credentials separately. Tests in Task 3 (`purge_job_deletes_expired_rows`, `data_retention_leaves_expired_connections`).

---

### Task 1: Repository and service: list and cancel by row ID

**Files:**
- Modify: `backend/crates/data/src/pending_connections.rs` (add `PendingRowSummary`, `list_live`, `delete_owned`)
- Modify: `backend/crates/data/src/oauth_states.rs` (add `list_live_results`, `discard_owned`)
- Modify: `backend/crates/oauth/src/flow.rs` (add `PendingSummary`, `AccountService::list_pending`, `AccountService::cancel_pending`; `pending_id` on `PendingView`)
- Modify: `backend/crates/oauth/src/connect.rs` (`CredentialCheck.pending` carries the row ID)
- Test: `backend/crates/data/tests/pending_connections.rs`, `backend/crates/data/tests/oauth_states.rs`, `backend/crates/oauth/tests/oauth_confirm.rs`

**Interfaces:**
- Produces:
  - `postit_data::pending_connections::PendingRowSummary { id: Uuid, platform: PlatformId, created_at, expires_at }`, defined in `pending_connections.rs` and returned by both repositories; never the discovered JSON or a count of it.
  - `PendingConnectionRepo::list_live(conn, scope: &OwnerScope, now: DateTime<Utc>) -> Result<Vec<PendingRowSummary>, DataError>`: rows with `owner_id = scope.owner AND expires_at > now`, newest first.
  - `PendingConnectionRepo::delete_owned(conn, scope: &OwnerScope, id: PendingConnectionId, now) -> Result<bool, DataError>`: one `DELETE … WHERE id = $1 AND owner_id = $2 AND expires_at > $3`; `true` when a row went.
  - `OAuthStateRepo::list_live_results(conn, user: UserId, now) -> Result<Vec<PendingRowSummary>, DataError>`: rows with `user_id = user AND connect_id_hash IS NOT NULL AND expires_at > now`, newest first.
  - `OAuthStateRepo::discard_owned(conn, user: UserId, id: OAuthStateId, now) -> Result<bool, DataError>`: one guarded `UPDATE oauth_states SET connect_id_hash = NULL, expires_at = LEAST(expires_at, $now) WHERE id = $1 AND user_id = $2 AND connect_id_hash IS NOT NULL AND expires_at > $now`, the same effect as `discard` limited to a live result of `user`; `true` when a row changed. Tokens and `remote_user_id` stay for the purge to revoke.
  - `PendingSummary { pending_id: Uuid, platform: PlatformId, connect: ConnectKind, created_at, expires_at }` in `postit-oauth`.
  - `AccountService::list_pending(&self, scope: &OwnerScope) -> Result<Vec<PendingSummary>, AccountError>`: both lists merged, sorted by `created_at` descending.
  - `AccountService::cancel_pending(&self, scope: &OwnerScope, pending_id: Uuid) -> Result<(), AccountError>`: tries `PendingConnectionRepo::delete_owned`, then `OAuthStateRepo::discard_owned`; neither changing a row is `AccountError::NotFound`. Both service methods call `scope.require(Capability::OwnerOnly)` first.
  - `PendingView.pending_id: Uuid` (the row ID of the credential row or the OAuth state).
  - `CredentialCheck.pending: Option<(ConnectId, PendingConnectionId, DateTime<Utc>)>`: the row ID sits inside the `Option`, since no row exists when the credential reaches no account.

- [ ] **Step 1: Write the failing repository tests** in the two data test files: `list_live_returns_only_owner_live_rows` (two owners, one expired row: owner A sees one row, B sees theirs), `list_live_results_skips_unfinished_and_discarded` (a state before callback, a discarded result, a live result: only the live one), `delete_owned_refuses_foreign_and_expired` (returns `false`, row still present), `discard_owned_refuses_foreign_expired_and_discarded` (returns `false`, `connect_id_hash` unchanged; on the owner's live row returns `true`, `connect_id_hash` is NULL, `grant_tokens` kept).
- [ ] **Step 2: Run them to see them fail.** Run: `cargo test -p postit-data --test pending_connections --test oauth_states` (DB env per `test-env-gotchas` memory). Expected: compile errors for the missing functions.
- [ ] **Step 3: Implement `PendingRowSummary` and the four repository functions** with `sqlx::query!` macros, then `./db.ps1 prepare` to refresh the offline cache.
- [ ] **Step 4: Run the repository tests.** Same command. Expected: PASS.
- [ ] **Step 5: Write the failing service tests** in `backend/crates/oauth/tests/oauth_confirm.rs`: `list_pending_merges_both_kinds` (one credential check plus one OAuth result: two summaries with the right `connect` kinds, newest first), `cancel_credential_row_deletes_it` (then `confirm` with its `connect_id` is `NotFound`), `cancel_oauth_result_discards_for_revoke` (row keeps `grant_tokens`, `connect_id_hash` is NULL, a later `run_oauth_state_purge` revokes it once against the mock provider), `cancel_after_confirm_is_not_found`, `list_skips_expired_unfinished_and_discarded`, `delegate_cannot_list_or_cancel` (both `AccountError::Scope`), `pending_view_and_check_carry_pending_id` (the `pending_id` from `pending` and from `start_credentials` equals the listed `pending_id`; a check that reaches no account has `pending: None`).
- [ ] **Step 6: Run them to see them fail.** Run: `cargo test -p postit-oauth --test oauth_confirm`. Expected: missing `list_pending` and `cancel_pending`.
- [ ] **Step 7: Implement `PendingSummary`, `list_pending`, `cancel_pending`** in `flow.rs` next to `pending`, add `pending_id` to `PendingView`, and move the row ID into `CredentialCheck.pending` (`connect.rs:100`). `cancel_pending` takes no explicit lock: each repository call is one guarded statement, so it serializes with `confirm`'s `take` (credential row) and `lock_result` (OAuth result) on the row lock, and the loser's `WHERE` no longer matches.
- [ ] **Step 8: Run the service tests.** Expected: PASS.
- [ ] **Step 9: Commit.** `git commit -m "feat: list and cancel pending connections by row ID"`

### Task 2: REST routes

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs` (handlers `list_pending`, `cancel_pending`)
- Modify: `backend/crates/api/src/routes/mod.rs:140-160` (register in the accounts router; `/accounts/pending-connections` is a static segment, which axum ranks above `/accounts/{id}`, so registration order does not matter)
- Modify: `backend/crates/api/src/dto_accounts.rs` (`PendingSummaryDto`; `pending_id` on `PendingConnectionDto` and `CredentialCheckResponse`)
- Modify: `backend/crates/api/src/openapi.rs` (both paths, next to `accounts::pending` at line 172)
- Test: `backend/crates/api/tests/oauth.rs`, `backend/crates/api/tests/oauth_leak.rs`
- Regenerated: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/`

**Interfaces:**
- Consumes: `AccountService::list_pending`, `AccountService::cancel_pending`, `PendingSummary`, `PendingView.pending_id`, `CredentialCheck.pending` (Task 1).
- Produces:
  - `GET /api/v1/accounts/pending-connections`, `operation_id = "list_pending_connections"`, 200 `Vec<PendingSummaryDto>`.
  - `DELETE /api/v1/accounts/pending-connections/{id}`, `operation_id = "cancel_pending_connection"`, 204; 404 `not_found`; 422 `validation_failed` for a non-UUID `id`.
  - `PendingSummaryDto { pending_id: Uuid, platform: String, connect: ConnectKindDto, created_at, expires_at }`. Its doc comment states that the entry cannot be confirmed and that a lost `connect_id` means starting the connection again.
  - Error descriptions copy the existing `pending` route's 401/403 (`owner_only` included)/404/429/500/503 list, plus 422 on the `DELETE`.
  - `pending_id: Uuid` on `PendingConnectionDto` (`GET /accounts/connect/{connect_id}`, from `PendingView.pending_id`), and `pending_id: Option<Uuid>` on `CredentialCheckResponse` (`POST /accounts/credentials/{platform}`, absent with `connect_id` when the credential reaches no account), so the checklist can cancel without guessing. Safe: only the holder of the `connect_id` sees it, and it cannot confirm.

- [ ] **Step 1: Write the failing API tests** in `tests/oauth.rs`: `list_pending_connections_shows_own_rows`, `cancel_pending_connection_returns_204_then_404`, `cancel_pending_connection_rejects_non_uuid` (422 `validation_failed`), `cancel_foreign_row_is_not_found_and_keeps_it` (B cancels A's ID: 404; A's list still has it; A's confirm with the `connect_id` still succeeds), `pending_connection_routes_are_owner_only` (act-as at `publish`: 403 `owner_only`), `credential_check_and_pending_read_return_pending_id`. In `tests/oauth_leak.rs`: `list_never_exposes_discovered_accounts` (the response body contains none of the discovered `remote_id`s or names, no `connect_id`, no `connect_id_hash` hex, and no count field).
- [ ] **Step 2: Run to see them fail.** Run: `cargo test -p postit-api --test oauth --test oauth_leak`. Expected: 404 for the unregistered routes.
- [ ] **Step 3: Implement the DTOs, handlers, and route registration;** add both paths to the utoipa document.
- [ ] **Step 4: Regenerate clients.** Run: `cargo xtask openapi`, then `cargo xtask openapi --check`. Expected: the check passes with the regenerated files staged.
- [ ] **Step 5: Run the API tests and `cargo test -p postit-api --test openapi`.** Expected: PASS.
- [ ] **Step 6: Commit** the code and the regenerated files. `git commit -m "feat: add pending-connections list and cancel routes"`

### Task 3: One owner for expired-connection cleanup

**Files:**
- Modify: `backend/crates/oauth/src/jobs.rs` (`run_oauth_state_purge` calls `retention::purge_expired_connections` directly; `PurgeReport` at line 53 gains `pending_purged: u64`, and `purged` now counts token-free states only; the doc comments of both follow)
- Modify: `backend/crates/data/src/oauth_states.rs` (remove `OAuthStateRepo::purge_expired`, line 508, the one-line wrapper; update the module doc if it names it)
- Modify: `backend/crates/data/src/retention.rs:251` (doc comment: the sweep is run by `oauth_state_purge`, not the retention job)
- Modify: `backend/crates/jobs/src/maintenance.rs:280-296` (drop the `purge_expired_connections` call and its `oauth_states` and `pending_connections` log fields from `data_retention`)
- Test: `backend/crates/oauth/tests/purge.rs`, `backend/crates/jobs/tests/maintenance.rs`

**Interfaces:**
- Consumes: `retention::purge_expired_connections(conn, now) -> Result<(u64, u64), DataError>` (unchanged signature; `retention_plan03.rs` tests it directly and needs no change).
- Produces: `PurgeReport { revoked, skipped, purged, pending_purged }`; log line `"oauth states purged"` gains `pending_purged`.

- [ ] **Step 1: Write the failing tests.** In `tests/purge.rs`, change `purge_job_deletes_expired_rows` from `report.purged == 2` to `report.purged == 1` and `report.pending_purged == 1`. In `crates/jobs/tests/maintenance.rs`, add `data_retention_leaves_expired_connections` on the pattern of `data_retention_purges_expired_idempotency_keys` (line 127): insert an expired token-free `oauth_states` row and an expired `pending_connections` row, run `data_retention`, expect both rows still present.
- [ ] **Step 2: Run to see them fail.** Run: `cargo test -p postit-oauth --test purge` and `cargo test -p postit-jobs --test maintenance`. Expected: no `pending_purged` field; the retention test finds both rows deleted.
- [ ] **Step 3: Move the call** as listed under Files; keep the order "revoke expired results, then purge", so a result with tokens is never deleted before its revoke.
- [ ] **Step 4: Run** the two commands from Step 2 and `cargo test -p postit-data --test retention_plan03`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "refactor: make oauth_state_purge the one expired-connection purge"`

### Task 4: Contract and docs

**Files:**
- Modify: `docs/contracts/03. publishing roadmap.md`: the Accounts rows of the REST table (two new rows, owner); the credential flow step 3 and the OAuth flow step 5 (the list and cancel; expired rows purged by the hourly `oauth_state_purge`; an OAuth cancel discards, so the revoke happens on the next hourly run); "Why the confirm step" (add Finding 2's reasoning: the list shows no accounts or account count and offers no confirm); the D4 section near the `/oauth-done` bullet and the D6 section (client rules below).
- Modify: `docs/plans/2026-10-23-b3-credential-connection.md` and `docs/plans/2026-10-28-b7-oauth-connection.md` only if they state that `data_retention` purges pending connections; point to this plan.

Client rules to write into D4 and D6:
- Keep the `connect_id` only for the checklist's lifetime: in `sessionStorage` on the web (so a reload inside 10 minutes resumes the checklist), in memory on mobile. Never in `localStorage`, logs, analytics, or crash reports.
- The checklist's **Cancel** calls `DELETE /accounts/pending-connections/{id}` with the `pending_id` from the credential check response or from `GET /accounts/connect/{connect_id}`. A credential check that reached no account has no `pending_id` and nothing to cancel.
- The Accounts screen may show "Unfinished connection to {platform}, expires in {n} min" from the list, with **Cancel** and **Start again**, never **Confirm**.

- [ ] **Step 1: Edit the contract and plans** as listed.
- [ ] **Step 2: Run the gates.** `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`. Expected: all pass.
- [ ] **Step 3: Update this plan's Status line** to done with the date, and the project `CLAUDE.md` "Built so far" line if it lists follow-ups.
- [ ] **Step 4: Commit.** `git commit -m "docs: document pending-connection list, cancel, and cleanup"`
