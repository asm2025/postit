# B6 Delegation — design

Plan 03 phase B6. Source of truth: `!ref/plans/03. publishing roadmap.md` (Phase B6, REST surface,
"Acting through a delegation", audit events, the `delegation_expiry` job) and
`!ref/plans/01. vision and architecture.md` (Delegation, Identity data model, user deletion).
This document records how B6 fits the code as it stands after B4 and B5, and the calls made
where the plans leave room. It does not repeat the plans; read those sections first.

## Intent

An owner lets another active postit user work in their workspace at a level (`view`, `edit`,
`publish`), optionally limited to chosen accounts, with an optional expiry. The delegate sends
`X-Postit-Act-As: <owner id>` and every existing scoped route then acts in the owner's
workspace with exactly that level and limit. B2 already built the access model
(`OwnerScope`, `Access::Delegated`, `require`, `account_filter`, the whole-object rule in the
post and job repositories); B6 makes it reachable: the tables, the service that creates and
ends grants, resolution of the header, the routes, and proof at the service and HTTP layers.

B6 unblocks C1 (contacts follow the capability table), D1, and D3.

## Goals

- `delegations` and `delegation_accounts` tables, exactly as plan 01 describes them.
- `postit-identity` delegation service: grant, change (level, account limit, expiry), revoke,
  leave, expire; act-as resolution.
- `postit-api`: the `Scope` extractor resolves `X-Postit-Act-As`; the B6 routes from the REST
  surface table; `owner_only` on owner-only, self, and admin routes.
- Lifecycle hooks: disabling, deleting, disconnecting.
- Audit events `delegation_granted`, `delegation_changed`, `delegation_revoked`,
  `delegation_left`, `delegation_expired`.
