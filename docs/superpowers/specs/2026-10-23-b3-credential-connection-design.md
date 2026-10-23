# B3: Account connection with credentials (design)

Date: 2026-10-23. Source: plan 03 (`!ref/plans/03. publishing roadmap.md`), phase B3, plus the sections it links to: Connecting an account with credentials (including CSV import and the email rule), Credential refresh (pasted credentials), Disconnecting an account, the Accounts rows of the REST surface, and the audit events for accounts. Plan 01 covers grants, `pending_connections`, the dependency order, and the no-secret-leak rule. This document does not repeat the plan. It records intent, the decisions the plan left open, and what the builder decides.

The user skipped the question round. Every decision below is the designer's call unless the plan states it, and each call is marked **(call)**.

## Intent

B3 is the first time a user's own platform credential enters postit. A user pastes a Telegram bot token, a Slack or Discord webhook URL, a Bluesky app password, a Mastodon token, or an SMTP login; postit checks it against the platform, shows the accounts it reaches, and connects the ones the user picks. Nothing in this crate knows any platform: everything comes from the agent's credentials schema and its `verify_credentials`. B5 (agent kit and mock), B7 (OAuth, which reuses the confirm step and the account routes), C2–C5 (the credential agents), and D4/D6 (the connection screens) all build on it.

A mistake here leaks a secret into a log or a response, attaches an account to the wrong postit user, or reveals who else connected a remote account.

## Goals

- Everything under plan 03 phase B3, meeting its exit criteria: any credential agent can connect, list, check, and disconnect accounts, with no platform code in `postit-oauth` and no secret in any output.
- The routes marked B3 in the REST surface, in the OpenAPI contract, with the TypeScript and Dart clients regenerated.
- The plan's B3 tests, plus the isolation matrix for every route and service method.

## Non-goals

- OAuth (B7): no `oauth_states` flow, no refresh cron, no provider revocation for OAuth, no platform event route. `PgCredentialStore` returns `Credentials::Fields` only; `store_refreshed` returns an error for a credentials grant. B7 adds the OAuth arm.
- The mock agent and the conformance suite (B5). B3 tests use a small test agent inside `postit-oauth` (see Testing).
- Real agents (C2–C5). No production build registers an agent yet, so in a running server every B3 route answers "platform not found" until B5 registers the mock.
- Notifications. `postit-notify` arrives in D1; B3 writes audit events only. D1 adds the critical notification on `needs_reconnect`.
- Delegation resolution (B6). The `Scope` extractor stays "own workspace only"; B3 routes are written against `OwnerScope` and its capabilities, so B6 changes nothing here except the extractor. Removing a disconnected account from delegation account limits is B6's (the `delegation_accounts` table does not exist yet).
- Deleting native-scheduled uploads on disconnect. No credential agent schedules natively. B3 cancels pending and parked deliveries and returns how many of the account's deliveries are still native-scheduled on the platform (a new per-account count query in `postit-data`, next to the deployment-wide `native_scheduled_counts`), so the response shape is final; deleting them on the platform is D1's.
- Switching the email sending method in place ("switching methods edits that account's credentials"). That needs the email agent and is C2's. B3 surfaces `sending_method_exists` as the data layer reports it.
- Client screens (D4, D6).
- The `account_preflight` cron (D1). B3 provides the service call it will use.

## Anti-goals

- A credential, a webhook URL, or any field marked `x-postit-secret` in a log line, a tracing span, an error body, a CSV report, an audit event, or an OpenAPI example.
- A confirm by anyone but the user who started the connection creating anything, or telling them whether the `connect_id` existed.
- A response that names, counts per owner, or otherwise hints at the other owner of an account connected elsewhere.
- The uploaded CSV, or any row of it, written to disk or kept in memory beyond the row being processed.
- Platform-specific code or names in `postit-oauth`.

## Constraints

