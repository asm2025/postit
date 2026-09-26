# P4 — Data layer and identity

> Design spec for plan 02 phase P4 (`!ref/plans/02. foundation.md`). Builds `postit-data`
> and the jobless/mailless slice of `postit-identity`. Approval emails, `user_approved`,
> `delete_user`, and the pending/audit cron jobs are explicitly out of scope — plan 02 P5,
> because `postit-identity` depends on `postit-jobs` and `postit-mail`.

## Context

The workspace is at plan 02 P3: `postit-config` and the foundation `postit-http` are
implemented and tested, dev infra (Docker, Zitadel bootstrap) works. `postit-data`,
`postit-identity`, `postit-jobs`, `postit-mail`, `postit-api`, and `postit-server` are
1-line stub crates. P4 is the next open phase; nothing in plan 03 has started.

Decisions confirmed with the maintainer before writing this spec:

- **JWT/OIDC library:** `jsonwebtoken` + a hand-rolled discovery/JWKS fetcher built on
  `postit-http`, not `openidconnect`. Full control over the verifier rules in plan 01
  (leeway, algorithm allowlist, `alg: none`/HMAC hard rejection, unknown-`kid` refetch rate
  limit) without a heavier crate's own opinions about flows postit doesn't drive itself
  (Flutter runs the auth-code exchange; the server only verifies).
- **Principal cache:** `moka::sync::Cache`. Built-in `invalidate(key)` and `invalidate_all()`
  match plan 01's per-key eviction plus full-clear-on-reconnect requirement exactly.
- **Test-issuer keypairs:** `rsa` + `p256` (RustCrypto), not `openssl`. Pure Rust, no native
  OpenSSL dependency in a workspace that otherwise only uses rustls; test-only via the
  `testkit` feature.
- **Local dev/test database:** an ad-hoc `postgres:18` container (`postgres-18`, port 5432,
  password in `!ref/vault/development/postgres.env`) is available for migrations,
  `#[sqlx::test]`, and `cargo sqlx prepare`. This is separate from the project's own
  `postit-postgres` compose service (port 44340) and is only for iterating on this work.

## Section A — `postit-data`

### Crate additions

- `sqlx` (`postgres`, `runtime-tokio-rustls`, `macros`, `migrate`, `uuid`, `chrono`, `json`
  features).
- `emixdb` (pagination, `ResultSet<T>`), `default-features = false` where it has heavy
  defaults.
- `serde_json` for `audit_events.details` and `idempotency_keys.response_body`.

### Pool

`Db(PgPool)` newtype. Built from `DatabaseSettings` (already implemented in
`postit-config`: `url` — host/port/dbname, never credentials — plus `username`,
`password: RedactedSecret`, `max_connections`). Connection options are assembled with
`PgConnectOptions` from the parsed `url` plus the secret credentials, never a
credential-bearing connection string in a log or error.

Migrations run via `sqlx::migrate!()` at startup, wrapped in
`advisory_lock::xact_lock(&mut conn, key)` — a small reusable helper
(`SELECT pg_advisory_xact_lock($1)`) that this phase introduces for migrations and reuses
for the bootstrap-admin check in `postit-identity`. Later phases reuse it again (vault
rotation, OAuth refresh serialization) without duplicating the pattern.

### Migrations (this phase)

In order:

1. `CREATE EXTENSION IF NOT EXISTS citext` (trusted extension, no superuser needed).
2. `users`: `id`, `oidc_issuer`, `oidc_subject` (unique together), `email citext`,
   `email_verified bool`, `display_name`, `role` (`admin`|`member`), `status`
   (`pending`|`active`|`disabled`|`deleting`), `approved_at`, `approved_by` (nullable FK,
   `ON DELETE SET NULL`), `last_seen_at`, `created_at`, `updated_at`.
3. `audit_events`: `id`, `at`, `actor_user_id`, `owner_id` (nullable), `subject_user_id`,
   `kind`, `ip`, `request_id`, `details jsonb`. User ID columns are plain columns, not FKs
   (audit rows must survive user deletion).
