# P6 — API and server binary

> Design spec for plan 02 phase P6 (`!ref/plans/02. foundation.md`). Builds `postit-api`
> and `postit-server` (bin `postit`), adds `cargo xtask openapi` and the committed
> `api/openapi.json`, wires the dev stack's `postit-server` container, and closes the
> deferred P4/P5 rulings pinned to P6.

## Context

P5 is merged on `stage`. `postit-api` is an empty scaffold; `postit-server` only answers
`--version`; `xtask` only has `zitadel-bootstrap`.

Already in place that this phase builds on:

- `postit-identity`: `Verifier::verify(token, &JwkSet)` (sync), `OidcDiscovery` with
  `jwks()` / `jwks_for_kid()` (at most one refetch per minute on an unknown `kid`),
  `ClaimsTransformer::transform(&VerifiedClaims, bearer) -> UserRecord` (provisioning,
  bootstrap, profile refresh, userinfo fallback, approval emails), `PrincipalCache` with
  `run_listener`, `UserAdminService` (`approve`, `disable`, `enable`, `change_role`,
  `delete_user`, `delete_self`), `jobs::register`, and the `testkit` `TestIssuer`.
- `postit-data`: `Db::connect`, `run_migrations`, `UsersRepo::list(status, search,
  pagination)`, `AuditRepo::list(filter, pagination)`, `IdempotencyRepo`, `OwnerScope`,
  `pseudonym_for` (UUID v8 pseudonyms, recognizable by version alone).
- `postit-jobs`: concrete `JobQueue` (outbox-backed), `JobRegistry`, `Worker::new(...).run(shutdown)`,
  `migrate`, `maintenance::register_data_retention`, `maintenance::register_job_history_purge`.
- `postit-mail`: `MailOutbox`, `MailLoaders`, `SmtpMailer`, `MemoryMailer`,
  `SendEmailHandler`, `register`.
- `postit-config`: the full settings tree, including `server.{host, api_port,
  worker_port, tls, trusted_proxies, public_url, shutdown_timeout, web}`, `auth.oidc.*`,
  `rate_limit.{unauthenticated, provisioning, authenticated}`, and `cors.allowed_origins`.
- `docker/`: qa/production compose already define `postit-api` and `postit-worker` with
  `POSTIT_ROLE`; the dev compose has `postit-nginx-app` serving placeholders on 44310,
  44311, and 44315; `postit-nginx-infra` has the `postit.local` network alias.

Gaps this phase must fill:

- Nothing composes token → verified claims → cached or transformed `Principal`.
  `UsersRepo::touch_last_seen` has no caller. There is no JWKS readiness signal and no
  background prefetch.
- There is no production bootstrap refusal at startup (plan 02, `postit-identity`).
- Deferred rulings pinned to P6 (memory `p4-deferred-rulings`, `p5-deferred-rulings`):
  P4 #1 (JWKS status classification), P4 #3 (principal cache stale re-insert), P4 #4
  (the composition above), the vacuous cross-process cache test, P5 #1 (audit writes do
  not lock referenced users), and P5 #3 (a dead queue supervisor is invisible to
  `/ready`). The maintainer also folded in P5 #4 (defer clock-skew churn) and P5 #5
  (config duration bounds).

## Decisions

Confirmed with the maintainer before writing this spec:

- **Deferred rulings:** P6 closes every P6-pinned item plus P5 #4 and #5. P4 #2 (last-admin
  TOCTOU) stays unpinned; P4 #5 (broad test gaps) is covered only as far as this spec's
  tests reach; P4 #6 stays with plan 03 B2.
- **The auth pipeline lives in `postit-identity`** as an `Authenticator` service. The API's
  `Auth` extractor only calls it and maps errors. The stricter provisioning rate limit is
  applied through a `ProvisionGate` trait that the API implements, because only identity
  knows that a request is about to create a user.
- **Dev stack exit is verified both natively and in Docker:** `cargo run` with rustls TLS
  on 44310/44311, and a new dev-compose `postit-server` service (role `all`, plain HTTP)
  behind `postit-nginx-app`.
- **Rate limiting is split:** a `tower_governor` layer keyed by client IP for every request;
  a `governor` keyed limiter by `UserId` inside the `Auth` extractor (a tower layer cannot
  key on a principal that does not exist yet); the provisioning bucket, keyed by client IP,
  checked through `ProvisionGate`.
- **Idempotency is a handler helper, not a tower layer.** A layer runs before extractors
  and cannot see the `OwnerScope`.
- **`PATCH /users/{id}` accepts exactly one of `status` or `role`.** The two service calls
  run in separate transactions, so combining them could half-apply.
- **Four additive error codes** beyond plan 02's list: `internal` (500), `unavailable`
  (503), `payload_too_large` (413), `request_timeout` (408).
- **The committed `api/openapi.json` uses placeholder IdP URLs**; `/api/openapi.json`
  patches in the real `authorization_endpoint` and `token_endpoint` from discovery at
  runtime. The contract diff stays stable across environments.
- **Swagger UI uses `utoipa-swagger-ui`'s `vendored` feature**, so builds never download
  assets (Docker and CI build offline).