- Dependency order (plan 01): `oauth` is level 8 and may depend on `config`, `core`, `http`, `data`, `vault`, `jobs` (`config` because the platform catalog resolves `[platforms.<id>]` from `Settings`). It must not depend on `agent-kit`, `media`, or `notify`.
- Schema exists from B2 (`account_grants`, `connected_accounts`, `account_tokens`, `pending_connections`). The single-migration policy applies: any schema change edits `0001_init.sql` in place. **(call)** B3 expects no schema change; the builder may add an index if a query needs one.
- Existing repositories in `postit-data/src/accounts.rs` (`GrantRepo`, `AccountRepo`, `AccountTokenRepo`) and `publishing::deliveries::cancel_for_account` are reused, not duplicated. New queries go in `postit-data` (one owner per concern). B3 changes two of them (see Data-layer changes).
- **Overlap with D1 (call).** Plan 03 D1 lists "the disconnect flow for pending and native-scheduled deliveries" and "the account and publishing audit events". B3 builds the parts that belong to connecting: the four account audit kinds (`account_connected`, `account_disconnected`, `account_needs_reconnect`, `accounts_imported`, added to `AuditEventKind`) and cancelling pending and parked deliveries on disconnect (the existing `cancel_for_account`). D1 keeps native-scheduled deletion on the platform, `account_revoked_by_platform` (B7), and the publishing audit events. Plan 03's D1 bullets are edited to say so.
- Quality gates from `CLAUDE.md`, plus `cargo xtask openapi --check`.

## Decisions

### Crate and seams

- **New crate `postit-oauth`** at `server/crates/oauth/`, named for its larger half as the plan says. It owns `PgCredentialStore` and an `AccountService` (connect, confirm, import, list, get, update, check, disconnect, mark needs-reconnect). `postit-api` depends on it; `postit-server` builds it.
- **Platform catalog seam (call).** The service needs, per platform ID: the agent, its resolved `[platforms.<id>]` config, and whether it is available. Availability today is computed in `postit-server/src/platforms.rs`. B3 moves `Availability`, `availability()`, and `PlatformConfigs` (with `resolve_platform_configs`) down into `postit-oauth` as a `PlatformCatalog { registry, configs }`, keeping the server's startup step as the only caller of resolution. The `enabled` flag is read from the `platforms` row at request time, so an admin switching a platform off takes effect on the next request. Unknown or not-compiled-in platforms answer 404; disabled answers 409 `platform_disabled`; not configured answers 503 `platform_unavailable` (one status per code, so the same code serves a platform that is down); an OAuth platform on a credentials or import route answers 409 `capability_unsupported`.
- **HTTP handles (call).** `postit_core::HttpHandle` has no implementation yet, and agents need `HttpHandles` in both `CredentialCheckContext` and `AgentContext`. `postit-http` gains a dependency on `postit-core` (core is level 1, so this points downward) and implements `HttpHandle` for a new newtype over the standard `reqwest::Client` (the orphan rule forbids implementing it on `reqwest::Client` directly) and for `PublicClient`, with a constructor for `HttpHandles`. B5's agent kit downcasts to these types. Plan 01's dependency list (`http (config)`) is updated to `http (config, core)`.
- **Missing context pieces (call).** `AgentContext` needs `MediaStore`, `MediaHost`, and `LinkSigner`, which arrive in B4 and C2. The service takes them as injected `Arc<dyn …>` values; `postit-server` passes "not available yet" implementations (media calls return `MediaError`, link signing returns URLs under `server.public_url` that 404) defined in `postit-server`, replaced by B4 and C2. `check` only calls `account_info`, so nothing in B3 reaches them.

### Core contract change: the remote account on `AccountRef` (call)

`postit_core::AccountRef` carries only `platform` and the postit `AccountId`, so an agent client bound to a grant that reaches several accounts (a Telegram bot in several channels) cannot tell which one `account_info`, and later `publish`, means. B3 adds `remote_id: String` (the `connected_accounts.remote_account_id`) to `AccountRef`. No agent exists yet, so the change touches only core's own tests and fake agent. Wire-format tests in `core/tests/wire_format.rs` are updated if `AccountRef` appears there.