4. `user_preferences`: `user_id` (1:1, FK `ON DELETE CASCADE`), `email_notifications bool`,
   `timezone`, `store_ai_prompts bool`, `last_approval_email_at`.
5. `idempotency_keys`: `id`, `actor_id`, `owner_id`, `key`, `route`, `request_hash`,
   `state` (`in_progress`|`completed`), `response_status`, `response_body jsonb`,
   `expires_at`; unique on `(owner_id, actor_id, key)`. `owner_id` always equals `actor_id`
   in this phase (no delegation exists yet) but the column is present now so plan 03 needs
   no migration to add it.

Committed `.sqlx` query cache from the start (`cargo sqlx prepare`), so
`SQLX_OFFLINE=true` works for the Windows CI job and any machine without a live database.

### `OwnerScope` and `Capability`

```rust
pub struct OwnerScope {
    pub actor: UserId,
    pub owner: UserId,
    pub access: Access,
}

pub enum Access {
    Owner,
    // Access::Delegated { level: Capability, accounts: AccountLimit } — added in plan 03 B2.
}

pub enum Capability {
    View,
    Edit,
    Publish,
}

impl OwnerScope {
    pub fn require(&self, capability: Capability) -> Result<(), ScopeError> { .. }
}
```

Every P4 caller uses `Access::Owner`, so `require()` always succeeds — the type and method
exist now per plan 01's explicit "no call site changes later" instruction, not because this
phase needs real enforcement yet.

### `AuditLog`

The only writer of `audit_events`. `AuditEvent` is built through a constructor that fixes
`kind`, `actor`, `owner`, and `subject`, plus `.detail_user_id(key, id)` (panics in debug /
returns `AuditError` in release if `key` doesn't end in `_user_id`) and `.detail(key, value)`
for plain JSON values. There is no way to attach a `SecretString` — it isn't `Serialize` —
so the "never secrets" rule is partly enforced by the type system, partly by review.

Events written this phase: `user_provisioned`, `user_approved`, `user_disabled`,
`user_enabled`, `role_changed`, `bootstrap_admin_granted`. (`user_deleted` is P5's, with the
`delete_user` job.)

### Repositories

- **`UsersRepo`**: `provision(iss, sub) -> ProvisionOutcome` (inserts with
  `ON CONFLICT (oidc_issuer, oidc_subject) DO NOTHING`, returns whether a row was created,
  runs the bootstrap check inside the same transaction under the advisory lock), `find_by_oidc`,
  `find_by_id`, `list` (admin, paginated via `emixdb::Pagination`/`ResultSet`, search +
  status filter), `set_status`, `set_role`, `touch_last_seen`, `update_profile_claims`. Every
  write that changes `status`, `role`, or deletes a row issues
  `NOTIFY postit_user_changed, '<user id>'` in the same transaction — `postit-data` does not
  know about the principal cache, it just announces the change.
- **`UserPreferencesRepo`**: created 1:1 with the user (same provisioning transaction),
  `get`, `update`, `set_last_approval_email_at`.
- **`IdempotencyRepo`**: `begin(owner, actor, key, route, request_hash)` (inserts
  `in_progress`, unique-violation surfaces as a typed conflict), `complete(id, status, body)`,
  `find`, `purge_expired`. The table and repository exist now; `postit-api` wires the
  `Idempotency-Key` middleware around them in P6.
- **`AuditRepo`**: admin read side, a separate type from `AuditLog` per the "admin
  repositories cannot read owned content" rule (moot in P4 since there's no owned content
  yet, but the type separation is established now). `list` filtered by kind, time range,
  actor, or subject, paginated.
- **Retention purges**: `purge_audit_events(ip_retention, retention)`,
  `purge_expired_idempotency_keys()`. Called by the `data_retention` cron job registered in
  P6/P5 — the query lives here, the scheduling doesn't.