- `delegation_expiry` cron job (every 5 minutes, `identity`).
- The test list from the roadmap's B6 entry.
- Regenerated contract: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/`.

## Non-goals

- No web or mobile screens. The workspace switcher and the Delegations screens are D4 (web)
  and D6 (mobile); B6 only regenerates the clients.
- No in-app notifications. They arrive with `postit-notify` in D1 (the roadmap says so for
  delegation changes). The "empty account limit" notification to both parties is also D1; it is
  recorded in `docs/superpowers/plans/2026-10-26-b6-delegation-deferred.md` with trigger D1.
- No new owned-content repositories. Target groups, writing styles, campaigns, contacts, inbox,
  recurrences, and the brand profile already filter through `OwnerScope` from B2; B6 checks
  their capabilities against plan 01's table and fixes any mismatch, nothing more.
- No re-delegation in any form.

## Anti-goals

- A delegate reaching anything beyond the grant's level and account limit, by any route.
- A stale grant still working: revocation, expiry, and a disabled or deleting party must take
  effect on the very next API request. No caching of resolution. Signed media URLs a delegate
  obtained before the grant ended keep working until they expire (`media.view_url_ttl`,
  10 minutes; `media.public_url_ttl`, 1 hour); they are capability links that platforms fetch
  anonymously, so B6 documents this window in `docs/security.md` instead of binding URLs to
  grants. (My call.)
- A client believing it acted in another workspace when it did not: a header on a route that
  does not act in a workspace is refused, never ignored.
- Revealing who else uses postit: lookup returns nothing for partial matches and for an
  ambiguous email, with the same response shape as no match.

## Constraints

- Single migration policy: edit `backend/crates/data/migrations/0001_init.sql`, no new file.
- Owned-table conventions: `owner_id`, `created_*`, `updated_*`, unique `(owner_id, id)`,
  composite FKs; actor columns (`created_by`, `updated_by`, `revoked_by`) `ON DELETE SET NULL`.
- Quality gates from the root `CLAUDE.md`, plus `cargo xtask openapi --check`.
- Deferred-file scan at plan start: no existing `*-deferred.md` entry has a B6 trigger.

## Decisions

### Schema

- `delegations` per plan 01: `id`, `owner_id` (FK users, cascade), `delegate_id` (FK users,
  cascade, so deleting either user removes the grant), check `owner_id <> delegate_id`,
  `level` (text enum `view|edit|publish`), `all_accounts bool`, `expires_at` nullable,
  `ended_at` nullable, `ended_reason` (`revoked|left|expired`, set iff `ended_at` set),
  `revoked_by` nullable actor FK, owned-table columns. Partial unique index on
  `(owner_id, delegate_id) WHERE ended_at IS NULL`.
- `delegation_accounts(owner_id, delegation_id, account_id)` with composite FKs
  `(owner_id, delegation_id)` to `delegations` and `(owner_id, account_id)` to
  `connected_accounts`, both cascading, primary key `(delegation_id, account_id)`. Repeating
  `owner_id` is what lets the database reject an account the grantor does not own.
- Ended grants are kept (history for the owner's list and for audit); `data_retention` does not
  purge them in B6.

### Resolution

- `postit-identity` exposes `resolve_act_as(actor, owner) -> Result<OwnerScope, ActAsError>`:
  one query joining `delegations` with both users' rows in `users` (never the principal cache),
  requiring `ended_at IS NULL`, `expires_at IS NULL OR expires_at > now()`, and both users
  `active`; it loads the account limit in the same round trip.
- Header value that is not a UUID, names the caller themselves, names nobody, or has no valid
  grant: 403 `delegation_invalid`. Same code and body for every case, so the header cannot probe
  which user IDs exist. (Self act-as is refused, not treated as the own workspace: my call.)
- An empty `Only([])` limit is a valid scope (posts and media only), per plan 01.

### Header handling in the API

- `Scope` is the only extractor that accepts `X-Postit-Act-As`. `Auth`, `ActiveUser`, and
  `RequireAdmin` refuse a request carrying it with 403 `owner_only`. Scope builds on an
  internal non-refusing variant of `ActiveUser`. Token checks run first, so a missing or bad
  token is still 401.
- Routes with no auth extractor (probes, `/auth/config`, docs, `/public/*`) keep a narrowed
  `reject_act_as` layer on just those route groups, now answering 403 `owner_only`, still
  charged to the per-IP bucket as `api/tests/hardening.rs` asserts today. The global layer goes;
  the hardening tests are updated to the new split rather than deleted. (My call.)
- Consequence: every self route (`/me*`, `/me/workspaces`, `/delegations/received*`,
  `/users/lookup`, preferences) and every admin route refuses act-as with `owner_only`. The D4
  web middleware must therefore add the header only on workspace routes; this is noted in the
  generated contract's route descriptions. (My call; the alternative, ignoring the header on
  self routes, is the "silently ignored" failure the current middleware comment rejects.)
- Routes whose action is owner-only (`/delegations*` owner side, account connect, import,
  disconnect, rename, check, credentials) keep taking `Scope` and call
  `scope.require(Capability::OwnerOnly)`, which maps to 403 `owner_only`; the insufficient-level
  case maps to 403 `capability_required`. B6 audits every existing route for this mapping.

### Service rules

- All grant-changing operations take `&OwnerScope` and call `require(OwnerOnly)` first, so a
  delegated scope can never reach them (this is how "no re-delegation" is enforced, plus a test).
  Leaving takes the caller's own `UserId` as delegate.
- **Account limit input** (grant and PATCH alike): `account_ids` absent means all accounts on
  grant and "unchanged" on PATCH; present means a limit to exactly those. Each ID must be the
  owner's account and not `disconnected` (422 `validation_failed` otherwise). An empty list is
  accepted on both: plan 01 treats "posts and media only" as a valid state, so an owner may
  choose it deliberately. PATCH switches back to all accounts with `all_accounts: true`;
  sending `all_accounts: true` together with `account_ids` is 422. (My call.)
- **Grant** `{ delegate_id, level, account_ids?, expires_at? }`: delegate must exist, be
  `active`, and not be the owner; `expires_at` must be in the future. In one transaction: end any live-but-expired grant for the pair with reason
  `expired` (and audit it), then insert. A live unexpired grant for the pair already existing:
  409 with a new code `delegation_exists` (change it with PATCH instead).
- **Change** `PATCH { level?, account_ids? | all_accounts?, expires_at? (nullable to clear) }`:
  locks the grant `FOR UPDATE`; refused with 404 for an ended or expired grant; one audit event
  with before and after values in `details`.
- **Revoke** (owner, `DELETE /delegations/{id}`) and **leave** (delegate,
  `DELETE /delegations/received/{id}`): set `ended_at`, `ended_reason`, and `revoked_by` for
  revoke; idempotent on an already-ended grant (204, no second audit event). A grant that has
  expired but that the job has not ended yet is ended with reason `expired` and one
  `delegation_expired` event (actor null), and the call still returns 204. (My call.)
- **Expire**: the `delegation_expiry` cron ends every grant with `expires_at <= now()` and
  `ended_at IS NULL` in batches (`FOR UPDATE SKIP LOCKED`), one audit event each, actor null.
- Concurrent owner and delegate edits on a post producing 412 `version_conflict` is already a
  B2 repository behavior; B6 adds the test through a resolved delegated scope.

### Lifecycle

- **Disable** either user: no row change. Resolution reads `users.status`, so the grant stops
  working immediately and works again on re-enable (unless it ended or expired meanwhile). The
  owner's and delegate's lists show the grant with the other party's status.
- **Delete** either user: the `deleting` status stops resolution at once; the `delete_user` job's
  cascade removes grants in both directions through the FKs. `deletion.rs` gains a test.
- **Disconnect** an account: `AccountRepo::disconnect` deletes its `delegation_accounts` rows in
  the same transaction (the account row stays as `disconnected`, so the FK cascade does not fire).
  A reconnected account does not rejoin any limit. A limited grant left with no accounts stays
  live; its DTO reports `account_ids: []`.

### Routes (shapes; names left to the builder)

- `GET /me/workspaces`: own workspace first, then each live, unexpired received grant whose
  owner is active: owner id and display name, level, all-accounts flag, account IDs.
- `GET /delegations` (owner, paginated `ResultSet`): live grants by default, `?include_ended`
  for history; each with delegate id, display name, status, level, account limit, expiry,
  ended fields, created and updated actors.
- `POST /delegations`, `PATCH /delegations/{id}`, `DELETE /delegations/{id}`.
- `GET /delegations/received` (self), `DELETE /delegations/received/{id}` (self).
- `GET /users/lookup?email=`: exact match on the normalized email among `active` users with
  `email_verified`; returns `{ user: { id, display_name } | null }` (200 either way). Never the
  caller themselves (returns null). Its own per-user rate limiter,
  `[rate_limit.lookup]` defaulting to 10 per minute with a burst of 5, charged on every call.
- New stable error code `delegation_exists` (409). `delegation_invalid` (403) joins the error
  enum. `owner_only` and `capability_required` already exist; a row outside the account limit
  keeps its existing 404 `not_found` mapping, which hides whether it exists. A whole-object
  refusal (`DataError::AccountNotInScope`: a visible post, job, or recurrence that touches
  accounts outside the limit) is 403 `account_not_in_scope`, the code plans 01 and 03 define.

### Audit

- `owner_id` = grant owner, `subject_user_id` = delegate, `actor_user_id` = whoever acted
  (null for the expiry job); `details` carries `delegation_id`, level, all-accounts flag,
  account IDs, expiry, and for changes the before and after values. No other user IDs except
  in keys ending `_user_id`.

## Tests

Everything in the roadmap's B6 test list, placed as follows:

- `postit-identity` (DB-backed): grant, change, revoke, leave, expiry job, regrant over an
  expired grant the job has not ended, `delegation_exists`, self and inactive parties,
  re-delegation refused, account IDs of another owner rejected by the composite FK, resolution
  for valid, revoked, expired, self, unknown, disabled-owner, disabled-delegate, and deleting
  cases, a level change and a revocation taking effect on the next resolution.
- `postit-data`: the four-caller isolation matrix re-run with scopes produced by real
  resolution; the whole-object rule (an `edit` delegate refused deleting a post with live
  history and allowed on a draft; edit, delete, cancel, reschedule, and recurrence changes with a
  limit that does and does not cover every override and delivery); concurrent owner and
  delegate edits giving `version_conflict`; disconnect removing the account from limits.
- `postit-api` (HTTP): act-as on scoped routes (accounts, media) at each level and limit;
  `delegation_invalid` for every invalid header; `owner_only` for act-as on admin, self, and
  owner-only routes; the delegation and workspace routes; lookup exact match, partial match,
  duplicate verified email, self, and rate limit; the no-secret-leak harness over the new
  responses.

## Left to the builder

- Module and type names, DTO field names, file layout inside the crates.
- Batch size of the expiry job, ordering of list responses.
- How the extractor split (`ActiveUser` refusing vs. an internal variant) is coded.
- Whether the lookup limiter reuses `limit.rs`'s keyed limiter (expected) and its config key name.