### Data-layer changes

- **Reconnect keeps the user's settings (call).** `AccountRepo::insert_or_reactivate` today overwrites `display_name`, `avatar_url`, and `test_account` when it reactivates or moves the owner's row. It changes to keep the existing `display_name` and `test_account` (the user may have renamed the account or marked it as a test account) and to update `avatar_url` and `scopes` from the platform.
- **Moved accounts report their old grant.** `AccountUpsert::Moved` gains the previous `grant_id`, so confirm can call `delete_if_unused` on it without a second read.
- **`pending_connections` repository**: insert, and take-by-hash (`DELETE … WHERE connect_id_hash = $1 RETURNING …`, ignoring owner, returning the row with its owner and expiry so the service decides).
- **Per-account native-scheduled count** for the disconnect response.
- **System scope for the worker (call).** `mark_needs_reconnect(scope, grant_id)` takes an `OwnerScope`. D1's worker and `account_preflight` have no user; they build `OwnerScope::own(owner_id)` for the grant's owner, the same pattern B2's system calls use where they need an owner check. B3 states this in the method's docs; no new capability.

### Storing a credential

- **Split (plan 01).** Fields marked `x-postit-secret` go into the sealed `credential` column as a JSON object; the other fields go into `connection_config`. `PgCredentialStore::credentials()` opens the sealed part and rebuilds `CredentialFields` from both. **(call)** `postit-core` gains `CredentialFields::from_parts(plain, secret)` (or an equivalent constructor) so the store does not re-run schema validation on stored values; a schema change between releases surfaces when the agent uses the fields, not as a load failure.
- **Seal contexts.** A pending connection's credential is sealed for its `pending_connections` row; confirm opens it and reseals it for the new grant (`GrantRepo::seal_context`). The plaintext lives only inside the confirm transaction.
- **`connect_id`.** 32 random bytes, base64url without padding, returned once; only its SHA-256 (`emixcrypto`) is stored in `connect_id_hash`. Expiry 10 minutes (plan). **(call)** One `connect_id` per pending row; starting again creates a new row and leaves the old one to expire.
- **Discovered accounts** are stored as `postit_core::Discovered` JSON. The response returns them with their `remote_id`, `display_name`, `handle`, and `avatar_url`. **(call)** Confirm's `account_ids` are those `remote_id` strings, since no postit account ID exists yet. Omitted means all; an unknown ID is ignored and listed back as `unknown`.

### Connect and confirm