### Tests

`#[sqlx::test]` per repository against the local `postgres-18` container; migrations apply
cleanly from empty; `cargo sqlx prepare --check` passes; concurrent `provision()` calls for
the same `(iss, sub)` create exactly one row; concurrent bootstrap-eligible provisioning
produces exactly one admin; `NOTIFY` fires on status/role changes (asserted via a second
connection's `LISTEN`); retention purges respect the configured windows; no plaintext secret
or token appears in any row (there are none yet in this phase's tables, but the test
harness for this check is established here since `social_account_tokens` etc. reuse it in
plan 03).

## Section B — `postit-identity`

### Crate additions

- `jsonwebtoken`, `moka` (default features — sync cache).
- `postit-http`, `postit-data`, `postit-core`, `postit-config`.
- `serde_json`.
- `testkit` feature: `rsa`, `p256`, and `postit-http` with its own `testkit` feature (for
  `postit_http::testkit::test_server()`).

### Discovery and JWKS

`OidcDiscovery`: fetches `{issuer}/.well-known/openid-configuration` through
`postit_http::execute_traced`, caches the document and the JWKS. Refresh on
`auth.oidc.jwks_refresh_interval`; on an unknown `kid`, refetch immediately but never more
than once per minute (a stored last-refetch timestamp, not a full rate limiter — one
process, one guard). `JwksSource` trait separates the real HTTP fetch from the test
issuer's static/wiremock source, per plan 01's decision to make this a core-adjacent
abstraction.

### Verifier

`jsonwebtoken`-based. Checks, in order: signature against a JWKS key matching the token's
`kid` and `alg`; `alg` is in `auth.oidc.accepted_algorithms` **and** is not `none` and not
an HMAC algorithm regardless of config (hard-coded, not configurable — closes the
`alg: none` / algorithm-confusion class of bug even if someone misconfigures
`accepted_algorithms`); `iss` exact match; `aud` intersects `auth.oidc.audiences`; `exp`/`nbf`
within `auth.oidc.leeway`. Returns a typed `VerifyError` (`BadSignature`, `WrongIssuer`,
`WrongAudience`, `Expired`, `NotYetValid`, `UnacceptedAlgorithm`, `UnknownKid`) — never
echoes the token.

### Claims → `Principal`

One function implementing plan 01's four steps:

1. `moka` lookup on `(iss, sub)`. Hit → return the cached `Principal`.
2. Miss → `UsersRepo::provision`, which also runs the bootstrap check
   (`auth.bootstrap.admin_email` with `email_verified = true`, or `admin_subject`, granted
   only while no active admin exists, under the advisory lock) and records
   `user_provisioned` / `bootstrap_admin_granted` through `AuditLog`.
3. Refresh profile claims from the token; when a configured claim is missing and
   `auth.oidc.userinfo` allows it (`fallback`/`always`), call the discovery document's
   `userinfo_endpoint` with the same bearer token through `postit-http`, and ignore the
   response if its `sub` differs from the token's. `display_name` falls back
   `name → preferred_username → email → subject`.
4. Build `Principal { user_id, role, status }` and cache it (TTL
   `auth.principal_cache_ttl`).

`last_seen_at` is written only on a cache miss (at most once per TTL per process), matching
plan 02's stated rate.

### Principal cache and cross-process eviction

Two small `moka::sync::Cache`s, both TTL `auth.principal_cache_ttl`, both owned by this
module:

- `identity_index: Cache<(String, String), UserId>` — `(iss, sub) → UserId`, populated on
  every provision/lookup.
- `principals: Cache<UserId, Principal>` — the actual cached `Principal`.

Claims transformation step 1 becomes: look up `identity_index`, then `principals`; either
miss falls through to `UsersRepo`. The `LISTEN postit_user_changed` background task's
notification payload is a user id, so it invalidates `principals` directly by that key —
no reverse lookup needed, and `identity_index` entries simply expire on their own TTL
instead of being tracked for eviction (they hold no status/role, so a stale one costs at
most one extra `principals` miss, not stale authorization data).

The task holds a dedicated `PgConnection` for `LISTEN`. On a dropped/reconnected listener
connection it calls `principals.invalidate_all()` instead of trying to replay missed
notifications, per plan 01's explicit fallback; `identity_index` is left alone since it
carries no access-control data.

### User administration service (P4 slice)

- `approve(user_id)`: `pending → active`, sets `approved_at`/`approved_by`, writes
  `user_approved`. (The email send is P5 — this phase only flips the status and audits it.)
- `disable(user_id)` / `enable(user_id)`: `active ↔ disabled`, refusing on the last active
  admin (`disable` only), writes `user_disabled`/`user_enabled`.
- `change_role(user_id, role)`: allowed on `active`/`disabled` users, refusing the
  last-active-admin demotion, writes `role_changed`.
- Any other transition (e.g. `disabled → pending`, anything on `deleting`) returns a typed
  `IdentityError::InvalidTransition` — `postit-api` maps this to 422 `validation_failed` in
  P6.
- **Not implemented this phase:** `DELETE /me`, `DELETE /users/{id}`, rejecting a pending
  user (a delete), `purge_pending_users`, `delete_user`. All need `postit-jobs`'s outbox and
  `postit-mail`'s `send_email`, per plan 02's explicit P5 assignment.

### Test issuer (`testkit` feature)

Generates one 2048-bit RSA keypair (`rsa` crate) and one P-256 keypair (`p256` crate) once
per issuer instance. Mints RS256 and ES256 JWTs with `jsonwebtoken`'s `EncodingKey` built
from the generated keys' PEM/DER encodings. Serves `/.well-known/openid-configuration` and
the JWKS document over a `postit_http::testkit::test_server()` (wiremock), so verifier and
identity tests exercise the same HTTP path production code uses.

### Tests

- **Verifier:** wrong issuer, wrong audience, expired, not-yet-valid, bad signature,
  `alg: none` rejected even if present in config, HMAC-with-the-public-key rejected,
  unknown-`kid` triggers exactly one refetch per minute, key rotation (old `kid` still valid
  until its key is dropped from the JWKS, new `kid` works once fetched).
- **Identity:** concurrent first sign-in for the same `(iss, sub)` creates exactly one user;
  a `pending` user's requests are rejected everywhere except `GET /me`'s underlying check (the
  route itself is P6, but the `Principal`/status check this phase provides is what it will
  call); a `disabled` user is rejected the same way; bootstrap grants admin only while no
  active admin exists; concurrent bootstrap-eligible sign-ins produce exactly one admin;
  `admin_email` is ignored when `email_verified` is false; a userinfo response is used only
  when its `sub` matches; profile claims load from userinfo when missing from the access
  token; the display-name fallback chain; every rejected status transition; the last-admin
  guard on both disable and demote; a disable in one process evicting the cache in a second
  process via `LISTEN` within about a second; a dropped listener connection falling back to
  a full cache clear on reconnect.

## Out of scope for P4 (confirmed against plan 02)

- `postit-jobs`, `postit-mail`, approval emails, `user_approved` email, `delete_user`,
  `purge_pending_users`, `audit_retention` cron scheduling (the purge *queries* are P4; the
  cron *job* that calls them is P5/P6).
- `postit-api`, `postit-server` wiring — no HTTP routes, no `/ready` gating, no
  `Idempotency-Key` middleware. Those are P6.
- Delegation (`Access::Delegated`, `delegation_accounts`) — plan 03 B2/B6.

## Exit criteria (plan 02's own, unchanged)

`#[sqlx::test]` suites cover every repository; migrations apply from empty; `cargo sqlx
prepare --check` passes; the verifier tests and the identity tests that need no jobs or mail
pass; no token appears in any `Debug` output, error, or audit row.