- **`postit healthcheck` speaks raw HTTP/1.1 over `TcpStream`**, keeping `postit-http` the
  only `reqwest` factory.
- **`POSTIT_ROLE` defaults to `all` in development only**; qa and production require it.
- **Crate versions** (crates.io, 2026-09-28): axum 0.8, axum-server 0.8 (rustls), tower 0.5,
  tower-http (latest compatible with axum 0.8), tower_governor 0.8, governor 0.10,
  utoipa (latest), utoipa-axum, utoipa-swagger-ui (`vendored`, `axum`), tokio-util
  (`CancellationToken`). Exact versions are fixed in the plan's first task.
- **Crate versions as resolved (Task 7):** utoipa 6.0.0, utoipa-swagger-ui 10.0.1 (`vendored`, `axum`), axum-server 0.8.0 (`tls-rustls-no-provider`). The list above is superseded where the rulings below drop `tower_governor` and `utoipa-axum`.

### Rulings made while planning

- **No `tower_governor`; one `governor` limiter type for all three buckets.** `tower_governor`'s layer cannot key on a principal and would be a second rate-limit mechanism beside the per-user and provisioning buckets. A small `KeyedLimiter<K>` over `governor::DefaultKeyedRateLimiter` serves all three, and our own middleware renders the 429 problem. Cost if wrong: swap the IP middleware body in `crates/api/src/limit.rs`.
- **The per-IP bucket charges every request that does not authenticate.** A request without an `Authorization` header is charged up front. A request with one carries an `Authenticated` marker that the `Auth` extractor sets on success; if it finishes unmarked (bad or empty token, a non-Bearer scheme, JWKS unavailable, an internal error, or a route that never authenticates: probes, `/auth/config`, docs, 404s), `ip_rate_limit` charges the IP afterwards and answers 429 if that charge fails. An IP that exhausted its bucket this way is refused before its next token is verified (`KeyedLimiter::penalize`/`blocked`). Authenticated requests are charged to their user's bucket only, so a team behind one NAT address never shares a bucket (plan 02).
- **No `utoipa-axum`.** Routes are plain axum routes; `#[utoipa::path]` plus `#[derive(OpenApi)] paths(...)` builds the spec. One less pre-1.0 dependency.
- **Problems are rendered by middleware.** `ApiError::into_response` stores a `Problem` in the response extensions with an empty body; the `render_problems` middleware (inside the request-ID layer) writes the JSON with `request_id`. The same middleware turns bare 404, 408, and 413 responses from the router and tower-http layers into `not_found`, `request_timeout`, and `payload_too_large` problems.
- **The defer clock-skew fix (P5 #4) lives in `send_email` only.** `send_email` reads `now` from the database (`SELECT now()`, the clock apalis schedules by) and floors every `Defer(at)` at `now + 1s`. Every loader's defer goes through that one path, so `identity/src/mail.rs` needs no change.
- **List values in env vars.** `POSTIT__…` values of the form `[a, b]` become arrays (items trimmed, one pair of quotes stripped, each item scalar-coerced), so compose can set `server.trusted_proxies` and `http.extra_ca_files`.
- **`postit_config::load_with(env, dir, vars)`** is public so tests load config without inheriting `POSTIT_SECRETS_FILE` from `server/.cargo/config.toml`.
- **`postit_data::Db::from_pool`** lets the server smoke test run the composition root against a `#[sqlx::test]` pool.
- **`server.web.enabled = true` is accepted and ignored with a warning in P6** (qa/production compose already set it; serving arrives in P7).
- **`rate_limit` buckets must be at least 1/minute with burst ≥ 1**, validated in `postit-config` (governor quotas need non-zero values).
- **Admin handlers evict the local principal cache** through `Authenticate::invalidate_user` after a successful status, role, or deletion change (plan 01, "Cache invalidation across processes"); the existing `pg_notify` covers other processes. Without it, the acting process would keep serving the stale principal until the `LISTEN` round trip lands.
- **The principal-cache generation is bumped before the eviction**, not after, so a concurrent miss can never re-insert a stale principal between the two (the P4 #3 race).
- **An unknown `kid` refetches the JWKS at most once a minute even when the IdP is failing**, and falls back to the cached set. `JwksUnavailable` (503) therefore means only "nothing was ever loaded"; with a cached set, an unknown `kid` is `unauthenticated`.
- **`mail.from_address` is config**: set in `development.toml` and `qa.toml`, and a deployment env var for production like the issuer.
- **Env-var list values (`[a, b]`) apply to real env vars only**, never to the secrets file, whose values are opaque strings.
- **The request span records the matched route, never the URI**, so query strings (search terms, OAuth codes) never reach logs; it also carries the request ID and the status.
- **`Retry-After` rounds up** to whole seconds (floor 1).
- **`WWW-Authenticate` follows RFC 6750 §3.1**: `Bearer` alone when the request had no usable bearer token (no `Authorization` header, another scheme, or an empty token), `Bearer error="invalid_token"` when a token failed verification. This changes the spec's error table, which listed `error="invalid_token"` for every `unauthenticated`.
- **A bare 405 is left as axum renders it**; the 17 problem codes have no method-not-allowed code.
- **`cors.allowed_origins = ["*"]` is a startup error** (tower-http's `AllowOrigin::list` panics on it). CORS also allows `PUT`, beyond the spec's method list, for the `If-Match` test route and plan 03's replace-style routes.
- **`AppState` holds `limits: Arc<Limits>`** (IP, user, provisioning) and reads the discovery document through `auth.discovery_document()`, instead of the spec's separate limiter and document fields.
- **Idempotency keys are released when a run is cancelled** (request timeout, client disconnect, panic) or `complete` fails, through a drop guard; an expired key is treated as absent. Only a crashed process leaves a key `in_progress` until it expires.
- **`If-Match: *` matches any current version** (RFC 9110); weak tags never match.
- **`PATCH /users/{id}` to `pending` or `deleting` is `validation_failed` whatever the current status**; any other change to a `deleting` user is `user_deleting`.
- **Pages are capped at 1,000,000**, keeping the SQL offset inside `i64`.
- **The dev trusted proxy is `postit-nginx-app` alone (`172.30.0.10/32`)**, not the network's /24, which would include Docker Desktop's gateway.
- **The worker's drain budget starts at shutdown**, the pool close is bounded to 5 s, and a second Ctrl-C exits at once.

### Rulings made during execution

- **`Cargo.lock` carries a deliberate hand-edit:** `emixcrypto` 0.7.0 resolves `rand_chacha` 0.10.0 (default resolution picked 0.3.1 and failed to compile). A `cargo update` may revert it; the durable fix is a newer `emixcrypto` upstream or a direct `rand_chacha = "0.10"` workspace dependency (untested). Cost if wrong: the build breaks after `cargo update`.
- **`postit-identity` cache listener sets `application_name` with `SELECT set_config('application_name', $1, false)`.** `SET application_name = $1` is invalid with a bind parameter; the earlier tests passed only because the error path ran `invalidate_all`. Cost if wrong: the listener connection is unnamed or fails to start.
- **The Swagger UI redirect URI `/docs/oauth2-redirect.html` must be registered with the IdP (Zitadel)** for the public client. Cost if wrong: Swagger UI sign-in fails with a redirect mismatch.
- **The `GET /me` pending-user exception:** pending users reach only `GET /me`; every other handler takes `ActiveUser`, `RequireAdmin`, or `Scope`, never bare `Auth`. Cost if wrong: a pending user reaches a route that should require approval.
- **Idempotency completion runs on a spawned task, and the release delete removes only `in_progress` rows** (a fix found in review). Replay stores status and body only: headers such as `Location` and `ETag` are not replayed, so plan 03 routes using `run` must put everything in the body or extend `StoredResponse`. Cost if wrong: replayed responses lose headers a client relies on.
- **Audit events do not yet record `ip` / `request_id`** (`AuditEvent::ip()`/`request_id()` exist but no caller sets them; `AuditEventDto` fields are always null). Deferred to plan 02 phase P8 (Jobs console / more audited actions); threading `ClientIp`/`RequestId` into the identity services is the fix. Cost if wrong: the audit trail lacks source IP and request correlation until then.
- **qa/production compose files set no `server.trusted_proxies`.** Behind the operator's reverse proxy every client shares the Docker bridge gateway IP and therefore one unauthenticated and one provisioning rate bucket; compose completion is plan 02 P9. Cost if wrong: one noisy client rate-limits everyone on qa/production.

## Section A — `postit-config` changes

- **Duration bounds (P5 #5).** `Settings::validate` rejects any retention, TTL, interval, or
  timeout `Duration` that is zero or longer than 100 years, naming the key. This covers
  `auth.{pending_ttl, principal_cache_ttl, approval_email_interval}`,
  `auth.oidc.{leeway, jwks_refresh_interval}` (leeway may be zero), `audit.{retention,
  ip_retention}`, every `retention.*`, `jobs.{outbox_poll_interval, history_retention.*}`,
  `ops.*`, `http.*` timeouts, and `server.{shutdown_timeout, request_timeout}`.
- **New keys:** `server.request_timeout` (default `30s`) and `server.body_limit` (bytes,
  default `1048576`), the global defaults for the API's timeout and body-limit layers.
- Log format and level, and whether `/docs` is served, stay derived from the environment
  (plan 02, Per-environment behavior); no config keys.

## Section B — `postit-data` changes

- **Audit reference locks (P5 #1).** `AuditLog::record` first runs
  `SELECT id FROM users WHERE id = ANY($1) FOR KEY SHARE` over every referenced user ID:
  `actor_user_id`, `owner_id`, `subject_user_id`, and every `details` value under a key
  ending in `_user_id`. If any ID is missing, it returns `DataError::Conflict("audit
  references a deleted user")` and writes nothing, so the caller's transaction rolls back.
  Because `delete_user`'s final step takes `FOR UPDATE` on the user row, an in-flight audit
  write either commits first (and is then pseudonymized by that final step) or sees the row
  gone and fails.
- **`send_email` recipient lock** (in `postit-mail`) changes from `FOR UPDATE` to
  `FOR NO KEY UPDATE`, so audit writes that take `FOR KEY SHARE` on an admin do not queue
  behind SMTP.
- **`UserRole` and `UserStatus` get public strict `FromStr`** (unknown values error) with a
  `ParseEnumError`. The existing lenient private parse used when reading rows is unchanged.
- **`AuditEventKind` gets `ALL` and a strict `FromStr`**, used to validate `?kind=`.
- **`UsersRepo::list` escapes `\`, `%`, and `_`** in the search term and uses
  `ILIKE … ESCAPE '\'`.

## Section C — `postit-jobs` changes

- **`WorkerHealth` (P5 #3).** `Worker::health(&self) -> WorkerHealth` returns a cheap clone
  (an `Arc` of atomics) exposing `is_ready() -> bool`: true once `Worker::run` has confirmed
  the storage is migrated and started every queue supervisor, and false after any queue
  supervisor task exits or panics. `Worker::run` watches each supervisor's `JoinHandle` and
  flips its flag. The plan's first jobs task reads `backend::run` to record exactly how a
  supervisor can die today and covers that path with a test.
- No other change; the API role never builds a `JobRegistry`.

## Section D — `postit-identity` changes

### `Authenticator`

New module `auth.rs`:

```rust
pub trait ProvisionGate: Send + Sync {
    /// Called once, just before a request would create a user row.
    fn check(&self) -> Result<(), RateLimited>;
}

pub struct RateLimited { pub retry_after: Duration }

pub enum AuthError {
    Invalid(VerifyError),        // 401
    JwksUnavailable,             // 503
    RateLimited(RateLimited),    // 429 (provisioning bucket)
    Internal(IdentityError),     // 500
}

pub struct Authenticator<S: JwksSource> { /* verifier, discovery, cache, transformer, pool */ }

impl<S: JwksSource> Authenticator<S> {
    pub async fn authenticate(&self, bearer: &SecretString, gate: &dyn ProvisionGate)
        -> Result<Principal, AuthError>;
    pub async fn prefetch(&self) -> Result<(), HttpError>;
    pub fn jwks_ready(&self) -> bool;
}
```

`authenticate`:

1. `jsonwebtoken::decode_header` → `kid`. A malformed header is `Invalid(Malformed)`; a
   missing `kid` is `Invalid(UnknownKid)`.
2. `discovery.jwks_for_kid(kid)`. An `HttpError` with nothing cached is `JwksUnavailable`.
   A `kid` still absent from the returned set is `Invalid(UnknownKid)`.
3. `verifier.verify(token, &jwks)`.
4. `cache.get(iss, sub)` → hit returns the `Principal`.
5. Miss: `let generation = cache.generation()`, then `UsersRepo::find_by_oidc`. If no row
   exists, `gate.check()?` and then `transformer.transform(...)`; otherwise also
   `transformer.transform(...)` (it refreshes profile claims). Then
   `UsersRepo::touch_last_seen` (so `last_seen_at` is written at most once per
   `principal_cache_ttl` per process), `Principal::from(&record)`, and
   `cache.insert_if_current(generation, iss, sub, principal)`.

It does not gate on status; the API extractors do, because `GET /me` admits `pending`.

`prefetch` calls `discovery.jwks()`. `jwks_ready` reads a new `OidcDiscovery::is_loaded()`
(true once any JWKS has been cached; a later failed refresh keeps the stale set and stays
ready, matching the existing stale fallback).

`DiscoveryDocument` gains `authorization_endpoint: Option<String>` and
`token_endpoint: Option<String>`, and `OidcDiscovery::document()` returns the cached
document (fetched with the first JWKS load) for the OpenAPI patch.

The API holds the authenticator as `Arc<dyn Authenticate>`, an object-safe `async_trait`
over the three methods, so `postit-api` is not generic over `JwksSource`.

### Other identity changes

- **Principal cache generation (P4 #3).** `PrincipalCache` gains an `AtomicU64` generation
  bumped by `invalidate_user` and `invalidate_all`, `generation()`, and
  `insert_if_current(generation, …)`, which drops the insert if the generation moved.
  A single global counter is coarse (any eviction discards concurrent inserts, which just
  become the next request's miss) but correct and allocation-free.
- **JWKS status classification (P4 #1).** `HttpJwksSource` passes non-2xx discovery and
  JWKS responses through `postit_http::classify_status` (5xx and 429 transient, other 4xx
  permanent) instead of treating every failure alike.
- **Production bootstrap refusal.** New `bootstrap::check_startup(pool, &BootstrapSettings,
  Environment) -> Result<(), IdentityError>`: in `production`, when no active admin exists
  and neither `admin_email` nor `admin_subject` is set, it returns
  `IdentityError::BootstrapRequired` with an actionable message. Other environments and
  deployments with an admin pass.
- **Defer clock skew (P5 #4).** The coalescing defer in `identity/src/mail.rs` and the
  retry defer in `mail/src/send.rs` use `max(next_slot, now + 1s)`.
- **Cross-process cache test rewrite.** The test starts the listener on a second
  `PrincipalCache`, waits until `pg_stat_activity` shows a session with
  `application_name = LISTENER_APPLICATION_NAME` in `LISTEN`, waits for the post-listen
  `invalidate_all` to have happened (generation advanced), inserts the entry, sends the
  targeted `NOTIFY`, and asserts eviction within a timeout.

## Section E — `postit-api`

### Structure

- `state.rs`: `AppState { auth: Arc<dyn Authenticate>, admin: UserAdminService, pool: PgPool,
  settings: Arc<ApiSettings>, user_limiter: Arc<UserLimiter>, provision_limiter:
  Arc<IpLimiter>, readiness: Arc<dyn Readiness>, discovery_doc: Arc<dyn DiscoverySnapshot> }`.
  `ApiSettings` is built from `Settings` (environment, CORS origins, trusted proxies, rate
  limits, request timeout, body limit, OIDC client ID, scopes, issuer, account URL).
- `router.rs`: `pub fn api_router(state: AppState) -> Router` (roles `all` and `api`) and
  `pub fn probe_router(readiness: Arc<dyn Readiness>) -> Router` (worker port).
- `error.rs`, `extract.rs`, `client_ip.rs`, `limit.rs`, `idempotency.rs`,
  `preconditions.rs`, `openapi.rs`, `routes/{auth, me, users, audit, probes}.rs`, `dto.rs`.

### Middleware (outer to inner)

1. `SetRequestIdLayer` / `PropagateRequestIdLayer` on `x-request-id` (UUID v7 when absent;
   an incoming value is kept only if it is a valid UUID, otherwise replaced).
2. `TraceLayer` with a span carrying method, matched path, request ID, and status;
   `SetSensitiveRequestHeadersLayer` marks `Authorization`. No header values or bodies are
   logged.
3. `CorsLayer`: allowed origins from `cors.allowed_origins`; methods GET, POST, PATCH,
   DELETE, OPTIONS; allowed headers `Authorization`, `Content-Type`, `X-Postit-Act-As`,
   `If-Match`, `If-None-Match`, `Idempotency-Key`; exposed headers `ETag`, `Location`,
   `Retry-After`, `x-request-id`; no credentials.
4. `tower_governor` keyed by `ClientIp` from `rate_limit.unauthenticated`, rejecting with a
   `rate_limited` problem and `Retry-After`.
5. `TimeoutLayer` from `server.request_timeout`, mapped to `request_timeout`.
6. `RequestBodyLimitLayer` from `server.body_limit`, mapped to `payload_too_large`.

A route that needs different limits applies its own `route_layer`; plan 03 uses this for
uploads. The probe router gets only layers 1–2.

**`ClientIp`**: the TCP peer address from `ConnectInfo`. When the peer is inside a
`server.trusted_proxies` CIDR, walk `X-Forwarded-For` from right to left, skipping trusted
addresses; the first untrusted one is the client. An untrusted peer's `X-Forwarded-For` is
ignored.

### Errors

`ApiError { code: ErrorCode, detail: Option<String>, retry_after: Option<Duration> }`
renders `application/problem+json`:

```json
{ "type": "about:blank", "title": "Account pending", "status": 403,
  "code": "account_pending", "request_id": "…", "detail": "…" }
```

| Code | Status |
|---|---|
| `unauthenticated` | 401, plus `WWW-Authenticate: Bearer error="invalid_token"` for a token that failed verification, or `WWW-Authenticate: Bearer` (no error code, RFC 6750 §3.1) when the request had no usable bearer token |
| `account_pending`, `account_disabled`, `forbidden` | 403 |
| `not_found` | 404 |
| `request_timeout` | 408 |
| `user_deleting`, `last_admin`, `idempotency_in_progress` | 409 |
| `version_conflict` | 412 |
| `payload_too_large` | 413 |
| `validation_failed`, `idempotency_key_reused` | 422 |
| `precondition_required` | 428 |
| `rate_limited` | 429, plus `Retry-After` |
| `internal` | 500 (no detail ever) |
| `unavailable` | 503 |

Mappings from services: `IdentityError::{InvalidTransition → validation_failed, LastAdmin →
last_admin, UserDeleting → user_deleting, CannotDeleteSelf → forbidden,
Data(NotFound) → not_found}`; everything else is `internal`, logged with the request ID.
`AuthError::{Invalid → unauthenticated, JwksUnavailable → unavailable, RateLimited →
rate_limited, Internal → internal}`. axum's own rejections (bad JSON, bad path, bad query)
map to `validation_failed`, with a detail naming the field but never echoing the value.

### Extractors

- `Auth(Principal)`: reads `Authorization: Bearer …` into a `SecretString`, calls
  `authenticate` with a `ProvisionGate` bound to the request's `ClientIp`, rejects
  `disabled` and `deleting` with `account_disabled`, then checks the per-user limiter
  (`rate_limit.authenticated`, keyed by `UserId`).
- `ActiveUser(Principal)`: `Auth` plus rejecting `pending` with `account_pending`.
- `RequireAdmin(Principal)`: `ActiveUser` plus `role == admin`, else `forbidden`.
- `Scope(OwnerScope)`: `ActiveUser` → `OwnerScope::own(user_id)`. Plan 03 adds the act-as
  header here and nowhere else.

### Idempotency and preconditions

- `IdempotencyKey(Option<String>)` extractor: the `Idempotency-Key` header, 1–255 visible
  ASCII characters, else `validation_failed`.
- `idempotency::run(pool, &scope, key, route, request_hash, f)` where `request_hash` is the
  hex SHA-256 (via `emixcrypto`) of the method, the matched route, and the raw body:
  - no key: run `f`;
  - `IdempotencyRepo::begin` → `Started`: run `f`; on success, `complete` with the status
    and JSON body; on failure, `delete` the in-progress row and return the error;
  - `Conflict` with a different route or hash → `idempotency_key_reused`;
  - `Conflict` in progress → `idempotency_in_progress`;
  - `Conflict` completed → replay the stored status and body.
  Keys expire after 24 hours (the existing `data_retention` purge removes them).
- `preconditions::check_if_match(&HeaderMap, &ETag) -> Result<(), ApiError>`: missing →
  `precondition_required`; mismatch → `version_conflict`. `ETag` renders as `"v{n}"`
  (strong). Plan 02 has no versioned resource, so `If-Match` is exercised only through test
  routes.
- Test routes (`POST /api/v1/_test/idempotent`, `PUT /api/v1/_test/versioned`) are compiled
  under `#[cfg(any(test, feature = "testkit"))]` and mounted only when the `testkit`
  feature builds the router; production builds never contain them.

### Endpoints

| Route | Extractor | Response |
|---|---|---|
| `GET /api/v1/auth/config` | none | `{issuer, client_id, scopes}` |
| `GET /api/v1/me` | `Auth` | `MeDto` |
| `DELETE /api/v1/me` | `ActiveUser` | body `{display_name}`; mismatch → `validation_failed`; 202 |
| `GET /api/v1/users` | `RequireAdmin` | `?status=&search=&page=&page_size=` → `Page<UserDto>` |
| `GET /api/v1/users/{id}` | `RequireAdmin` | `UserDto` |
| `PATCH /api/v1/users/{id}` | `RequireAdmin` | body `{status}` or `{role}` (exactly one) → `UserDto` |
| `DELETE /api/v1/users/{id}` | `RequireAdmin` | 202 |
| `GET /api/v1/admin/audit` | `RequireAdmin` | `?kind=&from=&to=&actor_user_id=&subject_user_id=&page=&page_size=` → `Page<AuditEventDto>` |
| `GET /api/openapi.json` | none | the spec, IdP URLs patched from discovery |
| `GET /docs` | none | Swagger UI, not mounted in production |
| `GET /health`, `GET /ready` | none | API port; the worker port serves the same two via `probe_router` |

- `PATCH` status values map to services: `active` from `pending` → `approve`, `disabled` →
  `disable`, `active` from `disabled` → `enable`; the service decides from the current
  status, and any other transition is `validation_failed`. `pending` or `deleting` as a
  target is `validation_failed`.
- **DTOs** (`serde`, `utoipa::ToSchema`, snake_case enums):
  - `UserDto { id, email, email_verified, display_name, role, status, approved_at,
    approved_by, last_seen_at, created_at, updated_at }`.
  - `MeDto { user: UserDto, account_url: Option<Url> }` (flattened).
  - `Page<T> { data: Vec<T>, total, page, page_size }`; `page` defaults to 1, `page_size` to
    20, maximum 100, out of range → `validation_failed`.
  - `AuditEventDto { id, at, kind, actor, owner, subject: Option<UserRef>, ip, request_id,
    details }` where `UserRef { id, deleted: bool }` and `deleted` is true for a UUID v8
    pseudonym. Every `details` value under a key ending in `_user_id` is rewritten to the
    same `UserRef` shape.
- **Probes:** `/health` always returns 200 `{"status":"ok"}`. `/ready` returns 200
  `{"status":"ready"}` or a 503 `unavailable` problem naming the failing check (`database`,
  `jwks`, `worker`). `Readiness` is a trait; `postit-server` composes it per role: `api`
  checks `SELECT 1` and `jwks_ready`; `worker` checks `SELECT 1` and `WorkerHealth`; `all`
  checks all three on both ports.

### OpenAPI

- `#[derive(OpenApi)]` over every route and DTO, with an OAuth2 authorization-code security
  scheme (scopes from the defaults), `info.version` from the crate version.
  `pub fn openapi() -> utoipa::openapi::OpenApi` is the single source used by both the
  runtime route and `cargo xtask openapi`.
- The committed spec's `authorizationUrl` and `tokenUrl` are
  `https://idp.invalid/authorize` and `https://idp.invalid/token`. The runtime handler
  clones the spec and replaces them with discovery's `authorization_endpoint` and
  `token_endpoint`; before discovery loads it serves the placeholders.
- Swagger UI at `/docs`, with its OAuth2 redirect at `/docs/oauth2-redirect.html`
  (the URI `xtask zitadel-bootstrap` already registers), `client_id` from
  `auth.oidc.client_id`, the configured scopes, and `usePkceWithAuthorizationCodeGrant`.

## Section F — `postit-server` (bin `postit`)

Only crate that uses `anyhow`.

### Commands

- no argument: run the server;
- `--version`: print the version;
- `healthcheck`: load config, pick the role's port (API port for `all` and `api`, worker
  port for `worker`), send `GET /health HTTP/1.1` over a raw `TcpStream` to `127.0.0.1`
  with a 5-second timeout, exit 0 on a 200 status line, else 1. With `server.tls.enabled`
  it exits 1 with "healthcheck requires plain HTTP (server.tls.enabled = true)".

Environment: `POSTIT_ENV` (existing resolution), `POSTIT_ROLE` (`all`, `api`, `worker`;
defaults to `all` in development, required otherwise), `POSTIT_CONFIG_DIR` (default
`./config`).

### Startup

1. Resolve the environment and role, load config, initialize tracing (pretty `debug` in
   development, JSON `info` otherwise; `RUST_LOG` overrides), and log the redacted dump.
2. `Db::connect`, `run_migrations`, `postit_jobs::migrate` (every role).
3. `postit_http::build_client`.
4. API roles: build `HttpJwksSource`, `OidcDiscovery`, `PrincipalCache`, `ClaimsTransformer`,
   and the `Authenticator`. Spawn a prefetch task that retries with exponential backoff
   (1 s up to 60 s) until the first JWKS load succeeds. Spawn `cache::run_listener`.
5. `bootstrap::check_startup` (every role, so a production worker also refuses).
6. Build `SystemClock`/UUID v7 `IdGenerator`, `JobQueue`, `MailOutbox`, and
   `UserAdminService`.
7. Worker roles: build the mailer (`SmtpMailer::new(&settings.mail)`) and the registry, in
   this order: `postit_identity::jobs::register` (adds the mail loaders),
   `postit_jobs::maintenance::register_data_retention`,
   `postit_jobs::maintenance::register_job_history_purge`, then `postit_mail::register`
   with the filled `MailLoaders`. The `api` role builds no registry and no mailer.
8. Serve according to the role:
   - `all`, `api`: `api_router` on `server.host:server.api_port`;
   - `all`, `worker`: `probe_router` on `server.host:server.worker_port`, and
     `Worker::run(shutdown)`.
   With `server.tls.enabled`, both listeners use `axum-server` with rustls from
   `server.tls.{cert_path, key_path}`; otherwise plain HTTP. Both use
   `into_make_service_with_connect_info::<SocketAddr>()` for `ClientIp`.

### Shutdown

A `CancellationToken` fires on SIGTERM (Unix) or Ctrl-C. Each listener's `axum_server::Handle`
calls `graceful_shutdown(Some(server.shutdown_timeout))`; `Worker::run` receives the token's
`cancelled()` future and is wrapped in `tokio::time::timeout(server.shutdown_timeout)`
(a job still running at the timeout is left to apalis's retry). Background tasks
(prefetch, listener) are aborted. The pool is closed last. Exit is 0 after a clean
shutdown; a startup failure exits non-zero with the `anyhow` chain, which never contains
secret values.

## Section G — Docker, nginx, xtask

- **`docker/server.Dockerfile`**: `cargo-chef` planner and cook stages for cached
  dependencies, a `SQLX_OFFLINE=true` build of `-p postit-server --profile dist`, the
  existing `debian:trixie-slim` non-root runtime, `EXPOSE 8080 8081 8082`, and
  `HEALTHCHECK CMD ["postit", "healthcheck"]`. The Flutter stage stays for P7.
- **`docker/docker-compose.development.yml`**: new service `postit-server` (profile `app`,
  container `postit-server`), built from the Dockerfile with the repository root as context:
  - environment: `POSTIT_ENV=development`, `POSTIT_ROLE=all`,
    `POSTIT__SERVER__TLS__ENABLED=false`, `POSTIT__SERVER__API_PORT=8080`,
    `POSTIT__SERVER__WORKER_PORT=8081`, the database URL on `postit-postgres`, SMTP host
    `host.docker.internal` port 25, `POSTIT__SERVER__TRUSTED_PROXIES` set to the dev
    network's subnet, and `POSTIT__HTTP__EXTRA_CA_FILES` pointing at the mounted dev CA;
  - `env_file`: `!ref/vault/development/postit.env`;
  - volumes: `server/config` read-only at `/app/config` (brings `development.toml` and
    `local.toml`) and the dev certificate read-only;
  - `extra_hosts: host.docker.internal:host-gateway`;
  - `depends_on`: `postit-postgres` (healthy) and `postit-nginx-infra`.
  `postit-net` gets an explicit IPAM subnet in the development override so the trusted-proxy
  CIDR is known.
- **`docker/shared/nginx/app.conf`**: 44310 → `http://postit-server:8080`, 44311 →
  `http://postit-server:8081`, with `proxy_set_header X-Forwarded-For
  $proxy_add_x_forwarded_for`, `Host`, and `X-Forwarded-Proto`. 44315 keeps its placeholder
  until P7.
- **`cargo xtask openapi`**: writes `postit_api::openapi()` as pretty JSON with a trailing
  newline to `api/openapi.json` (relative to the repository root). `--check` compares
  instead of writing and exits 1 on drift, for P9's `contract` job.

## Tests

- **postit-config:** zero and over-100-years durations rejected with the key named; the
  shipped `config/*.toml` still validate.
- **postit-data:** `AuditLog::record` fails and writes nothing when any referenced user is
  missing; an audit transaction holding `FOR KEY SHARE` blocks a concurrent
  `DELETE FROM users` until it commits; search escaping (`%`, `_`, `\`); strict `FromStr`
  round trips and rejections.
- **postit-jobs:** a queue supervisor that exits flips `WorkerHealth::is_ready` to false.
- **postit-identity:** `Authenticator` with the test issuer: a cache hit skips the database;
  a miss provisions exactly once and writes `last_seen_at`; an unknown `kid` triggers a
  refetch and a still-unknown `kid` is `Invalid(UnknownKid)`; `JwksUnavailable` before any
  JWKS loads; the gate is called only when a row would be created, and a gate denial
  creates nothing; `insert_if_current` drops an insert that raced an `invalidate_user`.
  `HttpJwksSource`: 503 transient, 404 permanent. `check_startup`: refuses in production
  with no admin and no rule; passes with an admin, with a rule, and in development. The
  rewritten cross-process eviction test.
- **postit-mail / identity:** the defer never targets a slot at or before `now`.
- **postit-api** (`tower::ServiceExt::oneshot`, `#[sqlx::test]` pool, the real `JobQueue`,
  the test issuer):
  - End-to-end exit flow: the bootstrap admin's first request makes them an active admin; a
    second user's first request provisions them `pending`; they get 403 `account_pending`
    on every route except `GET /me`; the admin approves and the outbox holds a
    `user_approved` `send_email`; the user gets access; disable → 403 `account_disabled`,
    enable → access again; role change; delete → 202 and further requests fail; the
    last-admin guard returns 409 `last_admin` on demote, disable, and `DELETE /me`;
    `DELETE /me` with the wrong display name → 422; `GET /admin/audit` lists every event
    from the flow, filters by kind, time range, actor, and subject, and marks a
    pseudonymized ID as `deleted`.
  - Error shape: each code renders problem+json with `code` and `request_id`; 401 carries
    `WWW-Authenticate`; 429 carries `Retry-After`; 500 carries no detail; a malformed JSON
    body is `validation_failed` without echoing input.
  - Idempotency (test route): replay returns the stored response; a reused key with a
    different body → `idempotency_key_reused`; two concurrent requests with one key → one
    success and one `idempotency_in_progress`; the same key for two users → two
    independent runs.
  - `If-Match` (test route): missing → 428, mismatch → 412, match → 200.
  - Rate limits: the per-IP limit for unauthenticated requests; two users behind one IP have
    separate buckets; the provisioning bucket trips before `transform` creates a row;
    `X-Forwarded-For` honored only from a trusted proxy.
  - CORS preflight allows and exposes exactly the listed headers and no credentials; an
    unknown origin gets no CORS headers.
  - Body limit → 413; timeout → 408 (with a test route that sleeps).
  - `/ready` returns 503 `jwks` before the JWKS loads and 200 after; `/docs` is absent in
    production; `/api/openapi.json` carries the discovery URLs.
  - Redaction: a bearer token never appears in captured tracing output, error bodies, or
    `Debug` output of any extractor or error.
- **postit-server:** a smoke test runs role `all` on ephemeral ports against a
  `#[sqlx::test]` database and the test issuer, checks `/health` and `/ready` on both
  ports, runs `healthcheck` against it, and shuts down cleanly within the timeout.
- **xtask:** `cargo xtask openapi --check` passes on the committed file.

## Out of scope for P6

- `/admin/jobs/*`, `JobConsole`, `JobSummary` (P8).
- `server.web`, `/config.json`, the Flutter web stage (P7).
- The `contract`, `app`, and `docker` CI jobs; qa/production compose completion (P9).
- `X-Postit-Act-As`, `owner_only`, `delegation_invalid` (plan 03 B6).
- nginx `client_max_body_size` and unbuffered upload locations for `postit-nginx-app` (plan 02 line 71): deferred to plan 03's media phase, the first route with large bodies; P6 bodies are capped at `server.body_limit` (1 MiB), under nginx's 1 MiB default.
- P4 #2 (last-admin TOCTOU), P4 #6 (`IdempotencyRepo` taking `OwnerScope`; plan 03 B2), and
  P4 #5 test gaps beyond the tests listed here.

## Exit criteria (plan 02's own)

- The end-to-end API test above passes with the test issuer.
- Against the dev stack, both natively (`cargo run` with TLS) and in Docker
  (`./stack.ps1 up -App`): a token from Zitadel is accepted by
  `https://postit.local:44310/api/v1/me`, `/docs` signs in through Zitadel, and
  `https://postit.local:44311/ready` answers 200.
- `api/openapi.json` is committed and `cargo xtask openapi --check` passes.
- The four quality gates pass and `cargo sqlx prepare --check` passes, run from `server/crates/data` (the offline cache lives in `server/crates/data/.sqlx`; only `postit-data` has `query!` macros).