1. `POST /accounts/credentials/{platform}`: owner only. Catalog check, then `parse_credentials` against the agent's credentials schema (422 `validation_failed` with every violation), then `verify_credentials` with a `CredentialCheckContext` carrying the agent config, both HTTP handles, and the clock. `InvalidCredentials { field }` becomes 422 `credentials_invalid` with `field` (plan 03 says "the agent's plain-language reason"; **(call)** the reason is the field plus the client's translated per-platform guide, since `AgentError` carries no message and free text from an agent would bypass localization); `Authorization { missing_scopes }` becomes 422 `credentials_invalid` with the missing scopes **(call)**; `RateLimited` becomes 429 `rate_limited`; a transport or platform failure becomes 503 `platform_unavailable` **(call)**. An empty discovery is not an error: it returns `discovered: []` and no `connect_id`, so the client can say "this bot is not an admin anywhere yet" **(call)**.
2. Confirm: one transaction. Delete the pending row by `connect_id_hash` with `RETURNING` (single use for everyone). If it was missing or expired, 404. If its owner is not the caller, 404 (the row is already gone, as the plan requires). Otherwise insert the grant, `insert_or_reactivate` each chosen account, collect `AccountConnectedElsewhere` as skipped (a count only), let `SendingMethodExists` fail the whole confirm with 409 `sending_method_exists` **(call)**, delete any previous grant left with no accounts (`delete_if_unused` on each moved account's old grant), and write `account_connected` per account. If every chosen account was skipped, the new grant is deleted in the same transaction, so no orphan grant exists.
3. **Reconnect.** A pasted credential does not refresh; reconnecting is connect and confirm again. `insert_or_reactivate` already moves the owner's existing row to the new grant, and confirm sets it `active`. **(call)** Confirm's response says per account whether it was `created`, `reactivated`, or `moved`, so the client can say "reconnected".
4. Response: `{ connected: [AccountDto], skipped_elsewhere: n, unknown: [remote_id] }`. Same shape B7 will return for OAuth.

### CSV import

- `POST /accounts/import/{platform}?commit=false|true`, owner only, credential platforms only, body `text/csv`. Parsed as a stream with the `csv-async` crate **(call: maintained library over a hand-rolled parser)** over the request body stream; never written to disk.
- Header row: the credentials schema's field names plus optional `display_name`. Unknown columns fail the whole request with 422 `validation_failed` naming the column **(call)**; missing required columns too. Limits **(call)**: 1 MiB body and 500 data rows by default, configurable under `[accounts]` in `postit-config`; the limit is enforced while streaming.
- Each row runs parse, `verify_credentials`, and (on commit) its own confirm transaction with every discovered account chosen. Rows run one at a time **(call)**: simplest, gentle on platform rate limits, and memory stays one row.
- `display_name` applies only when the row discovers exactly one account; otherwise it is ignored and the row's report says so **(call)**.
- Report per row: `row` (1-based data row number), `status` (`ok`, `invalid_fields`, `invalid_credentials`, `already_connected`, `connected_elsewhere`, `nothing_found`, `platform_error`), violation codes or the credential field at fault, and the created or matched account IDs when committed. **Never** a field value. `already_connected` means every discovered account is already the owner's own active account on another grant or this file's earlier row **(call)**; with `commit=true` such a row still moves accounts to the new credential, matching single confirm.
- One audit event `accounts_imported` with the counts per status and `commit`.

### Account routes

- `GET /accounts` (view, account limit applied, `emixdb` pagination, optional `platform` filter; **(call)** disconnected accounts are hidden unless `include_disconnected=true`, since history screens still need their names), `GET /accounts/{id}` (view). The DTO carries id, platform, remote account ID, display name, avatar URL, status, test flag, grant kind, last checked time, created and updated times. Never the grant's credential or connection config **(call: connection config can hold a webhook's channel name or SMTP host, which is not secret but not needed by clients; leaving it out keeps the DTO safe by construction)**.
- `PATCH /accounts/{id}` with `{ test_account?, display_name? }`, owner only. **(call)** No `If-Match`: accounts carry no version, and both fields are last-writer-wins settings.
- `POST /accounts/{id}/check`, owner only: builds an `AgentContext` with `PgCredentialStore`, calls `account_info`, then `mark_checked`. `Authentication` or `InvalidCredentials` moves the grant and its accounts to `needs_reconnect` (`GrantRepo::set_status`) and writes `account_needs_reconnect`; the response is 200 with the new status, not an error **(call)**, so the client shows "reconnect needed". Display name and avatar changes from `account_info` are not written back **(call: the user may have renamed the account in postit)**. A disconnected account answers 422 `validation_failed` with the detail "the account is disconnected" **(call: no generic conflict code exists, and plan 03 adds none for this)**.
- `DELETE /accounts/{id}`, owner only.
    1. **In-flight deliveries (call).** Plan 03 says in-flight deliveries finish first, but `disconnect` nulls the account's `grant_id`, so a worker mid-publish would lose its credential. B3 refuses with 409 `job_active` while any delivery of the account is claimed or in flight; the client says "a post is being published to this account, try again in a minute". D1 may replace this with waiting.
    2. One transaction: `cancel_for_account`, `AccountRepo::disconnect`, `account_disconnected`. The grant row stays even if it has no account left.
    3. After commit, when the grant has no account left: build a client on `PgCredentialStore` (the grant still exists), call `revoke` (where offered; `Unsupported` is fine), then `delete_if_unused`. The revoke is best effort and logged without secrets; a failure never undoes the disconnect, and the grant is deleted either way. A crash between commit and delete leaves an unused grant that the next disconnect or B7's cleanup removes **(call: no new cron in B3)**.
    4. Response: `{ cancelled_deliveries, native_scheduled_remaining }`, where `cancelled_deliveries` counts only `Cancelled` outcomes.
- `AccountService::mark_needs_reconnect(scope, grant_id)` is the one entry point D1's worker and `account_preflight` will call on an authentication failure.

### Errors

New stable codes used by B3, all already listed in plan 03: `credentials_invalid`, `account_connected_elsewhere` (only as a skipped-count field, never as an error), `sending_method_exists`, `platform_disabled`, `platform_unavailable`, `capability_unsupported`, `owner_only`. They are added to the API error catalog and its OpenAPI schema if not present.

### Rate limiting (call)

`POST /accounts/credentials/{platform}` and `POST /accounts/import/{platform}` call out to platforms on the user's behalf. They get their own per-user `KeyedLimiter` bucket, `credential_checks`, in `RateLimitSettings` (default 10 per minute, burst 5); one import request counts as one. Existing authenticated limits still apply.

## Testing

- **Test agent (call).** `postit-oauth` gets a `testkit` feature with a small credentials agent: a schema with one plain and one secret field, scriptable `verify_credentials` (accounts, invalid credentials, rate limited, failure), scriptable `account_info`, and a revoke counter. B5's mock agent supersedes it; B5 decides whether to delete it.
- The plan's B3 tests: confirm by another user refused and the pending row gone; an expired pending connection creating nothing; an account connected elsewhere skipped without revealing the owner (response, error, and audit checked); a grant reaching several accounts sharing its credential; CSV dry-run and commit reports; a CSV with a bad row committing the others; secrets never present in reports, responses, logs, or audit rows, using the P10 no-secret-leak harness; and the isolation matrix (owner, delegate at each level, delegate limited to other accounts, unrelated user) using constructed scopes at the service layer, as B2 did.
- Also: single-use `connect_id` (second confirm 404); confirm with a subset of `account_ids`; every chosen account skipped leaving no grant; reconnect moving an account and deleting the old grant; `sending_method_exists` on a second email confirm (via the test agent registered under the `email` platform ID); `check` moving the grant to `needs_reconnect`; disconnect cancelling pending and parked deliveries, keeping the row, and freeing the remote key for another owner; disconnect of the last account revoking and deleting the grant, and of one of two accounts keeping it; CSV limits (rows, bytes, unknown column) enforced while streaming; disabled and unconfigured platforms refused; an OAuth platform refused on the credentials and import routes; the `credential_checks` limiter.
- HTTP-level tests in `postit-api` for each route's status codes and problem bodies, and the OpenAPI contract check.

## Left to the builder

- Module layout inside `postit-oauth`, DTO names, and how the service is split.
- Exact new repository method names for `pending_connections` (insert, take by hash) in `postit-data`.
- The CSV config key names and whether the `credential_checks` bucket reuses `RateBucket`.
- Whether `PlatformCatalog` caches `platforms.enabled` (it must not cache for longer than one request).

## Docs

- `docs/configuration.md`: the new `[accounts]` keys and the `credential_checks` rate bucket.
- Plan 01: the `http (config, core)` dependency line, and `config` in the level-8 line for `oauth`.
- Plan 03: the D1 bullets for disconnect and account audit events (see Overlap with D1).
- `CLAUDE.md`: add `oauth` to the crate list and note B3 in the project state.
