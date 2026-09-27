# P6: API and Server Binary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `postit-api` (axum router, middleware, extractors, problem+json, the plan 02 endpoints, OpenAPI with Swagger UI sign-in) and `postit-server` (bin `postit`: roles, rustls TLS, graceful shutdown, `healthcheck`, job registration), add `cargo xtask openapi` with a committed `api/openapi.json`, wire the dev-compose `postit-server` container, and close the deferred P4/P5 rulings pinned to P6 — plan 02 phase P6.

**Architecture:** `postit-identity` gains an `Authenticator` that owns the whole token → `Principal` pipeline (header, JWKS, verify, principal cache with a generation counter, claims transformation on a miss, `last_seen_at`). `postit-api` is thin: an `Auth` extractor calls `Authenticate::authenticate` through an object-safe trait and maps `AuthError` to problem codes; handlers call `UserAdminService` and the admin repositories. Errors are rendered by one middleware that turns a response-extension `Problem` (or a bare 404/408/413 from a layer) into RFC 9457 JSON carrying the request ID. Rate limiting uses one `governor`-backed `KeyedLimiter<K>` type for the per-IP, per-user, and provisioning buckets. `postit-server` is a small library (`start`, returning bound addresses and a shutdown token) plus a `main.rs`, so a smoke test can run the real composition root on ephemeral ports.

**Tech Stack:** Rust stable (edition 2024, `rust-version = "1.98"`), axum 0.8, axum-server 0.8 (`tls-rustls-no-provider` + rustls with `ring`), tower 0.5, tower-http 0.6, governor 0.10, utoipa 6 + utoipa-swagger-ui 10 (`vendored`, `axum`), tokio-util (`CancellationToken`), ipnet 2, emixcrypto 0 (`Sha256Hash`), existing `postit-config`/`postit-core`/`postit-data`/`postit-http`/`postit-jobs`/`postit-mail`/`postit-identity`.

**Spec:** `docs/superpowers/specs/2026-09-28-p6-api-server-design.md`

## Rulings made while writing this plan

These refine the spec; Task 17 records them in the spec's Decisions section.

- **No `tower_governor`; one `governor` limiter type for all three buckets.** `tower_governor`'s layer cannot key on a principal and would be a second rate-limit mechanism beside the per-user and provisioning buckets. A small `KeyedLimiter<K>` over `governor::DefaultKeyedRateLimiter` serves all three, and our own middleware renders the 429 problem. Cost if wrong: swap the IP middleware body in `crates/api/src/limit.rs`.
- **The per-IP bucket charges only requests without a bearer token, plus requests whose token fails authentication.** Authenticated requests are charged to their user's bucket only, so a team behind one NAT address never shares a bucket (plan 02), while a flood of bad tokens is still bounded per IP.
- **No `utoipa-axum`.** Routes are plain axum routes; `#[utoipa::path]` plus `#[derive(OpenApi)] paths(...)` builds the spec. One less pre-1.0 dependency.
- **Problems are rendered by middleware.** `ApiError::into_response` stores a `Problem` in the response extensions with an empty body; the `render_problems` middleware (inside the request-ID layer) writes the JSON with `request_id`. The same middleware turns bare 404, 408, and 413 responses from the router and tower-http layers into `not_found`, `request_timeout`, and `payload_too_large` problems.
- **The defer clock-skew fix (P5 #4) lives in `send_email` only.** `send_email` reads `now` from the database (`SELECT now()`, the clock apalis schedules by) and floors every `Defer(at)` at `now + 1s`. Every loader's defer goes through that one path, so `identity/src/mail.rs` needs no change.
- **List values in env vars.** `POSTIT__…` values of the form `[a, b]` become arrays (items trimmed, one pair of quotes stripped, each item scalar-coerced), so compose can set `server.trusted_proxies` and `http.extra_ca_files`.
- **`postit_config::load_with(env, dir, vars)`** is public so tests load config without inheriting `POSTIT_SECRETS_FILE` from `server/.cargo/config.toml`.
- **`postit_data::Db::from_pool`** lets the server smoke test run the composition root against a `#[sqlx::test]` pool.
- **`server.web.enabled = true` is accepted and ignored with a warning in P6** (qa/production compose already set it; serving arrives in P7).
- **`rate_limit` buckets must be at least 1/minute with burst ≥ 1**, validated in `postit-config` (governor quotas need non-zero values).
- **Admin handlers evict the local principal cache** through `Authenticate::invalidate_user` after a successful status, role, or deletion change (plan 01, "Cache invalidation across processes"); the existing `pg_notify` covers other processes. Without it, the acting process would keep serving the stale principal until the `LISTEN` round trip lands.

## Global Constraints

- Rust stable, edition 2024, `rust-version = "1.98"` floor (workspace `[workspace.package]`).
- Run everything from `server/`. After every task: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace` all pass.
- `unsafe_code = "forbid"`; `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` are denied everywhere including `tests/`. Tests use `unwrap_or_else(|e| unreachable!("context: {e}"))` and `let … else { unreachable!(…) }`, the pattern already used in `crates/identity/tests/admin.rs`.
- clippy `pedantic` is on: more than 7 parameters fails `too_many_arguments`; bundle parameters into a struct.
- Every new external dependency is declared once in `server/Cargo.toml` `[workspace.dependencies]` and consumed with `dep.workspace = true`, enabling only needed features.
- Only `postit-server` uses `anyhow`. Library crates use `thiserror` enums.
- `postit-data` repository functions take `conn: &mut sqlx::PgConnection`. New `sqlx::query!` macros live only in `postit-data`; after changing them regenerate the offline cache from `server/crates/data` with `DATABASE_URL` pointing at the local database (`postit-postgres` container, port 5432, user `postgres`, password in `!ref/vault/development/postgres.env`, database `postit`): `cargo sqlx prepare`, then `SQLX_OFFLINE=true cargo check -p postit-data --all-targets`.
- `#[sqlx::test]` tests outside `postit-data` use `#[sqlx::test(migrations = "../data/migrations")]` and call `postit_jobs::migrate(&pool).await` first when they touch apalis storage.
- No bearer token, SMTP password, pseudonym key, or other secret appears in a log, error body, `Debug` output, or audit row. Bearer tokens travel as `secrecy::SecretString` from the extractor to `Authenticator`.
- Every audit write goes through `postit_data::audit::AuditLog`.
- apalis types never leave `postit-jobs`.
- Problem+json codes are exactly: `unauthenticated`, `account_pending`, `account_disabled`, `forbidden`, `not_found`, `request_timeout`, `user_deleting`, `last_admin`, `idempotency_in_progress`, `version_conflict`, `payload_too_large`, `validation_failed`, `idempotency_key_reused`, `precondition_required`, `rate_limited`, `internal`, `unavailable`.
- CORS allowed headers: `Authorization`, `Content-Type`, `X-Postit-Act-As`, `If-Match`, `If-None-Match`, `Idempotency-Key`. Exposed headers: `ETag`, `Location`, `Retry-After`, `x-request-id`. No credentials.
- Dev ports: API 44310, worker 44311 (native, TLS); container ports 8080 (API), 8081 (worker), 8082 (web, P7).

## Review Focus

- A token that fails verification must never provision a user or touch the database, and must be charged to the caller's IP bucket — Task 9's invalid-token test asserts zero `users` rows and a 429 after the IP burst.
- A `pending` user calling any route other than `GET /me` must get 403 `account_pending`, including admin routes and `DELETE /me` — Task 14's end-to-end test walks every route.
- `PATCH /users/{id}` with both `status` and `role`, or neither, must be 422 and change nothing — Task 10's test re-reads the row.
- An audit event referencing a user deleted concurrently must roll back the whole admin action, not write a raw ID — Task 2's `record` test and its `FOR KEY SHARE` blocking test.
- A 500 must never carry the underlying error text (SQL, HTTP, file paths) in its body — Task 7's `internal` test builds an `ApiError` from a `DataError::Sql` and asserts the body has no `detail`.

---

### Task 1: `postit-config` — duration bounds, rate-limit bounds, request limits, env list values, `load_with`

**Files:**
- Modify: `server/crates/config/src/settings.rs`
- Modify: `server/crates/config/src/loader.rs`
- Modify: `server/crates/config/src/lib.rs`
- Modify: `server/config/default.toml`
- Test: `server/crates/config/src/loader.rs` (`mod tests`)

**Interfaces:**
- Produces: `ServerSettings { …, request_timeout: Duration, body_limit: usize }` (both `#[serde(default)]` with defaults 30 s and 1 MiB).
- Produces: `pub fn load_with(env: Environment, config_dir: &Path, vars: impl IntoIterator<Item = (String, String)>) -> Result<Settings, ConfigError>` re-exported from `postit_config`.
- Produces: validation errors naming the offending key, e.g. `"audit.retention must be greater than 0 and at most 100 years"`.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `loader.rs`:

```rust
    #[test]
    fn zero_duration_is_rejected_with_its_key() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "[audit]\nretention = \"0s\"\n");
        let err = load_with_vars(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("audit.retention")),
            "got {err:?}"
        );
    }

    #[test]
    fn duration_over_one_hundred_years_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "[auth]\npending_ttl = \"200years\"\n");
        let err = load_with_vars(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("auth.pending_ttl")),
            "got {err:?}"
        );
    }

    #[test]
    fn zero_leeway_is_allowed() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "[auth.oidc]\nleeway = \"0s\"\n");
        assert!(load_with_vars(Environment::Development, dir.path(), []).is_ok());
    }

    #[test]
    fn zero_rate_limit_is_rejected() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[rate_limit.authenticated]\nrate_per_minute = 0\nburst = 1\n",
        );
        let err = load_with_vars(Environment::Development, dir.path(), []).err();
        assert!(
            matches!(&err, Some(ConfigError::Validation(msg)) if msg.contains("rate_limit.authenticated")),
            "got {err:?}"
        );
    }

    #[test]
    fn request_timeout_and_body_limit_default() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");
        let settings = ok_settings(load_with_vars(Environment::Development, dir.path(), []));
        assert_eq!(settings.server.request_timeout, std::time::Duration::from_secs(30));
        assert_eq!(settings.server.body_limit, 1_048_576);
    }

    #[test]
    fn bracketed_env_value_becomes_a_list() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(dir.path(), "development.toml", "");
        let settings = ok_settings(load_with_vars(
            Environment::Development,
            dir.path(),
            [
                (
                    "POSTIT__SERVER__TRUSTED_PROXIES".to_string(),
                    "[172.30.0.0/24, \"10.0.0.1\"]".to_string(),
                ),
                (
                    "POSTIT__HTTP__EXTRA_CA_FILES".to_string(),
                    "[/certs/ca.crt]".to_string(),
                ),
            ],
        ));
        assert_eq!(settings.server.trusted_proxies, vec!["172.30.0.0/24", "10.0.0.1"]);
        assert_eq!(
            settings.http.extra_ca_files,
            vec![std::path::PathBuf::from("/certs/ca.crt")]
        );
    }

    #[test]
    fn shipped_config_files_validate() {
        // server/config/*.toml must keep loading after this task's new rules. Secrets are
        // supplied inline so no vault is needed.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let secrets = [
            ("POSTIT__DATABASE__USERNAME".to_string(), "u".to_string()),
            ("POSTIT__DATABASE__PASSWORD".to_string(), "p".to_string()),
            ("POSTIT__AUDIT__PSEUDONYM_KEY".to_string(), "k".to_string()),
            ("POSTIT__MAIL__FROM_ADDRESS".to_string(), "noreply@postit.test".to_string()),
        ];
        ok_settings(load_with_vars(Environment::Development, &dir, secrets.clone()));
        let mut qa = secrets.to_vec();
        qa.push(("POSTIT__MAIL__SMTP__HOST".to_string(), "smtp.test".to_string()));
        ok_settings(load_with_vars(Environment::Qa, &dir, qa));
    }
```

If `shipped_config_files_validate` fails before your change because a shipped file lacks some required key (for example `mail.from_address`), add that key to the `secrets` vector above, not to the TOML files — secrets and deployment values stay out of the committed files.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p postit-config`
Expected: the new tests fail (no validation, no `request_timeout` field, no list parsing).

- [ ] **Step 3: Implement**

In `settings.rs`, add to `ServerSettings`:

```rust
    #[serde(with = "crate::duration", default = "default_request_timeout")]
    pub request_timeout: Duration,
    #[serde(default = "default_body_limit")]
    pub body_limit: usize,
```

```rust
fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_body_limit() -> usize {
    1_048_576
}

const MAX_DURATION: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

fn check_duration(key: &str, value: Duration, allow_zero: bool) -> Result<(), ConfigError> {
    if (!allow_zero && value.is_zero()) || value > MAX_DURATION {
        let lower = if allow_zero { "at least 0" } else { "greater than 0" };
        return Err(ConfigError::Validation(format!(
            "{key} must be {lower} and at most 100 years"
        )));
    }
    Ok(())
}

fn check_bucket(key: &str, bucket: &RateBucket) -> Result<(), ConfigError> {
    if bucket.rate_per_minute == 0 || bucket.burst == 0 {
        return Err(ConfigError::Validation(format!(
            "{key}.rate_per_minute and {key}.burst must both be at least 1"
        )));
    }
    Ok(())
}
```

Add a `validate_bounds(&self)` method called at the end of `validate` (before `Ok(())`):

```rust
    fn validate_bounds(&self) -> Result<(), ConfigError> {
        let s = self;
        for (key, value) in [
            ("server.shutdown_timeout", s.server.shutdown_timeout),
            ("server.request_timeout", s.server.request_timeout),
            ("auth.oidc.jwks_refresh_interval", s.auth.oidc.jwks_refresh_interval),
            ("auth.pending_ttl", s.auth.pending_ttl),
            ("auth.principal_cache_ttl", s.auth.principal_cache_ttl),
            ("auth.approval_email_interval", s.auth.approval_email_interval),
            ("audit.retention", s.audit.retention),
            ("audit.ip_retention", s.audit.ip_retention),
            ("retention.notifications_read_after", s.retention.notifications_read_after),
            ("retention.delivery_attempts_after", s.retention.delivery_attempts_after),
            ("retention.ai_generation_prompt_after", s.retention.ai_generation_prompt_after),
            ("retention.ai_generation_row_after", s.retention.ai_generation_row_after),
            ("ops.check_interval", s.ops.check_interval),
            ("ops.due_delivery_overdue_after", s.ops.due_delivery_overdue_after),
            ("jobs.outbox_poll_interval", s.jobs.outbox_poll_interval),
            ("jobs.history_retention.succeeded", s.jobs.history_retention.succeeded),
            ("jobs.history_retention.failed", s.jobs.history_retention.failed),
            ("http.connect_timeout", s.http.connect_timeout),
            ("http.request_timeout", s.http.request_timeout),
        ] {
            check_duration(key, value, false)?;
        }
        check_duration("auth.oidc.leeway", s.auth.oidc.leeway, true)?;
        check_bucket("rate_limit.unauthenticated", &s.rate_limit.unauthenticated)?;
        check_bucket("rate_limit.provisioning", &s.rate_limit.provisioning)?;
        check_bucket("rate_limit.authenticated", &s.rate_limit.authenticated)?;
        if s.server.body_limit == 0 {
            return Err(ConfigError::Validation(
                "server.body_limit must be at least 1".into(),
            ));
        }
        Ok(())
    }
```

Update `validate`'s doc comment to mention the bounds.

In `loader.rs`, make the list form work — replace `coerce_scalar(raw)` in `EnvVars::data` with `coerce_value(raw)`:

```rust
/// A bracketed value (`[a, b]`) becomes an array of scalars; anything else is one scalar.
fn coerce_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let items: Vec<Value> = inner
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| {
                let unquoted = ['"', '\'']
                    .iter()
                    .find_map(|q| item.strip_prefix(*q)?.strip_suffix(*q))
                    .unwrap_or(item);
                coerce_scalar(unquoted)
            })
            .collect();
        return Value::from(items);
    }
    coerce_scalar(raw)
}
```

`coerce_scalar` turns `"10.0.0.1"` into a string already (it is not a valid `f64`), but a bare numeric item would become a number; `trusted_proxies` items are always CIDRs or addresses, so this is fine.

Make loading with explicit vars public: rename `load_with_vars` to `pub fn load_with` (keep the same signature and body), make `load` call it, update every call in `mod tests` (`load_with_vars(` → `load_with(`), and in `lib.rs` export it next to `load` (`pub use loader::{load, load_with};` — match the existing export style).

Add to `server/config/default.toml` under `[server]`:

```toml
request_timeout = "30s"
body_limit = 1048576
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p postit-config`
Expected: PASS.

- [ ] **Step 5: Quality gates and commit**

Run the four gates from `server/`.

```bash
git add server/crates/config server/config/default.toml
git commit -m "postit-config: duration and rate-limit bounds, request limits, env list values, load_with"
```

---

### Task 2: `postit-data` — audit reference locks, strict enum parsing, search escaping, `Db::from_pool`, send lock

**Files:**
- Modify: `server/crates/data/src/audit.rs`
- Modify: `server/crates/data/src/users.rs`
- Modify: `server/crates/data/src/pool.rs`
- Modify: `server/crates/data/.sqlx/` (regenerated)
- Test: `server/crates/data/tests/audit.rs`, `server/crates/data/tests/users.rs`

**Interfaces:**
- Produces: `AuditLog::record` fails with `DataError::Conflict("audit references a deleted user".into())` when any referenced user row is missing, after taking `FOR KEY SHARE` on all of them.
- Produces: `impl FromStr for UserRole`, `impl FromStr for UserStatus`, `impl FromStr for AuditEventKind` (all with `Err = ParseEnumError`), `AuditEventKind::ALL: [AuditEventKind; 10]`, `pub struct ParseEnumError(pub String)` (in `users.rs`, re-exported as `postit_data::ParseEnumError`; `Display`: `unknown value: {0}`).
- Produces: `UsersRepo::lock_for_send(conn, id) -> Result<Option<UserRecord>, DataError>` (`FOR NO KEY UPDATE`).
- Produces: `Db::from_pool(pool: PgPool) -> Db`.

- [ ] **Step 1: Write the failing tests**

Append to `server/crates/data/tests/audit.rs` (reuse its existing imports and helpers; add missing `use` lines):

```rust
#[sqlx::test]
async fn record_fails_when_a_referenced_user_is_missing(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let existing = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, existing, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    let ghost = UserId::from(uuid::Uuid::now_v7());

    let event = AuditEvent::new(AuditEventKind::RoleChanged)
        .actor(existing)
        .detail_user_id("other_user_id", ghost)
        .unwrap_or_else(|e| unreachable!("detail: {e}"));
    let result = AuditLog::record(&mut conn, AuditEventId::from(uuid::Uuid::now_v7()), event).await;

    assert!(matches!(result, Err(DataError::Conflict(_))), "got {result:?}");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn an_open_audit_write_blocks_deleting_its_referenced_user(pool: PgPool) {
    let mut setup = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut setup, user, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    drop(setup);

    let mut audit_tx = pool.begin().await.unwrap_or_else(|e| unreachable!("begin: {e}"));
    AuditLog::record(
        &mut audit_tx,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserEnabled).subject(user),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record: {e}"));

    let delete_pool = pool.clone();
    let delete = tokio::spawn(async move {
        let mut conn = delete_pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::delete(&mut conn, user).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!delete.is_finished(), "DELETE must wait for the audit transaction's key-share lock");

    audit_tx.commit().await.unwrap_or_else(|e| unreachable!("commit: {e}"));
    let deleted = delete
        .await
        .unwrap_or_else(|e| unreachable!("join: {e}"))
        .unwrap_or_else(|e| unreachable!("delete: {e}"));
    assert!(deleted);
}

#[test]
fn audit_event_kind_parses_strictly() {
    for kind in AuditEventKind::ALL {
        assert_eq!(kind.as_str().parse::<AuditEventKind>().ok(), Some(kind));
    }
    assert!("user_exploded".parse::<AuditEventKind>().is_err());
}
```

Append to `server/crates/data/tests/users.rs`:

```rust
#[test]
fn role_and_status_parse_strictly() {
    assert_eq!("admin".parse::<UserRole>().ok(), Some(UserRole::Admin));
    assert_eq!("member".parse::<UserRole>().ok(), Some(UserRole::Member));
    assert!("root".parse::<UserRole>().is_err());
    for status in [UserStatus::Pending, UserStatus::Active, UserStatus::Disabled, UserStatus::Deleting] {
        assert_eq!(status.as_str().parse::<UserStatus>().ok(), Some(status));
    }
    assert!("banned".parse::<UserStatus>().is_err());
}

#[sqlx::test]
async fn list_search_treats_wildcards_literally(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    for (sub, name) in [("s1", "100% real"), ("s2", "100 real"), ("s3", "a_b"), ("s4", "axb")] {
        UsersRepo::provision(&mut conn, UserId::from(uuid::Uuid::now_v7()), "https://i.test", sub, name)
            .await
            .unwrap_or_else(|e| unreachable!("provision: {e}"));
    }
    let page = emixdb::dto::Pagination { page: 1, page_size: 10 };

    let percent = UsersRepo::list(&mut conn, None, Some("100%"), page.clone())
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(percent.data.iter().map(|u| u.display_name.as_str()).collect::<Vec<_>>(), ["100% real"]);

    let underscore = UsersRepo::list(&mut conn, None, Some("a_b"), page)
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));
    assert_eq!(underscore.data.len(), 1);
}

#[sqlx::test]
async fn lock_for_send_returns_the_row(pool: PgPool) {
    let mut tx = pool.begin().await.unwrap_or_else(|e| unreachable!("begin: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut tx, id, "https://i.test", "s", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    let found = UsersRepo::lock_for_send(&mut tx, id)
        .await
        .unwrap_or_else(|e| unreachable!("lock: {e}"));
    assert!(found.is_some_and(|u| u.id == id));
}
```

If `emixdb::dto::Pagination` is not `Clone`, build two literals instead of `.clone()`. Add `emixdb.workspace = true` to `[dev-dependencies]` in `crates/data/Cargo.toml` only if the test file cannot already reach it (it is a normal dependency, so `emixdb::` works from integration tests).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p postit-data`
Expected: compile errors (`ALL`, `FromStr`, `lock_for_send` missing).

- [ ] **Step 3: Implement**

`users.rs` — add:

```rust
/// A string that is not one of an enum's known values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value: {0}")]
pub struct ParseEnumError(pub String);

impl std::str::FromStr for UserRole {
    type Err = ParseEnumError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            other => Err(ParseEnumError(other.to_string())),
        }
    }
}

impl std::str::FromStr for UserStatus {
    type Err = ParseEnumError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            "deleting" => Ok(Self::Deleting),
            other => Err(ParseEnumError(other.to_string())),
        }
    }
}
```

Keep the private lenient `parse` functions used by `From<UserRow>`.

Escape the search term in `list` — replace `let search_pattern = search.map(|s| format!("%{s}%"));` with:

```rust
        let search_pattern = search.map(|s| {
            let escaped = s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            format!("%{escaped}%")
        });
```

and change both `ILIKE $2` occurrences in the two queries to `ILIKE $2 ESCAPE '\'` (in the Rust raw string that is `ESCAPE '\'`; the SQL literal `'\'` is one backslash because `standard_conforming_strings` is on).

Add `lock_for_send` next to `lock_by_id`, identical except for the lock clause and doc:

```rust
    /// `SELECT … FOR NO KEY UPDATE` on one user: serializes concurrent `send_email` runs for
    /// one recipient without blocking audit writes, which take `FOR KEY SHARE`. Call inside a
    /// transaction.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn lock_for_send(
        conn: &mut PgConnection,
        id: UserId,
    ) -> Result<Option<UserRecord>, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users WHERE id = $1 FOR NO KEY UPDATE"#,
            id.as_uuid()
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }
```

`audit.rs` — add `ALL` and `FromStr`:

```rust
impl AuditEventKind {
    pub const ALL: [Self; 10] = [
        Self::UserProvisioned,
        Self::UserApproved,
        Self::UserDisabled,
        Self::UserEnabled,
        Self::RoleChanged,
        Self::BootstrapAdminGranted,
        Self::UserDeleted,
        Self::EmailSent,
        Self::EmailDropped,
        Self::EmailFailed,
    ];
}

impl std::str::FromStr for AuditEventKind {
    type Err = crate::users::ParseEnumError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| crate::users::ParseEnumError(value.to_string()))
    }
}
```

In `AuditLog::record`, before the insert:

```rust
        let mut referenced: Vec<Uuid> = [event.actor_user_id, event.owner_id, event.subject_user_id]
            .into_iter()
            .flatten()
            .map(|id| id.as_uuid())
            .collect();
        referenced.extend(
            event
                .details
                .iter()
                .filter(|(key, _)| key.ends_with("_user_id"))
                .filter_map(|(_, value)| value.as_str())
                .filter_map(|s| Uuid::parse_str(s).ok()),
        );
        referenced.sort_unstable();
        referenced.dedup();
        if !referenced.is_empty() {
            // FOR KEY SHARE: a concurrent delete of any referenced user (delete_user's final
            // step takes FOR UPDATE, UsersRepo::delete deletes) waits for this transaction,
            // and a user already gone fails the write, so no raw ID of a deleted user can
            // land in audit_events after its pseudonymization.
            let locked = sqlx::query_scalar!(
                "SELECT id FROM users WHERE id = ANY($1) FOR KEY SHARE",
                &referenced
            )
            .fetch_all(&mut *conn)
            .await?;
            if locked.len() != referenced.len() {
                return Err(DataError::Conflict("audit references a deleted user".into()));
            }
        }
```

Update `record`'s `# Errors` doc to list `DataError::Conflict`.

`pool.rs` — add:

```rust
    /// Wraps an existing pool (tests hand the composition root a `#[sqlx::test]` pool).
    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
```

(Adjust the field name to match `Db`'s actual field.) Re-export `ParseEnumError` from `lib.rs`: `pub use users::ParseEnumError;`.

- [ ] **Step 4: Regenerate the offline cache and fix affected tests**

From `server/crates/data`, with `DATABASE_URL` set per Global Constraints: `cargo sqlx prepare`, then `SQLX_OFFLINE=true cargo check -p postit-data --all-targets`.

Run: `cargo test -p postit-data -p postit-identity -p postit-mail`
Any existing test that records an audit event referencing a user that was never provisioned now fails with `DataError::Conflict`. Fix each such test by provisioning the referenced users first (`UsersRepo::provision`), never by weakening `record`. Expected afterwards: PASS.

- [ ] **Step 5: Quality gates and commit**

```bash
git add server/crates/data server/crates/identity/tests server/crates/mail/tests
git commit -m "postit-data: audit writes lock referenced users, strict enum parsing, literal search, lock_for_send, Db::from_pool"
```

---

### Task 3: `postit-mail` — database clock, defer floor, non-blocking recipient lock

**Files:**
- Modify: `server/crates/mail/src/send.rs`
- Test: `server/crates/mail/tests/` (add to the existing `send_email` test file; find it with `rg -l "SendEmailHandler" server/crates/mail/tests`)

**Interfaces:**
- Consumes: `UsersRepo::lock_for_send` (Task 2).
- Produces: `send_email` behavior only — a deferred mail is re-enqueued with `run_at >= db_now + 1s`.

- [ ] **Step 1: Write the failing test**

In the `send_email` test file, add a test with a stub loader that always defers to a slot in the past:

```rust
struct DeferToPast;

#[async_trait::async_trait]
impl postit_mail::MailContextLoader for DeferToPast {
    async fn load(
        &self,
        _conn: &mut sqlx::PgConnection,
        _recipient: &postit_data::users::UserRecord,
        _params: &postit_mail::MailParams,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<postit_mail::LoadOutcome, postit_mail::MailError> {
        Ok(postit_mail::LoadOutcome::Defer(now - chrono::Duration::seconds(30)))
    }
    async fn mark_sent(
        &self,
        _conn: &mut sqlx::PgConnection,
        _recipient: postit_core::UserId,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), postit_mail::MailError> {
        Ok(())
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_defer_never_targets_a_slot_at_or_before_now(pool: sqlx::PgPool) {
    // Build the handler exactly as the file's other tests do, but register DeferToPast for
    // MailKind::UserApproved, then provision an active recipient with a verified email and
    // call handler.handle(SendEmail { kind: UserApproved, recipient, params: None }, ctx).
    // Then read the one job_outbox row the defer wrote:
    let run_at: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT run_at FROM job_outbox ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("outbox row: {e}"));
    let db_now: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("now: {e}"));
    let Some(run_at) = run_at else { unreachable!("defer must set run_at") };
    assert!(run_at > db_now, "run_at {run_at} must be after db now {db_now}");
}
```

Fill the handler construction and recipient provisioning from the helpers already in that file (copy the setup of its nearest `LoadOutcome::Defer` or send test; the column names in `job_outbox` are in `crates/data/migrations/0006_job_outbox.sql` — if the scheduled-time column is not `run_at`, use its real name).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p postit-mail a_defer_never_targets`
Expected: FAIL — `run_at` is 30 s in the past.

- [ ] **Step 3: Implement**

In `SendEmailHandler::handle`, replace `let now = Utc::now();` and the lock:

```rust
        let mut tx = d.pool.begin().await.map_err(retry)?;
        // The database clock, not this process's: apalis schedules run_at by the database's
        // now(), so comparing and deferring against it cannot churn on app/DB clock skew.
        let now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx)
            .await
            .map_err(retry)?;

        // FOR NO KEY UPDATE serializes concurrent sends to one recipient (coalescing depends
        // on it) without blocking audit writes, which take FOR KEY SHARE on this row.
        let Some(recipient) = UsersRepo::lock_for_send(&mut tx, recipient_id)
```

(delete the old `let mut tx = …` line that followed `now`). In the `LoadOutcome::Defer(at)` arm, floor the slot:

```rust
            LoadOutcome::Defer(at) => {
                let at = at.max(now + chrono::Duration::seconds(1));
```

In `deliver`'s failure path, change the re-lock `UsersRepo::lock_by_id` to `UsersRepo::lock_for_send`.

`sqlx::query_scalar` with a string literal is runtime SQL, allowed outside `postit-data` because it touches no postit table.

- [ ] **Step 4: Run tests**

Run: `cargo test -p postit-mail -p postit-identity`
Expected: PASS.

- [ ] **Step 5: Quality gates and commit**

```bash
git add server/crates/mail
git commit -m "postit-mail: database clock and a 1s defer floor in send_email, FOR NO KEY UPDATE recipient lock"
```

---

### Task 4: `postit-jobs` — `WorkerHealth`

**Files:**
- Create: `server/crates/jobs/src/health.rs`
- Modify: `server/crates/jobs/src/lib.rs`, `server/crates/jobs/src/worker.rs`, `server/crates/jobs/src/backend.rs`
- Test: `server/crates/jobs/src/health.rs` (`mod tests`), `server/crates/jobs/tests/worker_health.rs`

**Interfaces:**
- Produces: `#[derive(Clone, Default)] pub struct WorkerHealth` with `pub fn is_ready(&self) -> bool`, `pub(crate) fn mark_started(&self)`, `pub(crate) fn mark_dead(&self, what: &str)`.
- Produces: `Worker::health(&self) -> WorkerHealth` (call before `run`, which consumes the worker).
- Produces: `pub(crate) async fn supervise(tasks: JoinSet<()>, shutdown: watch::Receiver<bool>, health: WorkerHealth)` in `health.rs`.

How a supervisor dies today: `Backend::run` joins its per-queue tasks with `join_next` and only logs a `JoinError` (a panic inside `run_queue` or `forward_inserts`); the task is gone and nothing restarts it. `run_queue` itself returns only on shutdown. The relay and recurring tasks spawned in `Worker::run` are never watched at all. After this task, any of those tasks ending before shutdown flips `WorkerHealth` to not ready.

- [ ] **Step 1: Write the failing unit test**

`server/crates/jobs/src/health.rs`:

```rust
//! Worker liveness for `/ready`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;
use tokio::task::JoinSet;

/// Whether this process's job worker can make progress: storage migrated and every
/// supervised task (queue workers, the insert listener, the outbox relay, recurring loops)
/// still running. Cheap to clone; read by the worker port's `/ready`.
#[derive(Clone, Default)]
pub struct WorkerHealth {
    started: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
}

impl WorkerHealth {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.started.load(Ordering::Acquire) && !self.dead.load(Ordering::Acquire)
    }

    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::Release);
    }

    pub(crate) fn mark_dead(&self, what: &str) {
        tracing::error!(task = what, "job worker task ended before shutdown; worker not ready");
        self.dead.store(true, Ordering::Release);
    }
}

/// Joins `tasks`; any task that ends (returns or panics) while `shutdown` is still false
/// marks `health` dead.
pub(crate) async fn supervise(
    mut tasks: JoinSet<()>,
    shutdown: watch::Receiver<bool>,
    health: WorkerHealth,
    what: &'static str,
) {
    while let Some(joined) = tasks.join_next().await {
        if let Err(err) = &joined {
            tracing::error!(error = %err, task = what, "job worker task ended abnormally");
        }
        if !*shutdown.borrow() {
            health.mark_dead(what);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_task_panicking_before_shutdown_marks_the_worker_dead() {
        let health = WorkerHealth::default();
        health.mark_started();
        let (_stop_tx, stop_rx) = watch::channel(false);
        let mut tasks = JoinSet::new();
        tasks.spawn(async { unreachable!("simulated queue supervisor panic") });
        supervise(tasks, stop_rx, health.clone(), "queue").await;
        assert!(!health.is_ready());
    }

    #[tokio::test]
    async fn tasks_ending_after_shutdown_keep_the_worker_healthy() {
        let health = WorkerHealth::default();
        health.mark_started();
        let (stop_tx, stop_rx) = watch::channel(false);
        let _ = stop_tx.send(true);
        let mut tasks = JoinSet::new();
        tasks.spawn(async {});
        supervise(tasks, stop_rx, health.clone(), "queue").await;
        assert!(health.is_ready());
    }

    #[test]
    fn not_ready_before_start() {
        assert!(!WorkerHealth::default().is_ready());
    }
}
```

`unreachable!` panics without tripping `clippy::panic`. Add `mod health;` and `pub use health::WorkerHealth;` to `lib.rs`.

- [ ] **Step 2: Run to verify**

Run: `cargo test -p postit-jobs health`
Expected: PASS for the unit tests (they test the new helper alone); the wiring below has no test yet.

- [ ] **Step 3: Write the failing integration test**

`server/crates/jobs/tests/worker_health.rs`:

```rust
use std::time::Duration;

use postit_jobs::testkit::{jobs_settings, wait_until};
use postit_jobs::{JobRegistry, Worker, migrate};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn worker_reports_ready_while_running_and_not_after_its_pool_dies(pool: PgPool) {
    migrate(&pool).await.unwrap_or_else(|e| unreachable!("migrate: {e}"));
    let worker = Worker::new(pool.clone(), &jobs_settings(), JobRegistry::default());
    let health = worker.health();
    assert!(!health.is_ready());

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(worker.run(async move {
        let _ = stop_rx.await;
    }));
    let probe = health.clone();
    assert!(wait_until(Duration::from_secs(10), move || probe.is_ready()).await);

    let _ = stop_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(30), handle).await;
    // A clean shutdown must not be reported as a dead worker.
    assert!(health.is_ready());
}
```

- [ ] **Step 4: Wire it**

`worker.rs`: add a `health: WorkerHealth` field (created in `new` with `WorkerHealth::default()`), and:

```rust
    /// Liveness handle for `/ready`. Take it before [`Worker::run`], which consumes the worker.
    #[must_use]
    pub fn health(&self) -> WorkerHealth {
        self.health.clone()
    }
```

In `run`: after `Backend::connect` succeeds, spawn the relay and the recurring tasks into a `JoinSet<()>` named `support` (instead of keeping separate `JoinHandle`s), and spawn `crate::health::supervise(support, stop_rx.clone(), self.health.clone(), "relay or recurring loop")`. Pass `self.health.clone()` into `backend.run(…)` as a new last parameter. Call `self.health.mark_started()` right before `backend.run(…)`. After `backend.run` returns, await the supervise task's `JoinHandle` (it finishes once the support tasks stop on shutdown). Keep the shutdown bridge (`shutdown.await; stop_tx.send(true)`) unchanged.

The relay and recurring functions return `()` today? If `relay::run` returns something else, wrap it: `support.spawn(async move { let _ = relay::run(…).await; })`.

`backend.rs` `Backend::run`: take `health: WorkerHealth` and replace the `while let Some(joined) = tasks.join_next()` loop with `crate::health::supervise(tasks, shutdown, health, "queue worker").await;` (move the `shutdown` receiver in after the last clone is made).

- [ ] **Step 5: Run tests**

Run: `cargo test -p postit-jobs`
Expected: PASS (including every existing worker test).

- [ ] **Step 6: Quality gates and commit**

```bash
git add server/crates/jobs
git commit -m "postit-jobs: WorkerHealth marks the worker not ready when a supervised task dies"
```

---

### Task 5: `postit-identity` — discovery fields and readiness, JWKS status classification, cache generation, listener test rewrite

**Files:**
- Modify: `server/crates/identity/src/discovery.rs`, `server/crates/identity/src/cache.rs`
- Test: `server/crates/identity/tests/discovery.rs`, `server/crates/identity/tests/cache.rs`

**Interfaces:**
- Produces: `DiscoveryDocument { issuer, jwks_uri, userinfo_endpoint, end_session_endpoint, authorization_endpoint: Option<String>, token_endpoint: Option<String> }` (new fields `#[serde(default)]`).
- Produces: `OidcDiscovery::is_loaded(&self) -> bool`, `OidcDiscovery::document(&self) -> Option<DiscoveryDocument>` (the document fetched alongside the most recent successful JWKS load).
- Produces: `JwksSource::jwks` now returns the document too: `async fn jwks(&self) -> Result<(DiscoveryDocument, JwkSet), HttpError>`.
- Produces: `PrincipalCache::generation(&self) -> u64`, `PrincipalCache::insert_if_current(&self, generation: u64, issuer: &str, subject: &str, principal: Principal) -> bool`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/discovery.rs` (use `postit_http::testkit::test_server()` and wiremock like the existing tests there):

```rust
#[tokio::test]
async fn jwks_5xx_is_transient_and_404_is_permanent() {
    let server = postit_http::testkit::test_server().await;
    wiremock::Mock::given(wiremock::matchers::path("/.well-known/openid-configuration"))
        .respond_with(wiremock::ResponseTemplate::new(503))
        .mount(&server)
        .await;
    let source = HttpJwksSource::new(test_client(), url::Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")));
    let err = source.discovery().await.err();
    assert!(matches!(err, Some(postit_http::HttpError::Transient(_))), "got {err:?}");

    let missing = postit_http::testkit::test_server().await;
    wiremock::Mock::given(wiremock::matchers::path("/.well-known/openid-configuration"))
        .respond_with(wiremock::ResponseTemplate::new(404))
        .mount(&missing)
        .await;
    let source = HttpJwksSource::new(test_client(), url::Url::parse(&missing.uri()).unwrap_or_else(|e| unreachable!("{e}")));
    let err = source.discovery().await.err();
    assert!(matches!(err, Some(postit_http::HttpError::Permanent(_))), "got {err:?}");
}

#[tokio::test]
async fn is_loaded_flips_after_the_first_jwks_and_document_exposes_endpoints() {
    let issuer = postit_identity::testkit::TestIssuer::start().await;
    let discovery = OidcDiscovery::new(
        HttpJwksSource::new(test_client(), issuer.issuer_url()),
        std::time::Duration::from_secs(3600),
    );
    assert!(!discovery.is_loaded());
    assert!(discovery.document().is_none());
    discovery.jwks().await.unwrap_or_else(|e| unreachable!("jwks: {e}"));
    assert!(discovery.is_loaded());
    let doc = discovery.document().unwrap_or_else(|| unreachable!("document cached"));
    assert!(doc.authorization_endpoint.is_some_and(|u| u.ends_with("/authorize")));
    assert!(doc.token_endpoint.is_some_and(|u| u.ends_with("/token")));
}
```

`test_client()` is the file's existing client helper; if it has none, build one with `postit_http::build_client(&postit_config::HttpSettings { connect_timeout: 5s, request_timeout: 5s, user_agent: "t".into(), extra_ca_files: vec![] })`.

In `testkit.rs` `TestIssuer::start`, add to `discovery_body`:

```rust
            "authorization_endpoint": format!("{}/authorize", server.uri()),
            "token_endpoint": format!("{}/token", server.uri()),
```

In `tests/cache.rs`, add:

```rust
#[test]
fn insert_if_current_drops_a_write_that_raced_an_invalidation() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    let before = cache.generation();
    cache.invalidate_user(user_id); // a concurrent admin change lands mid-miss
    assert!(!cache.insert_if_current(before, "https://issuer.test", "sub-1", principal(user_id)));
    assert!(cache.get("https://issuer.test", "sub-1").is_none());

    let now = cache.generation();
    assert!(cache.insert_if_current(now, "https://issuer.test", "sub-1", principal(user_id)));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());
}
```

Replace `a_status_change_notification_evicts_that_user_in_a_second_process` with:

```rust
#[sqlx::test(migrations = "../data/migrations")]
async fn a_status_change_notification_evicts_that_user_in_a_second_process(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let cache = PrincipalCache::new(Duration::from_secs(60));
    let before_listen = cache.generation();
    let listener_task = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));

    // Wait until the session is LISTENing: its backend is visible with our application_name,
    // and its post-listen invalidate_all() has advanced the generation. Only then is an
    // inserted entry safe from that initial clear, so the eviction below is the NOTIFY's.
    let listening = postit_jobs::testkit::wait_until(Duration::from_secs(10), || {
        cache.generation() > before_listen
    })
    .await;
    assert!(listening, "listener never reached LISTEN");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_stat_activity WHERE application_name = $1",
    )
    .bind(LISTENER_APPLICATION_NAME)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("pg_stat_activity: {e}"));
    assert_eq!(count, 1);

    cache.insert("https://issuer.test", "sub-1", principal(user_id));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());

    UsersRepo::set_status(&mut conn, user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    let evicted = postit_jobs::testkit::wait_until(Duration::from_secs(5), || {
        cache.get("https://issuer.test", "sub-1").is_none()
    })
    .await;
    assert!(evicted, "NOTIFY did not evict the user");
    listener_task.abort();
}
```

`postit-jobs` with `testkit` is already a dev-dependency of `postit-identity`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-identity --test discovery --test cache`
Expected: compile errors for the missing methods and fields.

- [ ] **Step 3: Implement `discovery.rs`**

Add the two fields to `DiscoveryDocument` with `#[serde(default)]`. Classify statuses in `HttpJwksSource` with a helper:

```rust
fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, HttpError> {
    let retry_after = postit_http::retry_after_from_headers(response.headers());
    match postit_http::classify_status(response.status(), retry_after) {
        Some(err) => Err(err),
        None => Ok(response),
    }
}
```

and call it on both responses: `let response = ensure_success(postit_http::execute_traced(&self.client, request).await?)?;`.

Change the trait to return the document with the set, so the cache holds the matching document without a second fetch:

```rust
#[async_trait]
pub trait JwksSource: Send + Sync {
    async fn discovery(&self) -> Result<DiscoveryDocument, HttpError>;
    /// The discovery document and the JWKS it points at, fetched together.
    async fn jwks(&self) -> Result<(DiscoveryDocument, JwkSet), HttpError>;
}
```

`HttpJwksSource::jwks` returns `Ok((discovery, set))`. Update any other `JwksSource` implementation in tests (`rg "impl JwksSource" server/crates`) the same way.

`CacheState` gains `document: Option<DiscoveryDocument>`. `refetch` stores both. Add:

```rust
    /// True once any JWKS has been cached. A later failed refresh keeps the stale set, so
    /// this stays true — matching [`OidcDiscovery::jwks`]'s stale fallback.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .jwks
            .is_some()
    }

    /// The discovery document from the most recent successful JWKS load.
    #[must_use]
    pub fn document(&self) -> Option<DiscoveryDocument> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .document
            .clone()
    }
```

- [ ] **Step 4: Implement `cache.rs`**

Add a generation counter:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
```

Field `generation: Arc<AtomicU64>` (initialized to 0 in `new`). `invalidate_user` and `invalidate_all` each call `self.generation.fetch_add(1, Ordering::AcqRel);` after invalidating. Add:

```rust
    /// Snapshot taken before a cache-miss database read; see [`Self::insert_if_current`].
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Inserts only if no invalidation happened since `generation` was read, so a principal
    /// loaded before a concurrent admin change can't be cached after that change's eviction.
    /// A single global counter is coarse — any eviction discards concurrent inserts, which
    /// just become the next request's miss — but correct and allocation-free.
    pub fn insert_if_current(
        &self,
        generation: u64,
        issuer: &str,
        subject: &str,
        principal: Principal,
    ) -> bool {
        if self.generation() != generation {
            return false;
        }
        self.insert(issuer, subject, principal);
        // Re-check: an invalidation between the check and the insert must win.
        if self.generation() != generation {
            self.principals.invalidate(&principal.user_id);
            return false;
        }
        true
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p postit-identity`
Expected: PASS.

- [ ] **Step 6: Quality gates and commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: discovery readiness and endpoints, JWKS status classification, principal cache generation, real cross-process eviction test"
```

---

### Task 6: `postit-identity` — `Authenticator`, `ProvisionGate`, `check_startup`

**Files:**
- Create: `server/crates/identity/src/auth.rs`, `server/crates/identity/src/bootstrap.rs`
- Modify: `server/crates/identity/src/lib.rs`, `server/crates/identity/src/error.rs`, `server/crates/identity/src/testkit.rs`
- Test: `server/crates/identity/tests/authenticator.rs`, `server/crates/identity/tests/bootstrap.rs`

**Interfaces:**
- Consumes: `OidcDiscovery::{jwks_for_kid, jwks, is_loaded, document}`, `PrincipalCache::{get, generation, insert_if_current}` (Task 5), `ClaimsTransformer::transform`, `UsersRepo::{find_by_oidc, touch_last_seen}`.
- Produces (all in `postit_identity::auth`):

```rust
pub struct RateLimited { pub retry_after: std::time::Duration }
pub trait ProvisionGate: Send + Sync { fn check(&self) -> Result<(), RateLimited>; }
pub struct AllowAll; // impl ProvisionGate, always Ok — tests and non-HTTP callers
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid token: {0}")] Invalid(VerifyError),
    #[error("signing keys are not available yet")] JwksUnavailable,
    #[error("provisioning rate limited")] RateLimited(RateLimited),
    #[error(transparent)] Internal(IdentityError),
}
#[async_trait] pub trait Authenticate: Send + Sync {
    async fn authenticate(&self, bearer: &SecretString, gate: &dyn ProvisionGate) -> Result<Principal, AuthError>;
    async fn prefetch(&self) -> Result<(), postit_http::HttpError>;
    fn jwks_ready(&self) -> bool;
    fn discovery_document(&self) -> Option<DiscoveryDocument>;
    fn invalidate_user(&self, user: UserId);   // local eviction after an admin change (plan 01); pg_notify covers other processes
}
pub struct AuthenticatorParts<S: JwksSource> { pub verifier: Verifier, pub discovery: Arc<OidcDiscovery<S>>, pub cache: PrincipalCache, pub transformer: ClaimsTransformer<S>, pub pool: PgPool }
pub struct Authenticator<S: JwksSource> { … }
impl<S: JwksSource + 'static> Authenticator<S> { pub fn new(parts: AuthenticatorParts<S>) -> Self; }
impl<S: JwksSource + 'static> Authenticate for Authenticator<S> { … }
```

`RateLimited` derives `Debug, Clone, Copy`.
- Produces: `postit_identity::bootstrap::check_startup(pool: &PgPool, bootstrap: &BootstrapSettings, env: Environment) -> Result<(), IdentityError>` and `IdentityError::BootstrapRequired`.
- Produces (testkit): `pub fn authenticator(pool: PgPool, issuer: &TestIssuer, bootstrap: BootstrapSettings) -> Authenticator<HttpJwksSource>` and `pub fn authenticator_with_cache(pool, issuer, bootstrap, cache: PrincipalCache) -> Authenticator<HttpJwksSource>`.

- [ ] **Step 1: Write the failing tests**

`server/crates/identity/tests/authenticator.rs`:

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use postit_config::BootstrapSettings;
use postit_data::users::{UserStatus, UsersRepo};
use postit_identity::VerifyError;
use postit_identity::auth::{AllowAll, AuthError, Authenticate, ProvisionGate, RateLimited};
use postit_identity::cache::PrincipalCache;
use postit_identity::testkit::{TestIssuer, authenticator, authenticator_with_cache};
use secrecy::SecretString;
use sqlx::PgPool;

fn no_bootstrap() -> BootstrapSettings {
    BootstrapSettings { admin_email: None, admin_subject: None }
}

fn bearer(issuer: &TestIssuer, sub: &str) -> SecretString {
    let (token, _) = issuer.token_and_claims(sub, Some(&format!("{sub}@postit.test")), Some(true), Some(sub));
    SecretString::from(token)
}

async fn user_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
}

struct CountingGate {
    calls: AtomicUsize,
    deny: bool,
}

impl ProvisionGate for CountingGate {
    fn check(&self) -> Result<(), RateLimited> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.deny {
            Err(RateLimited { retry_after: Duration::from_secs(7) })
        } else {
            Ok(())
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn first_request_provisions_once_and_sets_last_seen(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");

    let principal = auth.authenticate(&token, &AllowAll).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(principal.status, UserStatus::Pending);
    let again = auth.authenticate(&token, &AllowAll).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(again.user_id, principal.user_id);
    assert_eq!(user_count(&pool).await, 1);

    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let row = UsersRepo::find_by_id(&mut conn, principal.user_id)
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("row exists"));
    assert!(row.last_seen_at.is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_cache_hit_skips_the_database(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");
    auth.authenticate(&token, &AllowAll).await.unwrap_or_else(|e| unreachable!("auth: {e}"));

    // Close the pool: a second authenticate must be served from the cache alone.
    pool.close().await;
    let cached = auth.authenticate(&token, &AllowAll).await;
    assert!(cached.is_ok(), "got {cached:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_gate_runs_only_when_a_row_would_be_created(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let auth = authenticator_with_cache(pool.clone(), &issuer, no_bootstrap(), cache.clone());
    let gate = CountingGate { calls: AtomicUsize::new(0), deny: false };
    let token = bearer(&issuer, "alice");

    auth.authenticate(&token, &gate).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);

    cache.invalidate_all(); // force a miss for an existing user
    auth.authenticate(&token, &gate).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1, "existing user must not hit the gate");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_gate_denial_creates_nothing(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let gate = CountingGate { calls: AtomicUsize::new(0), deny: true };

    let result = auth.authenticate(&bearer(&issuer, "alice"), &gate).await;
    assert!(matches!(result, Err(AuthError::RateLimited(r)) if r.retry_after == Duration::from_secs(7)));
    assert_eq!(user_count(&pool).await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_invalid_token_never_touches_the_database(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let other = TestIssuer::start().await; // different keys, same kid naming scheme
    let foreign = bearer(&other, "mallory");

    let result = auth.authenticate(&foreign, &AllowAll).await;
    assert!(matches!(result, Err(AuthError::Invalid(_))), "got {result:?}");
    assert_eq!(user_count(&pool).await, 0);

    let garbage = auth.authenticate(&SecretString::from("not.a.jwt".to_string()), &AllowAll).await;
    assert!(matches!(garbage, Err(AuthError::Invalid(VerifyError::Malformed))), "got {garbage:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn jwks_unavailable_before_the_issuer_answers(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let token = bearer(&issuer, "alice");
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    drop(issuer); // wiremock server stops; nothing was ever cached
    assert!(!auth.jwks_ready());
    let result = auth.authenticate(&token, &AllowAll).await;
    assert!(matches!(result, Err(AuthError::JwksUnavailable)), "got {result:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn prefetch_makes_jwks_ready(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool, &issuer, no_bootstrap());
    assert!(!auth.jwks_ready());
    auth.prefetch().await.unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    assert!(auth.jwks_ready());
    assert!(auth.discovery_document().is_some());
}
```

The two `TestIssuer`s mint kids from one global counter (`test-rsa-N`), so a token from `other` carries a kid the first issuer never published: it resolves as `UnknownKid` (after one allowed refetch), which is `Invalid`. If the `Keys` counter ever produced colliding kids, the signature check would still reject it — either way `Invalid`.

`server/crates/identity/tests/bootstrap.rs`:

```rust
use postit_config::{BootstrapSettings, Environment};
use postit_core::UserId;
use postit_data::users::UsersRepo;
use postit_identity::IdentityError;
use postit_identity::bootstrap::check_startup;
use sqlx::PgPool;

fn none() -> BootstrapSettings {
    BootstrapSettings { admin_email: None, admin_subject: None }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_refuses_with_no_admin_and_no_rule(pool: PgPool) {
    let result = check_startup(&pool, &none(), Environment::Production).await;
    assert!(matches!(result, Err(IdentityError::BootstrapRequired)), "got {result:?}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_passes_with_a_rule(pool: PgPool) {
    let rule = BootstrapSettings { admin_email: Some("admin@postit.test".into()), admin_subject: None };
    assert!(check_startup(&pool, &rule, Environment::Production).await.is_ok());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn production_passes_with_an_active_admin(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://i.test", "root", "Root")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, id).await.unwrap_or_else(|e| unreachable!("grant: {e}"));
    assert!(check_startup(&pool, &none(), Environment::Production).await.is_ok());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn development_and_qa_never_refuse(pool: PgPool) {
    assert!(check_startup(&pool, &none(), Environment::Development).await.is_ok());
    assert!(check_startup(&pool, &none(), Environment::Qa).await.is_ok());
}
```

If `postit_config::Environment` is not re-exported at the crate root, import it from its module (`rg "pub use" server/crates/config/src/lib.rs`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-identity --test authenticator --test bootstrap`
Expected: compile errors.

- [ ] **Step 3: Implement `bootstrap.rs` and the error variant**

`error.rs` — add to `IdentityError`:

```rust
    #[error(
        "production has no active admin and no bootstrap rule: set auth.bootstrap.admin_email \
         (POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL) or auth.bootstrap.admin_subject"
    )]
    BootstrapRequired,
```

`bootstrap.rs`:

```rust
//! Startup checks for the first-admin rule (plan 02, `postit-identity`).

use postit_config::{BootstrapSettings, Environment};
use postit_data::DataError;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

use crate::error::IdentityError;

/// Refuses a production start that could never get an admin: no active admin exists and
/// neither `admin_email` nor `admin_subject` is configured. Other environments always pass.
///
/// # Errors
///
/// Returns [`IdentityError::BootstrapRequired`] in that case, or [`IdentityError::Data`] on
/// a database failure.
pub async fn check_startup(
    pool: &PgPool,
    bootstrap: &BootstrapSettings,
    env: Environment,
) -> Result<(), IdentityError> {
    if env != Environment::Production
        || bootstrap.admin_email.is_some()
        || bootstrap.admin_subject.is_some()
    {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(DataError::from)?;
    let admins = UsersRepo::count_active_admins(&mut tx).await?;
    tx.rollback().await.map_err(DataError::from)?;
    if admins == 0 {
        return Err(IdentityError::BootstrapRequired);
    }
    Ok(())
}
```

(`count_active_admins` takes `FOR UPDATE` row locks, hence the short transaction.)

- [ ] **Step 4: Implement `auth.rs`**

```rust
//! The token → `Principal` pipeline. `postit-api`'s `Auth` extractor calls
//! [`Authenticate::authenticate`] and maps [`AuthError`]; everything identity-specific
//! (JWKS, verification, the principal cache, provisioning) stays here.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_data::DataError;
use postit_data::users::UsersRepo;
use postit_http::HttpError;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

use crate::cache::PrincipalCache;
use crate::claims::ClaimsTransformer;
use crate::discovery::{DiscoveryDocument, JwksSource, OidcDiscovery};
use crate::error::{IdentityError, VerifyError};
use crate::principal::Principal;
use crate::verifier::Verifier;

#[derive(Debug, Clone, Copy)]
pub struct RateLimited {
    pub retry_after: Duration,
}

/// Asked once, just before a request would create a `users` row, so the API can apply its
/// stricter provisioning rate limit (keyed by client IP) without knowing identity internals.
pub trait ProvisionGate: Send + Sync {
    /// # Errors
    ///
    /// Returns [`RateLimited`] when the caller must not provision right now.
    fn check(&self) -> Result<(), RateLimited>;
}

/// A gate that never refuses: tests and callers without an HTTP client IP.
pub struct AllowAll;

impl ProvisionGate for AllowAll {
    fn check(&self) -> Result<(), RateLimited> {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    Invalid(VerifyError),
    #[error("signing keys are not available yet")]
    JwksUnavailable,
    #[error("provisioning rate limited")]
    RateLimited(RateLimited),
    #[error(transparent)]
    Internal(IdentityError),
}

#[async_trait]
pub trait Authenticate: Send + Sync {
    /// # Errors
    ///
    /// See [`AuthError`]. Status (pending, disabled, deleting) is not checked here.
    async fn authenticate(
        &self,
        bearer: &SecretString,
        gate: &dyn ProvisionGate,
    ) -> Result<Principal, AuthError>;

    /// Loads the JWKS (and discovery document) if nothing is cached yet.
    ///
    /// # Errors
    ///
    /// Returns [`HttpError`] if the fetch fails.
    async fn prefetch(&self) -> Result<(), HttpError>;

    fn jwks_ready(&self) -> bool;

    fn discovery_document(&self) -> Option<DiscoveryDocument>;

    /// Evicts `user` from this process's principal cache. Admin changes call it after
    /// committing (plan 01, "Cache invalidation across processes"); the `pg_notify` those
    /// changes send covers every other process.
    fn invalidate_user(&self, user: postit_core::UserId);
}

pub struct AuthenticatorParts<S: JwksSource> {
    pub verifier: Verifier,
    pub discovery: Arc<OidcDiscovery<S>>,
    pub cache: PrincipalCache,
    pub transformer: ClaimsTransformer<S>,
    pub pool: PgPool,
}

pub struct Authenticator<S: JwksSource> {
    parts: AuthenticatorParts<S>,
}

impl<S: JwksSource + 'static> Authenticator<S> {
    #[must_use]
    pub fn new(parts: AuthenticatorParts<S>) -> Self {
        Self { parts }
    }
}

#[async_trait]
impl<S: JwksSource + 'static> Authenticate for Authenticator<S> {
    #[tracing::instrument(skip_all)]
    async fn authenticate(
        &self,
        bearer: &SecretString,
        gate: &dyn ProvisionGate,
    ) -> Result<Principal, AuthError> {
        let p = &self.parts;
        let token = bearer.expose_secret();
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| AuthError::Invalid(VerifyError::Malformed))?;
        let kid = header
            .kid
            .ok_or(AuthError::Invalid(VerifyError::UnknownKid))?;
        let jwks = p
            .discovery
            .jwks_for_kid(&kid)
            .await
            .map_err(|_| AuthError::JwksUnavailable)?;
        let verified = p.verifier.verify(token, &jwks).map_err(AuthError::Invalid)?;

        if let Some(principal) = p.cache.get(&verified.iss, &verified.sub) {
            return Ok(principal);
        }

        let generation = p.cache.generation();
        let mut conn = p
            .pool
            .acquire()
            .await
            .map_err(|e| AuthError::Internal(DataError::from(e).into()))?;
        let existing = UsersRepo::find_by_oidc(&mut conn, &verified.iss, &verified.sub)
            .await
            .map_err(|e| AuthError::Internal(e.into()))?;
        drop(conn);
        if existing.is_none() {
            gate.check().map_err(AuthError::RateLimited)?;
        }
        let record = p
            .transformer
            .transform(&verified, token)
            .await
            .map_err(AuthError::Internal)?;
        let mut conn = p
            .pool
            .acquire()
            .await
            .map_err(|e| AuthError::Internal(DataError::from(e).into()))?;
        UsersRepo::touch_last_seen(&mut conn, record.id)
            .await
            .map_err(|e| AuthError::Internal(e.into()))?;

        let principal = Principal::from(&record);
        p.cache
            .insert_if_current(generation, &verified.iss, &verified.sub, principal);
        Ok(principal)
    }

    async fn prefetch(&self) -> Result<(), HttpError> {
        self.parts.discovery.jwks().await.map(|_| ())
    }

    fn jwks_ready(&self) -> bool {
        self.parts.discovery.is_loaded()
    }

    fn discovery_document(&self) -> Option<DiscoveryDocument> {
        self.parts.discovery.document()
    }

    fn invalidate_user(&self, user: postit_core::UserId) {
        self.parts.cache.invalidate_user(user);
    }
}
```

Add to `tests/authenticator.rs`:

```rust
#[sqlx::test(migrations = "../data/migrations")]
async fn invalidate_user_forces_a_fresh_read(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let auth = authenticator(pool.clone(), &issuer, no_bootstrap());
    let token = bearer(&issuer, "alice");
    let first = auth.authenticate(&token, &AllowAll).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    UsersRepo::set_status(&mut conn, first.user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));
    auth.invalidate_user(first.user_id);
    let fresh = auth.authenticate(&token, &AllowAll).await.unwrap_or_else(|e| unreachable!("auth: {e}"));
    assert_eq!(fresh.status, UserStatus::Active);
}
```

Adjust `find_by_oidc`'s argument order to its real signature (`rg "pub async fn find_by_oidc" -A6 server/crates/data/src/users.rs`). `#[tracing::instrument(skip_all)]` keeps the bearer out of spans.

`lib.rs`: add `pub mod auth;` and `pub mod bootstrap;`.

- [ ] **Step 5: Implement the testkit builders**

In `testkit.rs`, refactor `claims_transformer` so its client/discovery construction is shared, then add:

```rust
/// An [`Authenticator`](crate::auth::Authenticator) wired against `issuer` for tests:
/// audience `"postit"`, RS256 and ES256, zero leeway, a 60 s principal cache.
#[must_use]
pub fn authenticator(
    pool: sqlx::PgPool,
    issuer: &TestIssuer,
    bootstrap: postit_config::BootstrapSettings,
) -> crate::auth::Authenticator<crate::discovery::HttpJwksSource> {
    authenticator_with_cache(
        pool,
        issuer,
        bootstrap,
        crate::cache::PrincipalCache::new(std::time::Duration::from_secs(60)),
    )
}

#[must_use]
pub fn authenticator_with_cache(
    pool: sqlx::PgPool,
    issuer: &TestIssuer,
    bootstrap: postit_config::BootstrapSettings,
    cache: crate::cache::PrincipalCache,
) -> crate::auth::Authenticator<crate::discovery::HttpJwksSource> {
    let transformer = claims_transformer(
        pool.clone(),
        issuer,
        postit_config::UserinfoMode::Never,
        bootstrap,
    );
    let issuer_str = issuer.issuer_url().to_string().trim_end_matches('/').to_string();
    crate::auth::Authenticator::new(crate::auth::AuthenticatorParts {
        verifier: crate::verifier::Verifier::new(
            issuer_str,
            vec!["postit".to_string()],
            vec![JwtAlgorithm::RS256, JwtAlgorithm::ES256],
            std::time::Duration::from_secs(0),
        ),
        discovery: transformer.discovery(),
        cache,
        transformer,
        pool,
    })
}
```

This needs the transformer and authenticator to share one `OidcDiscovery`: add `pub fn discovery(&self) -> Arc<OidcDiscovery<S>>` to `ClaimsTransformer` (returns `Arc::clone(&self.discovery)`).

- [ ] **Step 6: Run tests**

Run: `cargo test -p postit-identity`
Expected: PASS.

- [ ] **Step 7: Quality gates and commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: Authenticator (token to Principal), ProvisionGate, production bootstrap startup check"
```

---

### Task 7: `postit-api` — crate setup, problem errors, client IP, keyed limiter

**Files:**
- Modify: `server/Cargo.toml` (`[workspace.dependencies]`), `server/crates/api/Cargo.toml`
- Create: `server/crates/api/src/lib.rs` (replace stub), `server/crates/api/src/error.rs`, `server/crates/api/src/client_ip.rs`, `server/crates/api/src/limit.rs`
- Test: unit tests in each new file

**Interfaces:**
- Produces (`postit_api::error`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode { Unauthenticated, AccountPending, AccountDisabled, Forbidden, NotFound, RequestTimeout,
    UserDeleting, LastAdmin, IdempotencyInProgress, VersionConflict, PayloadTooLarge, ValidationFailed,
    IdempotencyKeyReused, PreconditionRequired, RateLimited, Internal, Unavailable }
impl ErrorCode { pub fn as_str(self) -> &'static str; pub fn status(self) -> StatusCode; pub fn title(self) -> &'static str; }
#[derive(Debug, Clone)] pub struct Problem { pub code: ErrorCode, pub detail: Option<String>, pub retry_after: Option<Duration> }
#[derive(Debug)] pub struct ApiError(pub Problem);
impl ApiError { pub fn new(code) -> Self; pub fn with_detail(self, impl Into<String>) -> Self; pub fn with_retry_after(self, Duration) -> Self; pub fn internal(err: &dyn std::fmt::Display) -> Self; }
impl IntoResponse for ApiError
impl From<IdentityError> for ApiError; impl From<AuthError> for ApiError; impl From<DataError> for ApiError
pub async fn render_problems(req: Request, next: Next) -> Response  // middleware
```

- Produces (`postit_api::client_ip`): `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ClientIp(pub IpAddr)`, `pub fn parse_trusted_proxies(raw: &[String]) -> Result<Vec<IpNet>, String>`, `pub fn resolve(peer: IpAddr, xff: Option<&str>, trusted: &[IpNet]) -> IpAddr`.
- Produces (`postit_api::limit`): `pub struct KeyedLimiter<K>` with `pub fn new(bucket: &RateBucket) -> Self`, `pub fn check(&self, key: &K) -> Result<(), Duration>` (Err = retry-after), `pub fn retain_recent(&self)`.

- [ ] **Step 1: Add dependencies**

`server/Cargo.toml` `[workspace.dependencies]` (external block, alphabetical):

```toml
axum = { version = "0.8", features = ["macros"] }
axum-server = { version = "0.8", default-features = false, features = ["tls-rustls-no-provider"] }
emixcrypto = { version = "0", default-features = false, features = ["sha2"] }
governor = "0.10"
http-body-util = "0.1"
ipnet = "2"
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
tokio-util = "0.7"
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6", features = ["cors", "limit", "request-id", "sensitive-headers", "timeout", "trace", "util"] }
utoipa = { version = "6", features = ["chrono", "uuid", "url"] }
utoipa-swagger-ui = { version = "10", features = ["axum", "vendored"] }
```

and in the internal block: `postit-server = { path = "crates/server" }` is not needed; leave internal entries as they are.

`server/crates/api/Cargo.toml`:

```toml
[dependencies]
postit-config.workspace = true
postit-core.workspace = true
postit-data.workspace = true
postit-http.workspace = true
postit-identity.workspace = true
postit-jobs.workspace = true
postit-mail.workspace = true
async-trait.workspace = true
axum.workspace = true
chrono.workspace = true
emixcrypto.workspace = true
emixdb.workspace = true
governor.workspace = true
ipnet.workspace = true
secrecy.workspace = true
serde.workspace = true
serde_json.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tokio.workspace = true
tower.workspace = true
tower-http.workspace = true
tracing.workspace = true
url.workspace = true
utoipa.workspace = true
utoipa-swagger-ui.workspace = true
uuid.workspace = true

http-body-util = { workspace = true, optional = true }
wiremock = { workspace = true, optional = true }

[dev-dependencies]
postit-api = { path = ".", features = ["testkit"] }
postit-identity = { workspace = true, features = ["testkit"] }
postit-jobs = { workspace = true, features = ["testkit"] }
http-body-util.workspace = true
tracing-subscriber.workspace = true

[features]
testkit = ["dep:http-body-util", "postit-identity/testkit", "postit-jobs/testkit"]
```

Drop the `wiremock` optional line if Task 8's testkit does not need it directly. Run `cargo check -p postit-api` and `cargo tree -d -p postit-api | rg "utoipa|axum |tower-http"`. If `utoipa-swagger-ui` 10 pulls a different `utoipa` major than 6, set `utoipa` to the major it requires; if it pulls a different `axum`, pick the `utoipa-swagger-ui` release built on axum 0.8. Record the resolved versions in the commit message.

- [ ] **Step 2: Write `error.rs` with its tests**

```rust
//! RFC 9457 problem+json. Handlers return `ApiError`; `render_problems` writes the body so it
//! can include the request ID, and also turns bare 404/408/413 responses from the router and
//! tower-http layers into problems.

use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use postit_data::DataError;
use postit_identity::IdentityError;
use postit_identity::auth::AuthError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unauthenticated,
    AccountPending,
    AccountDisabled,
    Forbidden,
    NotFound,
    RequestTimeout,
    UserDeleting,
    LastAdmin,
    IdempotencyInProgress,
    VersionConflict,
    PayloadTooLarge,
    ValidationFailed,
    IdempotencyKeyReused,
    PreconditionRequired,
    RateLimited,
    Internal,
    Unavailable,
}

impl ErrorCode {
    pub const ALL: [Self; 17] = [
        Self::Unauthenticated, Self::AccountPending, Self::AccountDisabled, Self::Forbidden,
        Self::NotFound, Self::RequestTimeout, Self::UserDeleting, Self::LastAdmin,
        Self::IdempotencyInProgress, Self::VersionConflict, Self::PayloadTooLarge,
        Self::ValidationFailed, Self::IdempotencyKeyReused, Self::PreconditionRequired,
        Self::RateLimited, Self::Internal, Self::Unavailable,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::AccountPending => "account_pending",
            Self::AccountDisabled => "account_disabled",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::RequestTimeout => "request_timeout",
            Self::UserDeleting => "user_deleting",
            Self::LastAdmin => "last_admin",
            Self::IdempotencyInProgress => "idempotency_in_progress",
            Self::VersionConflict => "version_conflict",
            Self::PayloadTooLarge => "payload_too_large",
            Self::ValidationFailed => "validation_failed",
            Self::IdempotencyKeyReused => "idempotency_key_reused",
            Self::PreconditionRequired => "precondition_required",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
        }
    }

    #[must_use]
    pub fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::AccountPending | Self::AccountDisabled | Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::UserDeleting | Self::LastAdmin | Self::IdempotencyInProgress => StatusCode::CONFLICT,
            Self::VersionConflict => StatusCode::PRECONDITION_FAILED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::ValidationFailed | Self::IdempotencyKeyReused => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionRequired => StatusCode::PRECONDITION_REQUIRED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Unauthenticated => "Authentication required",
            Self::AccountPending => "Account pending approval",
            Self::AccountDisabled => "Account disabled",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not found",
            Self::RequestTimeout => "Request timed out",
            Self::UserDeleting => "User is being deleted",
            Self::LastAdmin => "Last active admin",
            Self::IdempotencyInProgress => "Request with this key is in progress",
            Self::VersionConflict => "Version conflict",
            Self::PayloadTooLarge => "Payload too large",
            Self::ValidationFailed => "Validation failed",
            Self::IdempotencyKeyReused => "Idempotency key reused",
            Self::PreconditionRequired => "Precondition required",
            Self::RateLimited => "Too many requests",
            Self::Internal => "Internal error",
            Self::Unavailable => "Service unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub code: ErrorCode,
    pub detail: Option<String>,
    pub retry_after: Option<Duration>,
}

#[derive(Debug)]
pub struct ApiError(pub Problem);

impl ApiError {
    #[must_use]
    pub fn new(code: ErrorCode) -> Self {
        Self(Problem { code, detail: None, retry_after: None })
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.0.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn with_retry_after(mut self, retry_after: Duration) -> Self {
        self.0.retry_after = Some(retry_after);
        self
    }

    /// Logs `err` and returns a detail-free 500.
    #[must_use]
    pub fn internal(err: &dyn std::fmt::Display) -> Self {
        tracing::error!(error = %err, "internal error");
        Self::new(ErrorCode::Internal)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = self.0.code.status().into_response();
        // An internal error never carries detail, whatever the caller attached.
        let mut problem = self.0;
        if problem.code == ErrorCode::Internal {
            problem.detail = None;
        }
        response.extensions_mut().insert(problem);
        response
    }
}

impl From<DataError> for ApiError {
    fn from(err: DataError) -> Self {
        match err {
            DataError::NotFound => Self::new(ErrorCode::NotFound),
            other => Self::internal(&other),
        }
    }
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        match err {
            IdentityError::InvalidTransition(from, to) => Self::new(ErrorCode::ValidationFailed)
                .with_detail(format!("status transition not allowed: {from} -> {to}")),
            IdentityError::LastAdmin => Self::new(ErrorCode::LastAdmin),
            IdentityError::UserDeleting => Self::new(ErrorCode::UserDeleting),
            IdentityError::CannotDeleteSelf => Self::new(ErrorCode::Forbidden)
                .with_detail("use DELETE /api/v1/me to delete your own account"),
            IdentityError::Data(DataError::NotFound) => Self::new(ErrorCode::NotFound),
            other => Self::internal(&other),
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Invalid(_) => Self::new(ErrorCode::Unauthenticated),
            AuthError::JwksUnavailable => Self::new(ErrorCode::Unavailable)
                .with_detail("signing keys are not loaded yet"),
            AuthError::RateLimited(limited) => {
                Self::new(ErrorCode::RateLimited).with_retry_after(limited.retry_after)
            }
            AuthError::Internal(inner) => Self::internal(&inner),
        }
    }
}

/// Outermost-but-one middleware (inside the request-ID layer): writes the problem body for
/// any response carrying a [`Problem`] extension, and converts bare 404/408/413 responses.
pub async fn render_problems(req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut response = next.run(req).await;
    let problem = response.extensions_mut().remove::<Problem>().or_else(|| {
        let bare = response.headers().get(header::CONTENT_TYPE).is_none();
        let code = match response.status() {
            StatusCode::NOT_FOUND if bare => Some(ErrorCode::NotFound),
            StatusCode::REQUEST_TIMEOUT => Some(ErrorCode::RequestTimeout),
            StatusCode::PAYLOAD_TOO_LARGE => Some(ErrorCode::PayloadTooLarge),
            _ => None,
        };
        code.map(|code| Problem { code, detail: None, retry_after: None })
    });
    let Some(problem) = problem else {
        return response;
    };

    let body = serde_json::json!({
        "type": "about:blank",
        "title": problem.code.title(),
        "status": problem.code.status().as_u16(),
        "code": problem.code.as_str(),
        "request_id": request_id,
        "detail": problem.detail,
    });
    let (mut parts, _) = response.into_parts();
    parts.status = problem.code.status();
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    parts.headers.remove(header::CONTENT_LENGTH);
    if problem.code == ErrorCode::Unauthenticated {
        parts.headers.insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer error=\"invalid_token\""),
        );
    }
    if let Some(retry_after) = problem.retry_after {
        let secs = retry_after.as_secs().max(1);
        if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
            parts.headers.insert(header::RETRY_AFTER, value);
        }
    }
    Response::from_parts(parts, Body::from(body.to_string()))
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::routing::get;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use super::*;

    async fn render(err: ApiError) -> (StatusCode, axum::http::HeaderMap, serde_json::Value) {
        let app = Router::new()
            .route("/", get(move || async move { err }))
            .layer(axum::middleware::from_fn(render_problems));
        let req = axum::http::Request::builder()
            .uri("/")
            .header("x-request-id", "0190d5a6-0000-7000-8000-000000000001")
            .body(Body::empty())
            .unwrap_or_else(|e| unreachable!("request: {e}"));
        let res = app.oneshot(req).await.unwrap_or_else(|e| unreachable!("oneshot: {e}"));
        let (parts, body) = res.into_parts();
        let bytes = body.collect().await.unwrap_or_else(|e| unreachable!("body: {e}")).to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (parts.status, parts.headers, json)
    }

    #[tokio::test]
    async fn every_code_renders_problem_json_with_request_id() {
        for code in ErrorCode::ALL {
            let (status, headers, body) = render(ApiError::new(code)).await;
            assert_eq!(status, code.status());
            assert_eq!(headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()), Some("application/problem+json"));
            assert_eq!(body["code"], code.as_str());
            assert_eq!(body["request_id"], "0190d5a6-0000-7000-8000-000000000001");
        }
    }

    #[tokio::test]
    async fn unauthenticated_carries_www_authenticate() {
        let (_, headers, _) = render(ApiError::new(ErrorCode::Unauthenticated)).await;
        assert_eq!(
            headers.get(header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()),
            Some("Bearer error=\"invalid_token\"")
        );
    }

    #[tokio::test]
    async fn rate_limited_carries_retry_after() {
        let err = ApiError::new(ErrorCode::RateLimited).with_retry_after(Duration::from_millis(2500));
        let (_, headers, _) = render(err).await;
        assert_eq!(headers.get(header::RETRY_AFTER).and_then(|v| v.to_str().ok()), Some("2"));
    }

    #[tokio::test]
    async fn internal_never_exposes_the_underlying_error() {
        let err = ApiError::from(DataError::Conflict("SELECT secret FROM vault".into()));
        let (status, _, body) = render(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body["detail"].is_null());
        assert!(!body.to_string().contains("vault"));
    }
}
```

Add a `#[cfg(test)] mod tests` dependency: `http-body-util` is in dev-dependencies. The `Retry-After` test expects whole seconds truncated with a floor of 1 (2.5 s → `2`); keep that behavior.

- [ ] **Step 3: Write `client_ip.rs` with tests**

```rust
//! The client address used for rate limiting and audit: the TCP peer, or — only when the
//! peer is a trusted proxy — the first untrusted hop in `X-Forwarded-For`, read right to left.

use std::net::IpAddr;

use ipnet::IpNet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientIp(pub IpAddr);

/// Parses `server.trusted_proxies`: CIDRs, or bare addresses treated as a single host.
///
/// # Errors
///
/// Returns the offending entry when one is neither.
pub fn parse_trusted_proxies(raw: &[String]) -> Result<Vec<IpNet>, String> {
    raw.iter()
        .map(|entry| {
            entry
                .parse::<IpNet>()
                .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| format!("server.trusted_proxies: not a CIDR or address: {entry}"))
        })
        .collect()
}

#[must_use]
pub fn resolve(peer: IpAddr, xff: Option<&str>, trusted: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let Some(xff) = xff else {
        return peer;
    };
    for hop in xff.rsplit(',').map(str::trim) {
        match hop.parse::<IpAddr>() {
            Ok(ip) if is_trusted(&ip) => {}
            Ok(ip) => return ip,
            Err(_) => return peer,
        }
    }
    peer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap_or_else(|e| unreachable!("{s}: {e}"))
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_for() {
        let trusted = parse_trusted_proxies(&["172.30.0.0/24".into()]).unwrap_or_default();
        assert_eq!(resolve(ip("203.0.113.9"), Some("1.2.3.4"), &trusted), ip("203.0.113.9"));
    }

    #[test]
    fn trusted_peer_uses_the_first_untrusted_hop_from_the_right() {
        let trusted = parse_trusted_proxies(&["172.30.0.0/24".into(), "10.0.0.5".into()]).unwrap_or_default();
        assert_eq!(
            resolve(ip("172.30.0.2"), Some("6.6.6.6, 198.51.100.7, 10.0.0.5"), &trusted),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn garbage_in_forwarded_for_falls_back_to_the_peer() {
        let trusted = parse_trusted_proxies(&["172.30.0.0/24".into()]).unwrap_or_default();
        assert_eq!(resolve(ip("172.30.0.2"), Some("not-an-ip"), &trusted), ip("172.30.0.2"));
    }

    #[test]
    fn invalid_entry_is_reported() {
        assert!(parse_trusted_proxies(&["nope".into()]).is_err());
    }
}
```

- [ ] **Step 4: Write `limit.rs` with tests**

```rust
//! One rate-limiter type for every bucket in `[rate_limit]`.

use std::hash::Hash;
use std::num::NonZeroU32;
use std::time::Duration;

use governor::clock::{Clock, DefaultClock};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use postit_config::RateBucket;

pub struct KeyedLimiter<K: Hash + Eq + Clone> {
    inner: DefaultKeyedRateLimiter<K>,
    clock: DefaultClock,
}

impl<K: Hash + Eq + Clone> KeyedLimiter<K> {
    /// `bucket` is validated non-zero by `postit-config`; zero falls back to 1 defensively.
    #[must_use]
    pub fn new(bucket: &RateBucket) -> Self {
        let rate = NonZeroU32::new(bucket.rate_per_minute).unwrap_or(NonZeroU32::MIN);
        let burst = NonZeroU32::new(bucket.burst).unwrap_or(NonZeroU32::MIN);
        Self {
            inner: RateLimiter::keyed(Quota::per_minute(rate).allow_burst(burst)),
            clock: DefaultClock::default(),
        }
    }

    /// # Errors
    ///
    /// Returns how long to wait when `key` is over its limit.
    pub fn check(&self, key: &K) -> Result<(), Duration> {
        self.inner
            .check_key(key)
            .map_err(|not_until| not_until.wait_time_from(self.clock.now()))
    }

    /// Drops state for keys whose buckets are full again; `postit-server` calls this every
    /// minute so the key maps cannot grow without bound.
    pub fn retain_recent(&self) {
        self.inner.retain_recent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_burst_then_refuses_with_a_wait() {
        let limiter = KeyedLimiter::new(&RateBucket { rate_per_minute: 1, burst: 2 });
        assert!(limiter.check(&"a").is_ok());
        assert!(limiter.check(&"a").is_ok());
        let wait = limiter.check(&"a").err();
        assert!(wait.is_some_and(|w| w > Duration::ZERO));
        assert!(limiter.check(&"b").is_ok(), "keys have separate buckets");
    }
}
```

If the governor 0.10 API names differ (`DefaultKeyedRateLimiter`, `wait_time_from`), adapt to the crate's docs for that version; the behavior tested above is the contract.

- [ ] **Step 5: `lib.rs`**

```rust
//! `postit-api`: the HTTP surface. Thin handlers over `postit-identity` services and the
//! `postit-data` admin repositories; see plan 02 `postit-api`.

pub mod client_ip;
pub mod error;
pub mod limit;
```

- [ ] **Step 6: Run tests and gates**

Run: `cargo test -p postit-api`
Expected: PASS. Then the four gates.

- [ ] **Step 7: Commit**

```bash
git add server/Cargo.toml server/Cargo.lock server/crates/api
git commit -m "postit-api: problem+json errors, client IP resolution, keyed rate limiter"
```

---

### Task 8: `postit-api` — state, settings, readiness, middleware stack, probes, testkit

**Files:**
- Create: `server/crates/api/src/settings.rs`, `server/crates/api/src/state.rs`, `server/crates/api/src/router.rs`, `server/crates/api/src/middleware.rs`, `server/crates/api/src/routes/mod.rs`, `server/crates/api/src/routes/probes.rs`, `server/crates/api/src/testkit.rs`
- Modify: `server/crates/api/src/lib.rs`
- Test: `server/crates/api/tests/middleware.rs`

**Interfaces:**
- Consumes: Task 7 modules; `postit_identity::auth::Authenticate`; `UserAdminService`.
- Produces:

```rust
// settings.rs
pub struct ApiSettings {
    pub environment: postit_config::Environment,
    pub cors_origins: Vec<axum::http::HeaderValue>,
    pub trusted_proxies: Vec<ipnet::IpNet>,
    pub rate_limit: postit_config::RateLimitSettings,
    pub request_timeout: std::time::Duration,
    pub body_limit: usize,
    pub issuer: url::Url,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub account_url: Option<url::Url>,
}
impl ApiSettings { pub fn from_settings(env: Environment, settings: &Settings) -> Result<Self, String>; }

// state.rs
#[async_trait] pub trait Readiness: Send + Sync { async fn check(&self) -> Result<(), &'static str>; }
pub struct Limits { pub ip: KeyedLimiter<IpAddr>, pub user: KeyedLimiter<UserId>, pub provisioning: KeyedLimiter<IpAddr> }
impl Limits { pub fn new(settings: &RateLimitSettings) -> Self; pub fn retain_recent(&self); }
#[derive(Clone)] pub struct AppState { pub auth: Arc<dyn Authenticate>, pub admin: UserAdminService, pub pool: PgPool,
    pub settings: Arc<ApiSettings>, pub limits: Arc<Limits>, pub readiness: Arc<dyn Readiness> }

// router.rs
pub fn api_router(state: AppState) -> axum::Router;          // includes /health, /ready, /api/v1/*
pub fn probe_router(readiness: Arc<dyn Readiness>) -> axum::Router;  // /health, /ready only

// testkit.rs (feature "testkit")
pub struct TestApp { pub router: Router, pub issuer: TestIssuer, pub pool: PgPool, pub state: AppState }
pub struct TestResponse { pub status: StatusCode, pub headers: HeaderMap, pub body: serde_json::Value }
impl TestApp {
    pub async fn start(pool: PgPool) -> Self;
    pub async fn start_with(pool: PgPool, configure: impl FnOnce(&mut ApiSettings)) -> Self;
    pub fn token(&self, sub: &str) -> String;           // email "{sub}@postit.test", verified, name = sub
    pub async fn send(&self, req: Request<Body>) -> TestResponse;
    pub async fn call(&self, method: Method, path: &str, bearer: Option<&str>, body: Option<serde_json::Value>) -> TestResponse;
}
pub const ADMIN_SUB: &str = "admin";   // bootstrap admin: email "admin@postit.test"
pub const PEER: SocketAddr;            // 198.51.100.10:40000, the default ConnectInfo
```

- [ ] **Step 1: `settings.rs`**

```rust
use std::time::Duration;

use axum::http::HeaderValue;
use ipnet::IpNet;
use postit_config::{Environment, RateLimitSettings, Settings};
use url::Url;

use crate::client_ip::parse_trusted_proxies;

/// The slice of `Settings` the HTTP layer reads, validated once at startup.
#[derive(Debug, Clone)]
pub struct ApiSettings {
    pub environment: Environment,
    pub cors_origins: Vec<HeaderValue>,
    pub trusted_proxies: Vec<IpNet>,
    pub rate_limit: RateLimitSettings,
    pub request_timeout: Duration,
    pub body_limit: usize,
    pub issuer: Url,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub account_url: Option<Url>,
}

impl ApiSettings {
    /// # Errors
    ///
    /// Returns a message naming the bad key: a CORS origin that is not a valid header value,
    /// or an invalid `server.trusted_proxies` entry.
    pub fn from_settings(environment: Environment, s: &Settings) -> Result<Self, String> {
        let cors_origins = s
            .cors
            .allowed_origins
            .iter()
            .map(|o| HeaderValue::from_str(o).map_err(|_| format!("cors.allowed_origins: invalid origin {o}")))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            environment,
            cors_origins,
            trusted_proxies: parse_trusted_proxies(&s.server.trusted_proxies)?,
            rate_limit: s.rate_limit.clone(),
            request_timeout: s.server.request_timeout,
            body_limit: s.server.body_limit,
            issuer: s.auth.oidc.issuer.clone(),
            client_id: s.auth.oidc.client_id.clone(),
            scopes: s.auth.oidc.scopes.clone(),
            account_url: s.auth.oidc.account_url.clone(),
        })
    }
}
```

- [ ] **Step 2: `state.rs`**

```rust
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use postit_config::RateLimitSettings;
use postit_core::UserId;
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use sqlx::PgPool;

use crate::limit::KeyedLimiter;
use crate::settings::ApiSettings;

/// What `/ready` asks. `postit-server` composes one per role.
#[async_trait]
pub trait Readiness: Send + Sync {
    /// # Errors
    ///
    /// Returns the name of the first failing check (`database`, `jwks`, `worker`).
    async fn check(&self) -> Result<(), &'static str>;
}

pub struct Limits {
    pub ip: KeyedLimiter<IpAddr>,
    pub user: KeyedLimiter<UserId>,
    pub provisioning: KeyedLimiter<IpAddr>,
}

impl Limits {
    #[must_use]
    pub fn new(settings: &RateLimitSettings) -> Self {
        Self {
            ip: KeyedLimiter::new(&settings.unauthenticated),
            user: KeyedLimiter::new(&settings.authenticated),
            provisioning: KeyedLimiter::new(&settings.provisioning),
        }
    }

    pub fn retain_recent(&self) {
        self.ip.retain_recent();
        self.user.retain_recent();
        self.provisioning.retain_recent();
    }
}

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<dyn Authenticate>,
    pub admin: UserAdminService,
    pub pool: PgPool,
    pub settings: Arc<ApiSettings>,
    pub limits: Arc<Limits>,
    pub readiness: Arc<dyn Readiness>,
}
```

- [ ] **Step 3: `middleware.rs`**

```rust
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;

use crate::client_ip::{ClientIp, resolve};
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

/// Drops an incoming `x-request-id` that is not a UUID, so `SetRequestIdLayer` replaces it
/// with a fresh UUID v7 instead of echoing arbitrary client text into logs and problems.
pub async fn sanitize_request_id(mut req: Request, next: Next) -> Response {
    let valid = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok());
    if !valid {
        req.headers_mut().remove("x-request-id");
    }
    next.run(req).await
}

/// Resolves [`ClientIp`] into the request extensions.
pub async fn client_ip(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let xff = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok());
    let ip = resolve(peer.ip(), xff, &state.settings.trusted_proxies);
    req.extensions_mut().insert(ClientIp(ip));
    next.run(req).await
}

/// The per-IP bucket, for requests without a bearer token. Authenticated requests are
/// charged per user by the `Auth` extractor instead (and to this bucket only when their
/// token fails), so a team behind one NAT address never shares a bucket.
pub async fn ip_rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let has_bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "));
    if !has_bearer
        && let Some(ClientIp(ip)) = req.extensions().get::<ClientIp>().copied()
        && let Err(wait) = state.limits.ip.check(&ip)
    {
        return ApiError::new(ErrorCode::RateLimited).with_retry_after(wait).into_response();
    }
    next.run(req).await
}
```

- [ ] **Step 4: `routes/probes.rs` and `routes/mod.rs`**

```rust
// routes/probes.rs
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::error::{ApiError, ErrorCode};
use crate::state::Readiness;

#[utoipa::path(get, path = "/health", tag = "probes", responses((status = 200, description = "Process is alive")))]
pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[utoipa::path(get, path = "/ready", tag = "probes", responses(
    (status = 200, description = "Ready to serve"),
    (status = 503, description = "A dependency is not ready", content_type = "application/problem+json"),
))]
pub async fn ready(State(readiness): State<Arc<dyn Readiness>>) -> Result<Json<Value>, ApiError> {
    readiness
        .check()
        .await
        .map(|()| Json(json!({ "status": "ready" })))
        .map_err(|failing| ApiError::new(ErrorCode::Unavailable).with_detail(format!("not ready: {failing}")))
}
```

```rust
// routes/mod.rs
pub mod probes;
```

- [ ] **Step 5: `router.rs`**

```rust
use std::sync::Arc;

use axum::Router;
use axum::extract::FromRef;
use axum::http::{HeaderName, Method, header};
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::error::render_problems;
use crate::middleware::{client_ip, ip_rate_limit, sanitize_request_id};
use crate::routes::probes;
use crate::state::{AppState, Readiness};

impl FromRef<AppState> for Arc<dyn Readiness> {
    fn from_ref(state: &AppState) -> Self {
        Arc::clone(&state.readiness)
    }
}

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

fn cors(state: &AppState) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(state.settings.cors_origins.clone()))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("x-postit-act-as"),
            header::IF_MATCH,
            header::IF_NONE_MATCH,
            HeaderName::from_static("idempotency-key"),
        ])
        .expose_headers([header::ETAG, header::LOCATION, header::RETRY_AFTER, REQUEST_ID])
        .allow_credentials(false)
}

/// The API port's router (roles `all` and `api`).
pub fn api_router(state: AppState) -> Router {
    let v1 = crate::routes::v1_router();
    let app = Router::new()
        .route("/health", get(probes::health))
        .route("/ready", get(probes::ready))
        .nest("/api/v1", v1)
        .merge(crate::openapi::docs_router(&state));

    app.layer(
        ServiceBuilder::new()
            .layer(axum::middleware::from_fn(sanitize_request_id))
            .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
            .layer(PropagateRequestIdLayer::new(REQUEST_ID))
            .layer(axum::middleware::from_fn(render_problems))
            .layer(SetSensitiveRequestHeadersLayer::new([header::AUTHORIZATION]))
            .layer(TraceLayer::new_for_http())
            .layer(cors(&state))
            .layer(axum::middleware::from_fn_with_state(state.clone(), client_ip))
            .layer(axum::middleware::from_fn_with_state(state.clone(), ip_rate_limit))
            .layer(TimeoutLayer::new(state.settings.request_timeout))
            .layer(RequestBodyLimitLayer::new(state.settings.body_limit)),
    )
    .with_state(state)
}

/// The worker port's router: `/health` and `/ready` only.
pub fn probe_router(readiness: Arc<dyn Readiness>) -> Router {
    Router::new()
        .route("/health", get(probes::health))
        .route("/ready", get(probes::ready))
        .layer(
            ServiceBuilder::new()
                .layer(axum::middleware::from_fn(sanitize_request_id))
                .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
                .layer(PropagateRequestIdLayer::new(REQUEST_ID))
                .layer(axum::middleware::from_fn(render_problems))
                .layer(TraceLayer::new_for_http()),
        )
        .with_state(readiness)
}
```

`MakeRequestUuid` produces UUID v4; the spec asks for v7. Replace it with a small `MakeRequestId` implementation:

```rust
#[derive(Clone, Copy)]
struct MakeRequestUuidV7;

impl tower_http::request_id::MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _: &axum::http::Request<B>) -> Option<tower_http::request_id::RequestId> {
        axum::http::HeaderValue::from_str(&uuid::Uuid::now_v7().to_string())
            .ok()
            .map(tower_http::request_id::RequestId::new)
    }
}
```

and use `SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuidV7)` in both routers.

For this task, stub the two not-yet-written pieces so the crate compiles: in `routes/mod.rs` add `pub fn v1_router() -> axum::Router<crate::state::AppState> { axum::Router::new() }` (Task 9 fills it), and create `src/openapi.rs` with `pub fn docs_router(_state: &crate::state::AppState) -> axum::Router<crate::state::AppState> { axum::Router::new() }` (Task 13 replaces it). Test-only routes are added to `v1_router` under `#[cfg(any(test, feature = "testkit"))]` in later tasks; this task adds the first one, used by the timeout and body-limit tests:

```rust
#[cfg(any(test, feature = "testkit"))]
pub mod testing;
```

`routes/testing.rs`:

```rust
//! Routes compiled only for tests (`testkit` feature): they exercise middleware and helpers
//! that no plan 02 production route uses yet. Production builds never contain them.

use std::time::Duration;

use axum::Router;
use axum::extract::Query;
use axum::routing::{get, post};

use crate::state::AppState;

#[derive(serde::Deserialize)]
struct Sleep {
    ms: u64,
}

async fn sleep(Query(q): Query<Sleep>) -> &'static str {
    tokio::time::sleep(Duration::from_millis(q.ms)).await;
    "done"
}

async fn echo(body: axum::body::Bytes) -> String {
    body.len().to_string()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sleep", get(sleep))
        .route("/echo", post(echo))
}
```

and in `v1_router`:

```rust
pub fn v1_router() -> axum::Router<crate::state::AppState> {
    let router = axum::Router::new();
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test", testing::router());
    router
}
```

`echo` reads the body as `Bytes`, which is what makes `RequestBodyLimitLayer` reject an oversized body with 413.

- [ ] **Step 6: `testkit.rs`**

```rust
//! Test harness: the real router over a `#[sqlx::test]` pool, the test OIDC issuer, and a
//! real outbox-backed job queue. Feature `testkit`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use postit_config::{BootstrapSettings, Environment, RateBucket, RateLimitSettings};
use postit_core::{IdGenerator, SystemIdGenerator};
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use postit_identity::testkit::{TestIssuer, authenticator};
use postit_jobs::JobQueue;
use postit_mail::MailOutbox;
use sqlx::PgPool;
use tower::ServiceExt as _;

use crate::router::api_router;
use crate::settings::ApiSettings;
use crate::state::{AppState, Limits, Readiness};

pub const ADMIN_SUB: &str = "admin";
pub const PEER: SocketAddr = SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 10)),
    40000,
);

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

pub struct TestApp {
    pub router: Router,
    pub issuer: TestIssuer,
    pub pool: PgPool,
    pub state: AppState,
}

struct DbAndJwks {
    pool: PgPool,
    auth: Arc<dyn Authenticate>,
}

#[async_trait]
impl Readiness for DbAndJwks {
    async fn check(&self) -> Result<(), &'static str> {
        sqlx::query("SELECT 1").execute(&self.pool).await.map_err(|_| "database")?;
        if !self.auth.jwks_ready() {
            return Err("jwks");
        }
        Ok(())
    }
}

fn generous() -> RateBucket {
    RateBucket { rate_per_minute: 10_000, burst: 10_000 }
}

fn default_settings(issuer: &TestIssuer) -> ApiSettings {
    ApiSettings {
        environment: Environment::Development,
        cors_origins: vec![axum::http::HeaderValue::from_static("https://app.postit.test")],
        trusted_proxies: vec![],
        rate_limit: RateLimitSettings {
            unauthenticated: generous(),
            provisioning: generous(),
            authenticated: generous(),
        },
        request_timeout: Duration::from_secs(10),
        body_limit: 64 * 1024,
        issuer: issuer.issuer_url(),
        client_id: "postit-app".into(),
        scopes: vec!["openid".into(), "profile".into(), "email".into()],
        account_url: None,
    }
}

impl TestApp {
    pub async fn start(pool: PgPool) -> Self {
        Self::start_with(pool, |_| {}).await
    }

    pub async fn start_with(pool: PgPool, configure: impl FnOnce(&mut ApiSettings)) -> Self {
        let issuer = TestIssuer::start().await;
        let mut settings = default_settings(&issuer);
        configure(&mut settings);
        let bootstrap = BootstrapSettings {
            admin_email: Some(format!("{ADMIN_SUB}@postit.test")),
            admin_subject: None,
        };
        let auth: Arc<dyn Authenticate> = Arc::new(authenticator(pool.clone(), &issuer, bootstrap));
        let ids: Arc<dyn IdGenerator> = Arc::new(SystemIdGenerator);
        let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
        let admin = UserAdminService::new(pool.clone(), ids, jobs.clone(), MailOutbox::new(jobs));
        let state = AppState {
            readiness: Arc::new(DbAndJwks { pool: pool.clone(), auth: Arc::clone(&auth) }),
            auth,
            admin,
            pool: pool.clone(),
            limits: Arc::new(Limits::new(&settings.rate_limit)),
            settings: Arc::new(settings),
        };
        Self { router: api_router(state.clone()), issuer, pool, state }
    }

    #[must_use]
    pub fn token(&self, sub: &str) -> String {
        let email = format!("{sub}@postit.test");
        self.issuer.token_and_claims(sub, Some(&email), Some(true), Some(sub)).0
    }

    pub async fn send(&self, mut req: Request<Body>) -> TestResponse {
        if req.extensions().get::<ConnectInfo<SocketAddr>>().is_none() {
            req.extensions_mut().insert(ConnectInfo(PEER));
        }
        let res = self
            .router
            .clone()
            .oneshot(req)
            .await
            .unwrap_or_else(|e| unreachable!("router is infallible: {e}"));
        let (parts, body) = res.into_parts();
        let bytes = body
            .collect()
            .await
            .unwrap_or_else(|e| unreachable!("collect body: {e}"))
            .to_bytes();
        TestResponse {
            status: parts.status,
            headers: parts.headers,
            body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        }
    }

    pub async fn call(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let req = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap_or_else(|e| unreachable!("request: {e}"));
        self.send(req).await
    }
}
```

`lib.rs`:

```rust
pub mod client_ip;
pub mod error;
pub mod limit;
pub mod middleware;
pub mod openapi;
pub mod router;
pub mod routes;
pub mod settings;
pub mod state;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use router::{api_router, probe_router};
pub use settings::ApiSettings;
pub use state::{AppState, Limits, Readiness};
```

- [ ] **Step 7: Write the middleware tests**

`server/crates/api/tests/middleware.rs`:

```rust
use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::TestApp;
use postit_config::RateBucket;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn health_is_ok_and_ready_waits_for_the_jwks(pool: PgPool) {
    let app = TestApp::start(pool).await;
    assert_eq!(app.call(Method::GET, "/health", None, None).await.status, StatusCode::OK);

    let not_ready = app.call(Method::GET, "/ready", None, None).await;
    assert_eq!(not_ready.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(not_ready.body["code"], "unavailable");
    assert!(not_ready.body["detail"].as_str().is_some_and(|d| d.contains("jwks")));

    app.state.auth.prefetch().await.unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    assert_eq!(app.call(Method::GET, "/ready", None, None).await.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn request_ids_are_generated_propagated_and_sanitized(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let generated = app.call(Method::GET, "/health", None, None).await;
    let id = generated.headers.get("x-request-id").and_then(|v| v.to_str().ok()).unwrap_or_default();
    assert_eq!(uuid::Uuid::parse_str(id).map(|u| u.get_version_num()).ok(), Some(7));

    let kept_id = "0190d5a6-0000-7000-8000-00000000abcd";
    let req = Request::get("/health").header("x-request-id", kept_id).body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(app.send(req).await.headers.get("x-request-id").and_then(|v| v.to_str().ok()), Some(kept_id));

    let req = Request::get("/health").header("x-request-id", "<script>").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    let replaced = app.send(req).await;
    assert_ne!(replaced.headers.get("x-request-id").and_then(|v| v.to_str().ok()), Some("<script>"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_route_is_a_not_found_problem(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app.call(Method::GET, "/api/v1/nope", None, None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.body["code"], "not_found");
    assert!(res.body["request_id"].is_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cors_preflight_allows_exactly_the_listed_headers(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/v1/_test/echo")
        .header(header::ORIGIN, "https://app.postit.test")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization,idempotency-key")
        .body(Body::empty())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let res = app.send(req).await;
    let allow = res.headers.get(header::ACCESS_CONTROL_ALLOW_HEADERS).and_then(|v| v.to_str().ok()).unwrap_or_default().to_lowercase();
    for h in ["authorization", "content-type", "x-postit-act-as", "if-match", "if-none-match", "idempotency-key"] {
        assert!(allow.contains(h), "missing {h} in {allow}");
    }
    assert!(res.headers.get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());

    let get = Request::get("/health").header(header::ORIGIN, "https://app.postit.test").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    let res = app.send(get).await;
    let expose = res.headers.get(header::ACCESS_CONTROL_EXPOSE_HEADERS).and_then(|v| v.to_str().ok()).unwrap_or_default().to_lowercase();
    for h in ["etag", "location", "retry-after", "x-request-id"] {
        assert!(expose.contains(h), "missing {h} in {expose}");
    }

    let evil = Request::get("/health").header(header::ORIGIN, "https://evil.test").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(app.send(evil).await.headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oversized_body_is_payload_too_large(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.body_limit = 16).await;
    let req = Request::post("/api/v1/_test/echo").body(Body::from(vec![b'x'; 64])).unwrap_or_else(|e| unreachable!("{e}"));
    let res = app.send(req).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "payload_too_large");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn slow_request_is_request_timeout(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.request_timeout = Duration::from_millis(100)).await;
    let res = app.call(Method::GET, "/api/v1/_test/sleep?ms=2000", None, None).await;
    assert_eq!(res.status, StatusCode::REQUEST_TIMEOUT);
    assert_eq!(res.body["code"], "request_timeout");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_requests_share_a_per_ip_bucket(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket { rate_per_minute: 1, burst: 2 };
    })
    .await;
    assert_eq!(app.call(Method::GET, "/health", None, None).await.status, StatusCode::OK);
    assert_eq!(app.call(Method::GET, "/health", None, None).await.status, StatusCode::OK);
    let limited = app.call(Method::GET, "/health", None, None).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.get(header::RETRY_AFTER).is_some());

    let mut other = Request::get("/health").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    other.extensions_mut().insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 50], 1))));
    assert_eq!(app.send(other).await.status, StatusCode::OK, "another IP has its own bucket");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn forwarded_for_is_honored_only_from_a_trusted_proxy(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket { rate_per_minute: 1, burst: 1 };
        s.trusted_proxies = postit_api::client_ip::parse_trusted_proxies(&["10.0.0.0/8".into()]).unwrap_or_default();
    })
    .await;
    let from_proxy = |client: &str| {
        let mut req = Request::get("/health").header("x-forwarded-for", client).body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 2], 1))));
        req
    };
    assert_eq!(app.send(from_proxy("192.0.2.1")).await.status, StatusCode::OK);
    assert_eq!(app.send(from_proxy("192.0.2.2")).await.status, StatusCode::OK, "different client behind the proxy");
    assert_eq!(app.send(from_proxy("192.0.2.1")).await.status, StatusCode::TOO_MANY_REQUESTS);

    // An untrusted peer's X-Forwarded-For is ignored: both requests are charged to the peer.
    let mut spoof = Request::get("/health").header("x-forwarded-for", "192.0.2.9").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    spoof.extensions_mut().insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 7], 1))));
    assert_eq!(app.send(spoof).await.status, StatusCode::OK);
    let mut spoof2 = Request::get("/health").header("x-forwarded-for", "192.0.2.10").body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"));
    spoof2.extensions_mut().insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 7], 1))));
    assert_eq!(app.send(spoof2).await.status, StatusCode::TOO_MANY_REQUESTS);
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p postit-api`
Expected: PASS. If the timeout test sees 408 without a problem body, check that `TimeoutLayer` sits inside `render_problems` (it must be added after it in the `ServiceBuilder`, which makes it inner).

- [ ] **Step 9: Quality gates and commit**

```bash
git add server/crates/api
git commit -m "postit-api: router, middleware stack, probes, and the API testkit"
```

---

### Task 9: `postit-api` — extractors, `/auth/config`, `/me`

**Files:**
- Create: `server/crates/api/src/extract.rs`, `server/crates/api/src/dto.rs`, `server/crates/api/src/routes/auth.rs`, `server/crates/api/src/routes/me.rs`
- Modify: `server/crates/api/src/routes/mod.rs`, `server/crates/api/src/lib.rs`
- Test: `server/crates/api/tests/me.rs`

**Interfaces:**
- Consumes: `Authenticate`, `Limits`, `ClientIp`, `ApiError`.
- Produces (`extract.rs`): `pub struct Auth(pub Principal)`, `pub struct ActiveUser(pub Principal)`, `pub struct RequireAdmin(pub Principal)`, `pub struct Scope(pub OwnerScope)` — all `FromRequestParts<AppState>` with `Rejection = ApiError`.
- Produces (`dto.rs`):

```rust
#[derive(Serialize, ToSchema)] #[serde(rename_all = "snake_case")] pub enum RoleDto { Admin, Member }
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)] #[serde(rename_all = "snake_case")] pub enum StatusDto { Pending, Active, Disabled, Deleting }
#[derive(Serialize, ToSchema)] pub struct UserDto { pub id: Uuid, pub email: Option<String>, pub email_verified: bool, pub display_name: String,
    pub role: RoleDto, pub status: StatusDto, pub approved_at: Option<DateTime<Utc>>, pub approved_by: Option<Uuid>,
    pub last_seen_at: Option<DateTime<Utc>>, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc> }
#[derive(Serialize, ToSchema)] pub struct MeDto { #[serde(flatten)] pub user: UserDto, pub account_url: Option<String> }
#[derive(Serialize, ToSchema)] pub struct AuthConfigDto { pub issuer: String, pub client_id: String, pub scopes: Vec<String> }
#[derive(Deserialize, ToSchema)] pub struct DeleteMeRequest { pub display_name: String }
impl From<&UserRecord> for UserDto
```

`RoleDto` also derives `Deserialize, Clone, Copy, PartialEq, Eq` (Task 10's PATCH body uses it). `From<UserRole> for RoleDto`, `From<RoleDto> for UserRole`, and the same pair for statuses.

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/me.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_config::RateBucket;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn auth_config_is_public(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app.call(Method::GET, "/api/v1/auth/config", None, None).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["client_id"], "postit-app");
    assert!(res.body["issuer"].as_str().is_some_and(|i| i.starts_with("http")));
    assert!(res.body["scopes"].as_array().is_some_and(|s| !s.is_empty()));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn me_requires_a_token(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app.call(Method::GET, "/api/v1/me", None, None).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    assert_eq!(res.body["code"], "unauthenticated");
    let bad = app.call(Method::GET, "/api/v1/me", Some("garbage"), None).await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bootstrap_admin_and_pending_member_see_their_own_profile(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None).await;
    assert_eq!(admin.status, StatusCode::OK);
    assert_eq!(admin.body["role"], "admin");
    assert_eq!(admin.body["status"], "active");
    assert_eq!(admin.body["email"], "admin@postit.test");

    let member = app.call(Method::GET, "/api/v1/me", Some(&app.token("bob")), None).await;
    assert_eq!(member.status, StatusCode::OK);
    assert_eq!(member.body["status"], "pending");
    assert!(member.body.get("account_url").is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_pending_user_cannot_delete_themselves(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token("bob");
    let res = app
        .call(Method::DELETE, "/api/v1/me", Some(&token), Some(serde_json::json!({ "display_name": "bob" })))
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "account_pending");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_me_checks_the_display_name_and_the_last_admin(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let wrong = app
        .call(Method::DELETE, "/api/v1/me", Some(&admin), Some(serde_json::json!({ "display_name": "nope" })))
        .await;
    assert_eq!(wrong.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(wrong.body["code"], "validation_failed");

    let last = app
        .call(Method::DELETE, "/api/v1/me", Some(&admin), Some(serde_json::json!({ "display_name": ADMIN_SUB })))
        .await;
    assert_eq!(last.status, StatusCode::CONFLICT);
    assert_eq!(last.body["code"], "last_admin");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_tokens_are_charged_to_the_ip_and_create_no_user(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), |s| {
        s.rate_limit.unauthenticated = RateBucket { rate_per_minute: 1, burst: 2 };
    })
    .await;
    for _ in 0..2 {
        assert_eq!(app.call(Method::GET, "/api/v1/me", Some("bad.token.here"), None).await.status, StatusCode::UNAUTHORIZED);
    }
    let limited = app.call(Method::GET, "/api/v1/me", Some("bad.token.here"), None).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(users, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn two_users_behind_one_ip_have_separate_buckets(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.authenticated = RateBucket { rate_per_minute: 1, burst: 2 };
        s.rate_limit.unauthenticated = RateBucket { rate_per_minute: 1, burst: 1 };
    })
    .await;
    let (a, b) = (app.token(ADMIN_SUB), app.token("bob"));
    for _ in 0..2 {
        assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&a), None).await.status, StatusCode::OK);
        assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&b), None).await.status, StatusCode::OK);
    }
    assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&a), None).await.status, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_provisioning_bucket_trips_before_a_row_is_created(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), |s| {
        s.rate_limit.provisioning = RateBucket { rate_per_minute: 1, burst: 1 };
    })
    .await;
    assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&app.token("u1")), None).await.status, StatusCode::OK);
    let limited = app.call(Method::GET, "/api/v1/me", Some(&app.token("u2")), None).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&pool).await.unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(users, 1);
    // An existing user is never charged to the provisioning bucket.
    assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&app.token("u1")), None).await.status, StatusCode::OK);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-api --test me`
Expected: FAIL (404 for every route).

- [ ] **Step 3: Implement `extract.rs`**

```rust
use std::net::IpAddr;

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use postit_data::OwnerScope;
use postit_data::users::{UserRole, UserStatus};
use postit_identity::auth::{AuthError, ProvisionGate, RateLimited};
use postit_identity::principal::Principal;
use secrecy::SecretString;

use crate::client_ip::ClientIp;
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

/// Signed in, any status except `disabled` and `deleting` (so `pending` users reach `GET /me`).
pub struct Auth(pub Principal);
/// Signed in and `active`.
pub struct ActiveUser(pub Principal);
/// Active admin. Checks the caller's own role only.
pub struct RequireAdmin(pub Principal);
/// The caller's own workspace. Plan 03 adds `X-Postit-Act-As` here and nowhere else.
pub struct Scope(pub OwnerScope);

struct IpGate<'a> {
    state: &'a AppState,
    ip: IpAddr,
}

impl ProvisionGate for IpGate<'_> {
    fn check(&self) -> Result<(), RateLimited> {
        self.state
            .limits
            .provisioning
            .check(&self.ip)
            .map_err(|retry_after| RateLimited { retry_after })
    }
}

fn bearer(parts: &Parts) -> Option<SecretString> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then(|| SecretString::from(token.to_owned()))
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ClientIp>()
            .map_or(IpAddr::from([0, 0, 0, 0]), |c| c.0);
        let Some(token) = bearer(parts) else {
            return Err(ApiError::new(ErrorCode::Unauthenticated));
        };
        let principal = match state.auth.authenticate(&token, &IpGate { state, ip }).await {
            Ok(principal) => principal,
            Err(AuthError::Invalid(err)) => {
                tracing::debug!(error = %err, "bearer token rejected");
                // A failed token is charged to the IP bucket: bad tokens are bounded per IP.
                if let Err(wait) = state.limits.ip.check(&ip) {
                    return Err(ApiError::new(ErrorCode::RateLimited).with_retry_after(wait));
                }
                return Err(ApiError::new(ErrorCode::Unauthenticated));
            }
            Err(other) => return Err(other.into()),
        };
        if matches!(principal.status, UserStatus::Disabled | UserStatus::Deleting) {
            return Err(ApiError::new(ErrorCode::AccountDisabled));
        }
        if let Err(wait) = state.limits.user.check(&principal.user_id) {
            return Err(ApiError::new(ErrorCode::RateLimited).with_retry_after(wait));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for ActiveUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Auth(principal) = Auth::from_request_parts(parts, state).await?;
        if principal.status == UserStatus::Pending {
            return Err(ApiError::new(ErrorCode::AccountPending));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let ActiveUser(principal) = ActiveUser::from_request_parts(parts, state).await?;
        if principal.role != UserRole::Admin {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        Ok(Self(principal))
    }
}

impl FromRequestParts<AppState> for Scope {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let ActiveUser(principal) = ActiveUser::from_request_parts(parts, state).await?;
        Ok(Self(OwnerScope::own(principal.user_id)))
    }
}
```

axum 0.8 uses native async fn in traits for `FromRequestParts` (no `#[async_trait]`). If `OwnerScope::own` has a different name, use the constructor in `crates/data/src/scope.rs`.

- [ ] **Step 4: Implement `dto.rs`, `routes/auth.rs`, `routes/me.rs`**

`dto.rs` — the types from Interfaces plus conversions:

```rust
use chrono::{DateTime, Utc};
use postit_data::users::{UserRecord, UserRole, UserStatus};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto { Admin, Member }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusDto { Pending, Active, Disabled, Deleting }

impl From<UserRole> for RoleDto {
    fn from(r: UserRole) -> Self { match r { UserRole::Admin => Self::Admin, UserRole::Member => Self::Member } }
}
impl From<RoleDto> for UserRole {
    fn from(r: RoleDto) -> Self { match r { RoleDto::Admin => Self::Admin, RoleDto::Member => Self::Member } }
}
impl From<UserStatus> for StatusDto {
    fn from(s: UserStatus) -> Self {
        match s {
            UserStatus::Pending => Self::Pending,
            UserStatus::Active => Self::Active,
            UserStatus::Disabled => Self::Disabled,
            UserStatus::Deleting => Self::Deleting,
        }
    }
}
impl From<StatusDto> for UserStatus {
    fn from(s: StatusDto) -> Self {
        match s {
            StatusDto::Pending => Self::Pending,
            StatusDto::Active => Self::Active,
            StatusDto::Disabled => Self::Disabled,
            StatusDto::Deleting => Self::Deleting,
        }
    }
}

/// A postit user as admins and the user themselves see it. Profile fields come from the IdP.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserDto {
    pub id: Uuid,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: String,
    pub role: RoleDto,
    pub status: StatusDto,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<Uuid>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&UserRecord> for UserDto {
    fn from(u: &UserRecord) -> Self {
        Self {
            id: u.id.as_uuid(),
            email: u.email.clone(),
            email_verified: u.email_verified,
            display_name: u.display_name.clone(),
            role: u.role.into(),
            status: u.status.into(),
            approved_at: u.approved_at,
            approved_by: u.approved_by.map(|id| id.as_uuid()),
            last_seen_at: u.last_seen_at,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeDto {
    #[serde(flatten)]
    pub user: UserDto,
    /// The IdP's account page (`auth.oidc.account_url`), when configured.
    pub account_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigDto {
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteMeRequest {
    /// Must equal the caller's current display name.
    pub display_name: String,
}
```

`routes/auth.rs`:

```rust
use axum::Json;
use axum::extract::State;

use crate::dto::AuthConfigDto;
use crate::state::AppState;

#[utoipa::path(get, path = "/api/v1/auth/config", tag = "auth",
    responses((status = 200, body = AuthConfigDto)))]
pub async fn config(State(state): State<AppState>) -> Json<AuthConfigDto> {
    let s = &state.settings;
    Json(AuthConfigDto {
        issuer: s.issuer.as_str().trim_end_matches('/').to_string(),
        client_id: s.client_id.clone(),
        scopes: s.scopes.clone(),
    })
}
```

`routes/me.rs`:

```rust
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_data::users::UsersRepo;

use crate::dto::{DeleteMeRequest, MeDto, UserDto};
use crate::error::{ApiError, ErrorCode};
use crate::extract::{ActiveUser, Auth};
use crate::json::ApiJson;
use crate::state::AppState;

async fn load(state: &AppState, id: postit_core::UserId) -> Result<postit_data::users::UserRecord, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    UsersRepo::find_by_id(&mut conn, id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))
}

#[utoipa::path(get, path = "/api/v1/me", tag = "me", security(("oidc" = [])),
    responses((status = 200, body = MeDto), (status = 401), (status = 403)))]
pub async fn get_me(State(state): State<AppState>, Auth(principal): Auth) -> Result<Json<MeDto>, ApiError> {
    let user = load(&state, principal.user_id).await?;
    Ok(Json(MeDto {
        user: UserDto::from(&user),
        account_url: state.settings.account_url.as_ref().map(ToString::to_string),
    }))
}

#[utoipa::path(delete, path = "/api/v1/me", tag = "me", security(("oidc" = [])),
    request_body = DeleteMeRequest,
    responses((status = 202, description = "Deletion started"), (status = 409), (status = 422)))]
pub async fn delete_me(
    State(state): State<AppState>,
    ActiveUser(principal): ActiveUser,
    ApiJson(body): ApiJson<DeleteMeRequest>,
) -> Result<StatusCode, ApiError> {
    let user = load(&state, principal.user_id).await?;
    if body.display_name != user.display_name {
        return Err(ApiError::new(ErrorCode::ValidationFailed)
            .with_detail("display_name does not match your current display name"));
    }
    state.admin.delete_self(principal.user_id).await?;
    state.auth.invalidate_user(principal.user_id);
    Ok(StatusCode::ACCEPTED)
}
```

The extractor order matters: `ActiveUser` must run before `ApiJson` so an unauthenticated request with a bad body gets 401, not 422.

Create `src/json.rs` — a `Json` extractor whose rejection is a `validation_failed` problem naming the problem kind, never echoing input:

```rust
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;

use crate::error::{ApiError, ErrorCode};

/// `axum::Json` with problem+json rejections. The detail says what was wrong ("expected
/// JSON", "missing field `role`") but never repeats request content.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(JsonRejection::BytesRejection(_)) => Err(ApiError::new(ErrorCode::PayloadTooLarge)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail("expected Content-Type: application/json")),
            Err(JsonRejection::JsonDataError(err)) => Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail(safe_serde_detail(&err.body_text()))),
            Err(_) => Err(ApiError::new(ErrorCode::ValidationFailed).with_detail("malformed JSON body")),
        }
    }
}

/// serde messages look like "missing field `role` at line 1 column 2" or "unknown variant
/// `x`, expected one of …". Keep only the part before " at line" and drop any quoted value
/// that is not a field name, so user input never reaches the response.
fn safe_serde_detail(text: &str) -> String {
    let head = text.split(" at line").next().unwrap_or(text);
    if head.contains("unknown variant") || head.contains("invalid type") || head.contains("invalid value") {
        return "a field has an invalid value".to_string();
    }
    head.rsplit(": ").next().unwrap_or(head).to_string()
}
```

Add `pub mod json;` to `lib.rs`.

`routes/mod.rs` — fill `v1_router`:

```rust
pub mod auth;
pub mod me;
pub mod probes;
#[cfg(any(test, feature = "testkit"))]
pub mod testing;

use axum::Router;
use axum::routing::get;

use crate::state::AppState;

pub fn v1_router() -> Router<AppState> {
    let router = Router::new()
        .route("/auth/config", get(auth::config))
        .route("/me", get(me::get_me).delete(me::delete_me));
    #[cfg(any(test, feature = "testkit"))]
    let router = router.nest("/_test", testing::router());
    router
}
```

Add `pub mod dto; pub mod extract;` to `lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p postit-api`
Expected: PASS.

- [ ] **Step 6: Quality gates and commit**

```bash
git add server/crates/api
git commit -m "postit-api: Auth/ActiveUser/RequireAdmin/Scope extractors, /auth/config, /me"
```

---

### Task 10: `postit-api` — user administration routes

**Files:**
- Create: `server/crates/api/src/routes/users.rs`, `server/crates/api/src/pagination.rs`
- Modify: `server/crates/api/src/routes/mod.rs`, `server/crates/api/src/dto.rs`, `server/crates/api/src/lib.rs`
- Test: `server/crates/api/tests/users.rs`

**Interfaces:**
- Consumes: `RequireAdmin`, `ApiJson`, `UserDto`, `UserAdminService`, `UsersRepo::{list, find_by_id}`, strict `FromStr` for `UserStatus` (Task 2).
- Produces (`pagination.rs`):

```rust
#[derive(Deserialize, IntoParams)] pub struct PageQuery { pub page: Option<u64>, pub page_size: Option<u64> }
impl PageQuery { pub fn to_pagination(&self) -> Result<emixdb::dto::Pagination, ApiError>; }  // defaults 1/20, page >= 1, 1 <= page_size <= 100
#[derive(Serialize, ToSchema)] pub struct Page<T: ToSchema> { pub data: Vec<T>, pub total: u64, pub page: u64, pub page_size: u64 }
```

- Produces (`dto.rs`): `#[derive(Deserialize, ToSchema)] #[serde(deny_unknown_fields)] pub struct PatchUserRequest { pub status: Option<StatusDto>, pub role: Option<RoleDto> }`.

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/users.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;

async fn signed_up(app: &TestApp, sub: &str) -> String {
    let res = app.call(Method::GET, "/api/v1/me", Some(&app.token(sub)), None).await;
    assert_eq!(res.status, StatusCode::OK);
    res.body["id"].as_str().unwrap_or_default().to_string()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn members_cannot_use_admin_routes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob_id = signed_up(&app, "bob").await;
    app.call(Method::PATCH, &format!("/api/v1/users/{bob_id}"), Some(&admin), Some(json!({ "status": "active" }))).await;

    let res = app.call(Method::GET, "/api/v1/users", Some(&app.token("bob")), None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "forbidden");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_searches_and_paginates(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    for sub in ["carol", "dave", "erin"] {
        signed_up(&app, sub).await;
    }

    let pending = app.call(Method::GET, "/api/v1/users?status=pending&page_size=2", Some(&admin), None).await;
    assert_eq!(pending.status, StatusCode::OK);
    assert_eq!(pending.body["total"], 3);
    assert_eq!(pending.body["data"].as_array().map(Vec::len), Some(2));
    assert_eq!(pending.body["page"], 1);
    assert_eq!(pending.body["page_size"], 2);

    let search = app.call(Method::GET, "/api/v1/users?search=dav", Some(&admin), None).await;
    assert_eq!(search.body["total"], 1);

    for bad in ["/api/v1/users?status=banned", "/api/v1/users?page=0", "/api/v1/users?page_size=101"] {
        let res = app.call(Method::GET, bad, Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(res.body["code"], "validation_failed");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_user_returns_404_for_unknown_ids(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let res = app.call(Method::GET, &format!("/api/v1/users/{}", uuid::Uuid::now_v7()), Some(&admin), None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    let bad = app.call(Method::GET, "/api/v1/users/not-a-uuid", Some(&admin), None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_requires_exactly_one_field_and_changes_nothing_otherwise(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");

    for body in [json!({}), json!({ "status": "active", "role": "admin" }), json!({ "status": "active", "extra": 1 })] {
        let res = app.call(Method::PATCH, &path, Some(&admin), Some(body.clone())).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let after = app.call(Method::GET, &path, Some(&admin), None).await;
    assert_eq!(after.body["status"], "pending");
    assert_eq!(after.body["role"], "member");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_rejects_disallowed_transitions(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");
    for status in ["pending", "deleting", "disabled"] {
        let res = app.call(Method::PATCH, &path, Some(&admin), Some(json!({ "status": status }))).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "pending -> {status}");
    }
    let role_on_pending = app.call(Method::PATCH, &path, Some(&admin), Some(json!({ "role": "admin" }))).await;
    assert_eq!(role_on_pending.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_admin_cannot_delete_themselves_through_users(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let me = app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    let id = me.body["id"].as_str().unwrap_or_default();
    let res = app.call(Method::DELETE, &format!("/api/v1/users/{id}"), Some(&admin), None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "forbidden");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-api --test users`
Expected: FAIL (404s).

- [ ] **Step 3: Implement `pagination.rs`**

```rust
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ErrorCode};

pub const DEFAULT_PAGE_SIZE: u64 = 20;
pub const MAX_PAGE_SIZE: u64 = 100;

#[derive(Debug, Deserialize, IntoParams)]
pub struct PageQuery {
    /// 1-based page number (default 1).
    pub page: Option<u64>,
    /// Items per page, 1–100 (default 20).
    pub page_size: Option<u64>,
}

impl PageQuery {
    /// # Errors
    ///
    /// `validation_failed` when `page` is 0 or `page_size` is outside 1–100.
    pub fn to_pagination(&self) -> Result<emixdb::dto::Pagination, ApiError> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
        if page == 0 {
            return Err(ApiError::new(ErrorCode::ValidationFailed).with_detail("page must be at least 1"));
        }
        if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail(format!("page_size must be between 1 and {MAX_PAGE_SIZE}")));
        }
        Ok(emixdb::dto::Pagination { page, page_size })
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Page<T: ToSchema> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

impl<T: ToSchema> Page<T> {
    #[must_use]
    pub fn new(data: Vec<T>, total: u64, pagination: &emixdb::dto::Pagination) -> Self {
        Self { data, total, page: pagination.page, page_size: pagination.page_size }
    }
}
```

`Query<…>` rejections (bad numbers) must also become `validation_failed`: add to `json.rs` an `ApiQuery<T>` wrapper built the same way over `axum::extract::Query` (rejection → `ValidationFailed` with detail `"invalid query string"`), and an `ApiPath<T>` over `axum::extract::Path` (rejection → `ValidationFailed` with detail `"invalid path parameter"`).

- [ ] **Step 4: Implement `routes/users.rs`**

```rust
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use postit_core::UserId;
use postit_data::users::{UserStatus, UsersRepo};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto::{PatchUserRequest, StatusDto, UserDto};
use crate::error::{ApiError, ErrorCode};
use crate::extract::RequireAdmin;
use crate::json::{ApiJson, ApiPath, ApiQuery};
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct UserListQuery {
    /// `pending`, `active`, `disabled`, or `deleting`.
    pub status: Option<String>,
    /// Case-insensitive substring of display name or email.
    pub search: Option<String>,
    #[serde(flatten)]
    #[param(inline)]
    pub page: PageQuery,
}

#[utoipa::path(get, path = "/api/v1/users", tag = "users", security(("oidc" = [])),
    params(UserListQuery), responses((status = 200, body = Page<UserDto>), (status = 403), (status = 422)))]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<UserListQuery>,
) -> Result<Json<Page<UserDto>>, ApiError> {
    let pagination = q.page.to_pagination()?;
    let status = q
        .status
        .as_deref()
        .map(str::parse::<UserStatus>)
        .transpose()
        .map_err(|_| ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown status"))?;
    let search = q.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut conn = state.pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    let result = UsersRepo::list(&mut conn, status, search, pagination.clone()).await?;
    let data = result.data.iter().map(UserDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}

#[utoipa::path(get, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), responses((status = 200, body = UserDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<UserDto>, ApiError> {
    let mut conn = state.pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    let user = UsersRepo::find_by_id(&mut conn, UserId::from(id))
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
    Ok(Json(UserDto::from(&user)))
}

#[utoipa::path(patch, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)), request_body = PatchUserRequest,
    responses((status = 200, body = UserDto), (status = 404), (status = 409), (status = 422)))]
pub async fn patch(
    State(state): State<AppState>,
    RequireAdmin(actor): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(body): ApiJson<PatchUserRequest>,
) -> Result<Json<UserDto>, ApiError> {
    let target = UserId::from(id);
    let record = match (body.status, body.role) {
        (Some(status), None) => {
            let mut conn = state.pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
            let current = UsersRepo::find_by_id(&mut conn, target)
                .await?
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
            drop(conn);
            match (current.status, status) {
                (UserStatus::Pending, StatusDto::Active) => state.admin.approve(actor.user_id, target).await?,
                (UserStatus::Disabled, StatusDto::Active) => state.admin.enable(actor.user_id, target).await?,
                (_, StatusDto::Disabled) => state.admin.disable(actor.user_id, target).await?,
                (UserStatus::Deleting, _) => return Err(ApiError::new(ErrorCode::UserDeleting)),
                (from, _) => {
                    return Err(ApiError::new(ErrorCode::ValidationFailed).with_detail(format!(
                        "status transition not allowed: {} -> {}",
                        from.as_str(),
                        UserStatus::from(status).as_str()
                    )));
                }
            }
        }
        (None, Some(role)) => state.admin.change_role(actor.user_id, target, role.into()).await?,
        _ => {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail("send exactly one of `status` or `role`"));
        }
    };
    // This process sees the change on the next request; pg_notify covers the others.
    state.auth.invalidate_user(target);
    Ok(Json(UserDto::from(&record)))
}

#[utoipa::path(delete, path = "/api/v1/users/{id}", tag = "users", security(("oidc" = [])),
    params(("id" = Uuid, Path)),
    responses((status = 202, description = "Deletion started"), (status = 403), (status = 404), (status = 409)))]
pub async fn delete(
    State(state): State<AppState>,
    RequireAdmin(actor): RequireAdmin,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.admin.delete_user(actor.user_id, UserId::from(id)).await?;
    state.auth.invalidate_user(UserId::from(id));
    Ok(StatusCode::ACCEPTED)
}
```

`disable` on a `pending` user goes to the service, which returns `InvalidTransition` → 422; on a `deleting` user the service returns `UserDeleting` → 409, so the service stays the one owner of transition rules. The pre-read only picks which service call applies. `serde(flatten)` with `IntoParams` on `PageQuery` requires `#[param(inline)]`; if utoipa 6 rejects flattening there, list `page` and `page_size` fields directly on `UserListQuery` and build a `PageQuery` from them.

Add `PatchUserRequest` to `dto.rs`:

```rust
/// Exactly one of `status` or `role`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchUserRequest {
    pub status: Option<StatusDto>,
    pub role: Option<RoleDto>,
}
```

Register in `v1_router`:

```rust
        .route("/users", get(users::list))
        .route("/users/{id}", get(users::get).patch(users::patch).delete(users::delete))
```

(`use axum::routing::get;` covers `.patch`/`.delete` chained on the `MethodRouter`.) Add `pub mod users;` to `routes/mod.rs` and `pub mod pagination;` to `lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p postit-api`
Expected: PASS.

- [ ] **Step 6: Quality gates and commit**

```bash
git add server/crates/api
git commit -m "postit-api: user administration routes with pagination and strict PATCH"
```

---

### Task 11: `postit-api` — `GET /admin/audit`

**Files:**
- Create: `server/crates/api/src/routes/audit.rs`
- Modify: `server/crates/api/src/routes/mod.rs`, `server/crates/api/src/dto.rs`
- Test: `server/crates/api/tests/audit.rs`

**Interfaces:**
- Consumes: `AuditRepo::list(conn, &AuditFilter, Pagination)`, `AuditEventKind: FromStr`, `Page`, `PageQuery`, `ApiQuery`.
- Produces (`dto.rs`): `#[derive(Serialize, ToSchema)] pub struct UserRef { pub id: Uuid, pub deleted: bool }`, `#[derive(Serialize, ToSchema)] pub struct AuditEventDto { pub id: Uuid, pub at: DateTime<Utc>, pub kind: String, pub actor: Option<UserRef>, pub owner: Option<UserRef>, pub subject: Option<UserRef>, pub ip: Option<String>, pub request_id: Option<Uuid>, #[schema(value_type = Object)] pub details: serde_json::Value }`, `impl From<AuditEventRow> for AuditEventDto`.

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/audit.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn audit_lists_filters_and_marks_pseudonyms(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let me = app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    let admin_id = me.body["id"].as_str().unwrap_or_default().to_string();

    // A row referencing a pseudonymized (UUID v8) user, as delete_user leaves behind. Written
    // with raw SQL because AuditLog::record refuses references to users that do not exist.
    let pseudo = postit_data::pseudonym::pseudonym_for(b"k", UserId::from(uuid::Uuid::now_v7()));
    sqlx::query(
        "INSERT INTO audit_events (id, actor_user_id, subject_user_id, kind, details)
         VALUES ($1, $2, $3, 'user_deleted', jsonb_build_object('other_user_id', $2::text))",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(pseudo)
    .bind(uuid::Uuid::parse_str(&admin_id).unwrap_or_else(|e| unreachable!("{e}")))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let all = app.call(Method::GET, "/api/v1/admin/audit", Some(&admin), None).await;
    assert_eq!(all.status, StatusCode::OK);
    let kinds: Vec<&str> = all.body["data"].as_array().into_iter().flatten().filter_map(|e| e["kind"].as_str()).collect();
    assert!(kinds.contains(&"user_provisioned"));
    assert!(kinds.contains(&"bootstrap_admin_granted"));

    let deleted = app.call(Method::GET, "/api/v1/admin/audit?kind=user_deleted", Some(&admin), None).await;
    assert_eq!(deleted.body["total"], 1);
    let event = &deleted.body["data"][0];
    assert_eq!(event["actor"]["deleted"], true);
    assert_eq!(event["subject"]["deleted"], false);
    assert_eq!(event["details"]["other_user_id"]["deleted"], true);

    let by_subject = app
        .call(Method::GET, &format!("/api/v1/admin/audit?subject_user_id={admin_id}"), Some(&admin), None)
        .await;
    assert!(by_subject.body["total"].as_u64().is_some_and(|t| t >= 2));

    let future = app.call(Method::GET, "/api/v1/admin/audit?from=2999-01-01T00:00:00Z", Some(&admin), None).await;
    assert_eq!(future.body["total"], 0);

    let bad = app.call(Method::GET, "/api/v1/admin/audit?kind=nope", Some(&admin), None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn audit_is_admin_only(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None).await;
    let res = app.call(Method::GET, "/api/v1/admin/audit", Some(&app.token("bob")), None).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
}
```

Remove the unused `AuditEvent`/`AuditLog`/`AuditEventId`/`AuditEventKind` imports if clippy flags them.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-api --test audit`
Expected: FAIL.

- [ ] **Step 3: Implement**

`dto.rs`:

```rust
/// A user reference in the audit log. `deleted` is true for a pseudonym left by user
/// deletion (a UUID v8, which postit never generates otherwise).
#[derive(Debug, Serialize, ToSchema)]
pub struct UserRef {
    pub id: Uuid,
    pub deleted: bool,
}

impl From<Uuid> for UserRef {
    fn from(id: Uuid) -> Self {
        Self { id, deleted: id.get_version_num() == 8 }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEventDto {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub actor: Option<UserRef>,
    pub owner: Option<UserRef>,
    pub subject: Option<UserRef>,
    pub ip: Option<String>,
    pub request_id: Option<Uuid>,
    /// Event details. Every `*_user_id` value is a `UserRef` object.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

impl From<postit_data::audit_repo::AuditEventRow> for AuditEventDto {
    fn from(row: postit_data::audit_repo::AuditEventRow) -> Self {
        let mut details = row.details;
        if let Some(map) = details.as_object_mut() {
            for (key, value) in map.iter_mut() {
                if key.ends_with("_user_id")
                    && let Some(id) = value.as_str().and_then(|s| Uuid::parse_str(s).ok())
                {
                    *value = serde_json::to_value(UserRef::from(id)).unwrap_or(serde_json::Value::Null);
                }
            }
        }
        Self {
            id: row.id.as_uuid(),
            at: row.at,
            kind: row.kind,
            actor: row.actor_user_id.map(|u| UserRef::from(u.as_uuid())),
            owner: row.owner_id.map(|u| UserRef::from(u.as_uuid())),
            subject: row.subject_user_id.map(|u| UserRef::from(u.as_uuid())),
            ip: row.ip,
            request_id: row.request_id,
            details,
        }
    }
}
```

(Check `AuditEventRow`'s field types with `rg "pub struct AuditEventRow" -A12 server/crates/data/src/audit_repo.rs`; `owner_id` is `Option<UserId>`.)

`routes/audit.rs`:

```rust
use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use postit_core::UserId;
use postit_data::audit::AuditEventKind;
use postit_data::audit_repo::{AuditFilter, AuditRepo};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::dto::AuditEventDto;
use crate::error::{ApiError, ErrorCode};
use crate::extract::RequireAdmin;
use crate::json::ApiQuery;
use crate::pagination::{Page, PageQuery};
use crate::state::AppState;

#[derive(Debug, Deserialize, IntoParams)]
pub struct AuditQuery {
    /// An audit event kind, e.g. `user_approved`.
    pub kind: Option<String>,
    /// Inclusive lower bound (RFC 3339).
    pub from: Option<DateTime<Utc>>,
    /// Inclusive upper bound (RFC 3339).
    pub to: Option<DateTime<Utc>>,
    pub actor_user_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[utoipa::path(get, path = "/api/v1/admin/audit", tag = "admin", security(("oidc" = [])),
    params(AuditQuery), responses((status = 200, body = Page<AuditEventDto>), (status = 403), (status = 422)))]
pub async fn list(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    ApiQuery(q): ApiQuery<AuditQuery>,
) -> Result<Json<Page<AuditEventDto>>, ApiError> {
    let pagination = PageQuery { page: q.page, page_size: q.page_size }.to_pagination()?;
    let kind = q
        .kind
        .as_deref()
        .map(str::parse::<AuditEventKind>)
        .transpose()
        .map_err(|_| ApiError::new(ErrorCode::ValidationFailed).with_detail("unknown audit event kind"))?;
    let filter = AuditFilter {
        kind: kind.map(|k| k.as_str().to_string()),
        from: q.from,
        to: q.to,
        actor_user_id: q.actor_user_id.map(UserId::from),
        subject_user_id: q.subject_user_id.map(UserId::from),
    };
    let mut conn = state.pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    let result = AuditRepo::list(&mut conn, &filter, pagination.clone()).await?;
    let data = result.data.into_iter().map(AuditEventDto::from).collect();
    Ok(Json(Page::new(data, result.total, &pagination)))
}
```

Register `.route("/admin/audit", get(audit::list))` in `v1_router` and `pub mod audit;` in `routes/mod.rs`.

- [ ] **Step 4: Run tests, gates, commit**

Run: `cargo test -p postit-api` → PASS, then gates.

```bash
git add server/crates/api
git commit -m "postit-api: GET /admin/audit with filters and deleted-user markers"
```

---

### Task 12: `postit-api` — idempotency helper and `If-Match`

**Files:**
- Create: `server/crates/api/src/idempotency.rs`, `server/crates/api/src/preconditions.rs`
- Modify: `server/crates/api/src/routes/testing.rs`, `server/crates/api/src/lib.rs`
- Test: `server/crates/api/tests/idempotency.rs`

**Interfaces:**
- Consumes: `IdempotencyRepo::{begin, complete, delete}`, `BeginOutcome`, `IdempotencyState`, `OwnerScope`.
- Produces:

```rust
// idempotency.rs
pub struct IdempotencyKey(pub Option<String>);   // FromRequestParts<S>, Rejection = ApiError
pub struct StoredResponse { pub status: StatusCode, pub body: serde_json::Value }
pub fn request_hash(method: &Method, route: &str, body: &[u8]) -> String;   // hex SHA-256 via emixcrypto
pub async fn run<F, Fut>(pool: &PgPool, scope: &OwnerScope, key: Option<&str>, route: &str, hash: &str, f: F)
    -> Result<StoredResponse, ApiError>
    where F: FnOnce() -> Fut, Fut: Future<Output = Result<StoredResponse, ApiError>>;
impl IntoResponse for StoredResponse
pub const KEY_TTL: chrono::Duration = 24 hours

// preconditions.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ETag(pub i64);   // renders "\"v{n}\""
impl ETag { pub fn header_value(self) -> HeaderValue; }
pub fn check_if_match(headers: &HeaderMap, current: ETag) -> Result<(), ApiError>;
```

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/idempotency.rs`:

```rust
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use sqlx::PgPool;

fn post(token: &str, key: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::post("/api/v1/_test/idempotent")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        b = b.header("idempotency-key", key);
    }
    b.body(Body::from(body.to_string())).unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_replayed_key_returns_the_stored_response(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let first = app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    assert_eq!(first.status, StatusCode::CREATED);
    let replay = app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(replay.body, first.body, "same run id means the handler did not run again");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_key_reused_with_a_different_body_is_rejected(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    let reused = app.send(post(&token, Some("k1"), r#"{"n":2}"#)).await;
    assert_eq!(reused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reused.body["code"], "idempotency_key_reused");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_requests_with_one_key_run_once(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&token), None).await; // provision first
    // The test route sleeps 300 ms inside the idempotent section when body has "slow".
    let (a, b) = tokio::join!(
        app.send(post(&token, Some("k2"), r#"{"slow":true}"#)),
        app.send(post(&token, Some("k2"), r#"{"slow":true}"#)),
    );
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    let conflict = if a.status == StatusCode::CONFLICT { a } else { b };
    assert_eq!(conflict.body["code"], "idempotency_in_progress");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_same_key_for_two_users_runs_twice(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = app.token("bob");
    let me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    let bob_id = me.body["id"].as_str().unwrap_or_default().to_string();
    app.call(Method::PATCH, &format!("/api/v1/users/{bob_id}"), Some(&admin), Some(serde_json::json!({"status":"active"}))).await;

    let a = app.send(post(&admin, Some("same"), r#"{"n":1}"#)).await;
    let b = app.send(post(&bob, Some("same"), r#"{"n":1}"#)).await;
    assert_eq!(a.status, StatusCode::CREATED);
    assert_eq!(b.status, StatusCode::CREATED);
    assert_ne!(a.body["run"], b.body["run"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failed_run_frees_the_key(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let failed = app.send(post(&token, Some("k3"), r#"{"fail":true}"#)).await;
    assert_eq!(failed.status, StatusCode::UNPROCESSABLE_ENTITY);
    let retry = app.send(post(&token, Some("k3"), r#"{"fail":true}"#)).await;
    assert_eq!(retry.body["code"], "validation_failed", "the key was released, not replayed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn if_match_is_required_and_checked(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let put = |if_match: Option<&str>| {
        let mut b = Request::put("/api/v1/_test/versioned").header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(v) = if_match {
            b = b.header(header::IF_MATCH, v);
        }
        b.body(Body::empty()).unwrap_or_else(|e| unreachable!("{e}"))
    };
    let missing = app.send(put(None)).await;
    assert_eq!(missing.status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(missing.body["code"], "precondition_required");
    let stale = app.send(put(Some("\"v6\""))).await;
    assert_eq!(stale.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(stale.body["code"], "version_conflict");
    let ok = app.send(put(Some("\"v7\""))).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.headers.get(header::ETAG).and_then(|v| v.to_str().ok()), Some("\"v8\""));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-api --test idempotency`
Expected: FAIL (404).

- [ ] **Step 3: Implement `preconditions.rs`**

```rust
use axum::http::{HeaderMap, HeaderValue, header};

use crate::error::{ApiError, ErrorCode};

/// A strong ETag over a resource version: `"v{n}"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ETag(pub i64);

impl ETag {
    #[must_use]
    pub fn header_value(self) -> HeaderValue {
        HeaderValue::from_str(&format!("\"v{}\"", self.0)).unwrap_or_else(|_| HeaderValue::from_static("\"v0\""))
    }
}

/// # Errors
///
/// `precondition_required` (428) without `If-Match`; `version_conflict` (412) when it names
/// another version.
pub fn check_if_match(headers: &HeaderMap, current: ETag) -> Result<(), ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(ErrorCode::PreconditionRequired));
    };
    let expected = current.header_value();
    let matches = value
        .to_str()
        .ok()
        .is_some_and(|v| v.split(',').map(str::trim).any(|tag| tag == expected.to_str().unwrap_or_default()));
    if matches { Ok(()) } else { Err(ApiError::new(ErrorCode::VersionConflict)) }
}
```

- [ ] **Step 4: Implement `idempotency.rs`**

```rust
use std::future::Future;

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use emixcrypto::HashAlgorithm as _;
use postit_data::OwnerScope;
use postit_data::idempotency::{BeginOutcome, IdempotencyRepo, IdempotencyState};
use sqlx::PgPool;

use crate::error::{ApiError, ErrorCode};

pub const KEY_TTL: chrono::Duration = chrono::Duration::hours(24);

pub struct IdempotencyKey(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let Some(value) = parts.headers.get("idempotency-key") else {
            return Ok(Self(None));
        };
        let key = value.to_str().unwrap_or_default();
        let valid = (1..=255).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic());
        if !valid {
            return Err(ApiError::new(ErrorCode::ValidationFailed)
                .with_detail("Idempotency-Key must be 1-255 visible ASCII characters"));
        }
        Ok(Self(Some(key.to_owned())))
    }
}

/// A JSON response that can be stored and replayed.
#[derive(Debug, Clone)]
pub struct StoredResponse {
    pub status: StatusCode,
    pub body: serde_json::Value,
}

impl IntoResponse for StoredResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Hex SHA-256 over the method, the matched route, and the raw body.
#[must_use]
pub fn request_hash(method: &Method, route: &str, body: &[u8]) -> String {
    let mut input = Vec::with_capacity(method.as_str().len() + route.len() + body.len() + 2);
    input.extend_from_slice(method.as_str().as_bytes());
    input.push(b'\n');
    input.extend_from_slice(route.as_bytes());
    input.push(b'\n');
    input.extend_from_slice(body);
    emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(&input)
        .unwrap_or_default()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Runs `f` at most once per `(owner, actor, key)`: plan 01's `Idempotency-Key` contract.
///
/// # Errors
///
/// `idempotency_key_reused` for the same key with another route or body,
/// `idempotency_in_progress` while the first request runs, or `f`'s own error (which frees
/// the key so the client can retry).
pub async fn run<F, Fut>(
    pool: &PgPool,
    scope: &OwnerScope,
    key: Option<&str>,
    route: &str,
    hash: &str,
    f: F,
) -> Result<StoredResponse, ApiError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<StoredResponse, ApiError>>,
{
    let Some(key) = key else {
        return f().await;
    };
    let mut conn = pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    let outcome = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        scope.owner,
        scope.actor,
        key,
        route,
        hash,
        chrono::Utc::now() + KEY_TTL,
    )
    .await?;
    drop(conn);

    let record = match outcome {
        BeginOutcome::Conflict(existing) => {
            if existing.route != route || existing.request_hash != hash {
                return Err(ApiError::new(ErrorCode::IdempotencyKeyReused));
            }
            return match (existing.state, existing.response_status, existing.response_body) {
                (IdempotencyState::Completed, Some(status), Some(body)) => Ok(StoredResponse {
                    status: u16::try_from(status)
                        .ok()
                        .and_then(|s| StatusCode::from_u16(s).ok())
                        .unwrap_or(StatusCode::OK),
                    body,
                }),
                _ => Err(ApiError::new(ErrorCode::IdempotencyInProgress)),
            };
        }
        BeginOutcome::Started(record) => record,
    };

    let result = f().await;
    let mut conn = pool.acquire().await.map_err(|e| ApiError::internal(&e))?;
    match &result {
        Ok(response) => {
            IdempotencyRepo::complete(
                &mut conn,
                record.id,
                i16::try_from(response.status.as_u16()).unwrap_or(200),
                response.body.clone(),
            )
            .await?;
        }
        Err(_) => IdempotencyRepo::delete(&mut conn, record.id).await?,
    }
    result
}
```

Field names on `OwnerScope` (`owner`, `actor`) and `IdempotencyRecord` come from Task 2's crate; adjust if they differ.

- [ ] **Step 5: Add the test routes**

Append to `routes/testing.rs`:

```rust
use axum::body::Bytes;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::put;

use crate::error::{ApiError, ErrorCode};
use crate::extract::Scope;
use crate::idempotency::{IdempotencyKey, StoredResponse, request_hash, run};
use crate::preconditions::{ETag, check_if_match};

async fn idempotent(
    axum::extract::State(state): axum::extract::State<AppState>,
    Scope(scope): Scope,
    IdempotencyKey(key): IdempotencyKey,
    body: Bytes,
) -> Result<StoredResponse, ApiError> {
    let route = "/api/v1/_test/idempotent";
    let hash = request_hash(&Method::POST, route, &body);
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    run(&state.pool, &scope, key.as_deref(), route, &hash, || async move {
        if parsed["slow"].as_bool() == Some(true) {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        if parsed["fail"].as_bool() == Some(true) {
            return Err(ApiError::new(ErrorCode::ValidationFailed));
        }
        Ok(StoredResponse {
            status: StatusCode::CREATED,
            body: serde_json::json!({ "run": uuid::Uuid::now_v7() }),
        })
    })
    .await
}

async fn versioned(Scope(_): Scope, headers: HeaderMap) -> Result<impl IntoResponse, ApiError> {
    check_if_match(&headers, ETag(7))?;
    Ok(([(header::ETAG, ETag(8).header_value())], "updated"))
}
```

and extend `router()` with `.route("/idempotent", post(idempotent)).route("/versioned", put(versioned))`.

Add `pub mod idempotency; pub mod preconditions;` to `lib.rs`.

- [ ] **Step 6: Run tests, gates, commit**

Run: `cargo test -p postit-api` → PASS, then gates.

```bash
git add server/crates/api
git commit -m "postit-api: Idempotency-Key helper and If-Match preconditions, exercised through test routes"
```

---

### Task 13: OpenAPI, Swagger UI, `cargo xtask openapi`

**Files:**
- Modify: `server/crates/api/src/openapi.rs` (replace the stub)
- Modify: `server/xtask/Cargo.toml`, `server/xtask/src/main.rs`
- Create: `server/xtask/src/openapi.rs`, `api/openapi.json` (generated)
- Test: `server/crates/api/tests/openapi.rs`

**Interfaces:**
- Produces: `pub fn openapi() -> utoipa::openapi::OpenApi`, `pub fn openapi_json_pretty() -> String` (pretty JSON + trailing newline, the committed form), `pub fn docs_router(state: &AppState) -> Router<AppState>` (serves `/api/openapi.json` always and `/docs` outside production).
- Produces: `cargo xtask openapi [--check]`.

- [ ] **Step 1: Write the failing tests**

`server/crates/api/tests/openapi.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::TestApp;
use postit_config::Environment;
use sqlx::PgPool;

#[test]
fn spec_lists_every_route_and_the_oauth_scheme() {
    let spec = serde_json::to_value(postit_api::openapi::openapi()).unwrap_or_default();
    for path in ["/api/v1/auth/config", "/api/v1/me", "/api/v1/users", "/api/v1/users/{id}", "/api/v1/admin/audit", "/health", "/ready"] {
        assert!(spec["paths"].get(path).is_some(), "missing {path}");
    }
    assert!(spec["paths"].get("/api/v1/_test/echo").is_none(), "test routes stay out of the contract");
    let flow = &spec["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
    assert_eq!(flow["authorizationUrl"], "https://idp.invalid/authorize");
    assert_eq!(flow["tokenUrl"], "https://idp.invalid/token");
}

#[test]
fn committed_spec_is_current() {
    let committed = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../api/openapi.json"))
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert_eq!(committed, postit_api::openapi::openapi_json_pretty(), "run `cargo xtask openapi`");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn served_spec_carries_the_discovery_urls(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.state.auth.prefetch().await.unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    let res = app.call(Method::GET, "/api/openapi.json", None, None).await;
    assert_eq!(res.status, StatusCode::OK);
    let flow = &res.body["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
    assert!(flow["authorizationUrl"].as_str().is_some_and(|u| u.ends_with("/authorize") && !u.contains("idp.invalid")));
    assert!(flow["tokenUrl"].as_str().is_some_and(|u| u.ends_with("/token") && !u.contains("idp.invalid")));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn docs_are_served_outside_production_only(pool: PgPool) {
    let dev = TestApp::start(pool.clone()).await;
    let res = dev.call(Method::GET, "/docs/", None, None).await;
    assert_eq!(res.status, StatusCode::OK);

    let prod = TestApp::start_with(pool, |s| s.environment = Environment::Production).await;
    assert_eq!(prod.call(Method::GET, "/docs/", None, None).await.status, StatusCode::NOT_FOUND);
    assert_eq!(prod.call(Method::GET, "/api/openapi.json", None, None).await.status, StatusCode::OK);
}
```

`committed_spec_is_current` fails until Step 4 generates the file; that is expected. Swagger UI may redirect `/docs` to `/docs/`; request `/docs/` as above.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p postit-api --test openapi`
Expected: FAIL.

- [ ] **Step 3: Implement `openapi.rs`**

```rust
//! The generated API contract. `openapi()` is the single source for the served spec and
//! `cargo xtask openapi` (which writes `api/openapi.json`).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use postit_config::Environment;
use utoipa::openapi::security::{AuthorizationCode, Flow, OAuth2, Scopes, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::state::AppState;

pub const PLACEHOLDER_AUTHORIZE: &str = "https://idp.invalid/authorize";
pub const PLACEHOLDER_TOKEN: &str = "https://idp.invalid/token";

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "oidc",
            SecurityScheme::OAuth2(OAuth2::new([Flow::AuthorizationCode(AuthorizationCode::new(
                PLACEHOLDER_AUTHORIZE,
                PLACEHOLDER_TOKEN,
                Scopes::from_iter([
                    ("openid", "OpenID Connect sign-in"),
                    ("profile", "Name and username"),
                    ("email", "Email address"),
                    ("offline_access", "Refresh tokens"),
                ]),
            ))])),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(title = "postit API", description = "Self-hosted social publishing for a small team."),
    paths(
        crate::routes::probes::health,
        crate::routes::probes::ready,
        crate::routes::auth::config,
        crate::routes::me::get_me,
        crate::routes::me::delete_me,
        crate::routes::users::list,
        crate::routes::users::get,
        crate::routes::users::patch,
        crate::routes::users::delete,
        crate::routes::audit::list,
    ),
    components(schemas(
        crate::dto::UserDto, crate::dto::MeDto, crate::dto::AuthConfigDto, crate::dto::DeleteMeRequest,
        crate::dto::PatchUserRequest, crate::dto::RoleDto, crate::dto::StatusDto,
        crate::dto::AuditEventDto, crate::dto::UserRef,
    )),
    modifiers(&SecurityAddon),
)]
struct ApiDoc;

#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let mut spec = ApiDoc::openapi();
    spec.info.version = env!("CARGO_PKG_VERSION").to_string();
    spec
}

#[must_use]
pub fn openapi_json_pretty() -> String {
    let mut text = serde_json::to_string_pretty(&openapi()).unwrap_or_default();
    text.push('\n');
    text
}

async fn served_spec(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut spec = serde_json::to_value(openapi()).unwrap_or_default();
    if let Some(doc) = state.auth.discovery_document() {
        let flow = &mut spec["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
        if let Some(url) = doc.authorization_endpoint {
            flow["authorizationUrl"] = url.into();
        }
        if let Some(url) = doc.token_endpoint {
            flow["tokenUrl"] = url.into();
        }
    }
    Json(spec)
}

/// `/api/openapi.json` always; Swagger UI at `/docs` outside production, signing in with the
/// Flutter public client through PKCE.
pub fn docs_router(state: &AppState) -> Router<AppState> {
    let router = Router::new().route("/api/openapi.json", get(served_spec));
    if state.settings.environment == Environment::Production {
        return router;
    }
    let oauth = utoipa_swagger_ui::oauth::Config::new()
        .client_id(&state.settings.client_id)
        .scopes(state.settings.scopes.clone())
        .use_pkce_with_authorization_code_grant(true);
    let swagger = utoipa_swagger_ui::SwaggerUi::new("/docs")
        .config(utoipa_swagger_ui::Config::new(["/api/openapi.json"]))
        .oauth(oauth);
    router.merge(swagger)
}
```

If the pinned `utoipa-swagger-ui` requires `.url(path, spec)` instead of `.config(Config::new([...]))`, use `.url("/api/openapi.json", openapi())` and remove the separate `/api/openapi.json` route, then move the discovery patch into a small middleware on that path — the served-spec test pins the behavior either way. `SwaggerUi` converts into `Router<S>` via `From`; if the state type does not line up, call `Router::<AppState>::from(swagger)` or `router.merge(Router::new().merge(swagger))`.

- [ ] **Step 4: `cargo xtask openapi`**

`server/xtask/Cargo.toml`: add `postit-api.workspace = true`. `server/xtask/src/openapi.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, bail};

fn target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../api/openapi.json")
}

pub fn run(check: bool) -> anyhow::Result<()> {
    let expected = postit_api::openapi::openapi_json_pretty();
    let path = target();
    if check {
        let current = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .replace("\r\n", "\n");
        if current != expected {
            bail!("{} is stale; run `cargo xtask openapi`", path.display());
        }
        println!("{} is current", path.display());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, expected).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}
```

`main.rs`:

```rust
mod openapi;
mod zitadel_bootstrap;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("zitadel-bootstrap") => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(zitadel_bootstrap::run())
        }
        Some("openapi") => openapi::run(args.next().as_deref() == Some("--check")),
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => anyhow::bail!("usage: xtask <zitadel-bootstrap|openapi [--check]>"),
    }
}
```

Add `api/openapi.json text eol=lf` to the repository's `.gitattributes` (create it if absent) so the committed file keeps LF on Windows checkouts.

Run: `cargo xtask openapi` then `cargo xtask openapi --check`.
Expected: `wrote …/api/openapi.json`, then `… is current`.

- [ ] **Step 5: Run tests, gates, commit**

Run: `cargo test -p postit-api` → PASS, then gates.

```bash
git add server/crates/api server/xtask api/openapi.json .gitattributes server/Cargo.lock
git commit -m "postit-api: OpenAPI with OAuth2 PKCE scheme and Swagger UI; cargo xtask openapi; commit api/openapi.json"
```

---

### Task 14: `postit-api` — end-to-end exit flow and redaction

**Files:**
- Create: `server/crates/api/tests/end_to_end.rs`, `server/crates/api/tests/redaction.rs`

**Interfaces:**
- Consumes: everything in Tasks 8–12 through `TestApp`.

- [ ] **Step 1: Write the end-to-end test**

`server/crates/api/tests/end_to_end.rs`:

```rust
use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;

async fn outbox_kinds(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT job_type FROM job_outbox ORDER BY created_at")
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| unreachable!("outbox: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn plan_02_p6_exit_flow(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);

    // 1. Bootstrap admin.
    let me = app.call(Method::GET, "/api/v1/me", Some(&admin), None).await;
    assert_eq!((me.body["role"].as_str(), me.body["status"].as_str()), (Some("admin"), Some("active")));
    let admin_id = me.body["id"].as_str().unwrap_or_default().to_string();

    // 2. A second user's first request provisions them as pending.
    let bob = app.token("bob");
    let bob_me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(bob_me.body["status"], "pending");
    let bob_id = bob_me.body["id"].as_str().unwrap_or_default().to_string();
    let bob_path = format!("/api/v1/users/{bob_id}");

    // 3. 403 account_pending everywhere except GET /me.
    for (method, path, body) in [
        (Method::GET, "/api/v1/users".to_string(), None),
        (Method::GET, bob_path.clone(), None),
        (Method::PATCH, bob_path.clone(), Some(json!({ "role": "admin" }))),
        (Method::DELETE, bob_path.clone(), None),
        (Method::GET, "/api/v1/admin/audit".to_string(), None),
        (Method::DELETE, "/api/v1/me".to_string(), Some(json!({ "display_name": "bob" }))),
    ] {
        let res = app.call(method.clone(), &path, Some(&bob), body).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(res.body["code"], "account_pending", "{method} {path}");
    }

    // 4. The admin approves; the user_approved email is enqueued.
    let approved = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "status": "active" }))).await;
    assert_eq!(approved.status, StatusCode::OK);
    assert_eq!(approved.body["status"], "active");
    assert_eq!(approved.body["approved_by"], admin_id.as_str());
    assert!(outbox_kinds(&pool).await.iter().any(|k| k == "send_email"));

    // 5. The user gets access (cache evicted by the approval).
    let bob_after = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(bob_after.body["status"], "active");

    // 6. Disable, then re-enable.
    let disabled = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "status": "disabled" }))).await;
    assert_eq!(disabled.body["status"], "disabled");
    let blocked = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(blocked.body["code"], "account_disabled");
    let enabled = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "status": "active" }))).await;
    assert_eq!(enabled.body["status"], "active");
    assert_eq!(app.call(Method::GET, "/api/v1/me", Some(&bob), None).await.status, StatusCode::OK);

    // 7. Role change: bob becomes admin, then back to member.
    let promoted = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "role": "admin" }))).await;
    assert_eq!(promoted.body["role"], "admin");
    assert_eq!(app.call(Method::GET, "/api/v1/users", Some(&bob), None).await.status, StatusCode::OK);
    let demoted = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "role": "member" }))).await;
    assert_eq!(demoted.body["role"], "member");

    // 8. Last-admin guard on demote, disable, and DELETE /me.
    let admin_path = format!("/api/v1/users/{admin_id}");
    for body in [json!({ "role": "member" }), json!({ "status": "disabled" })] {
        let res = app.call(Method::PATCH, &admin_path, Some(&admin), Some(body.clone())).await;
        assert_eq!(res.status, StatusCode::CONFLICT, "{body}");
        assert_eq!(res.body["code"], "last_admin");
    }
    let self_delete = app.call(Method::DELETE, "/api/v1/me", Some(&admin), Some(json!({ "display_name": ADMIN_SUB }))).await;
    assert_eq!(self_delete.body["code"], "last_admin");

    // 9. Delete bob: 202, then bob is locked out (deleting) and status changes are refused.
    let deleted = app.call(Method::DELETE, &bob_path, Some(&admin), None).await;
    assert_eq!(deleted.status, StatusCode::ACCEPTED);
    let gone = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(gone.body["code"], "account_disabled");
    let refused = app.call(Method::PATCH, &bob_path, Some(&admin), Some(json!({ "status": "active" }))).await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.body["code"], "user_deleting");
    assert!(outbox_kinds(&pool).await.iter().any(|k| k == "delete_user"));

    // 10. The audit list shows the flow's events.
    let audit = app.call(Method::GET, "/api/v1/admin/audit?page_size=100", Some(&admin), None).await;
    let kinds: Vec<&str> = audit.body["data"].as_array().into_iter().flatten().filter_map(|e| e["kind"].as_str()).collect();
    for kind in ["user_provisioned", "bootstrap_admin_granted", "user_approved", "user_disabled", "user_enabled", "role_changed", "user_deleted"] {
        assert!(kinds.contains(&kind), "audit is missing {kind}: {kinds:?}");
    }
    let by_actor = app
        .call(Method::GET, &format!("/api/v1/admin/audit?actor_user_id={admin_id}&kind=role_changed"), Some(&admin), None)
        .await;
    assert_eq!(by_actor.body["total"], 2);
}
```

The `job_outbox` column holding the job type may not be `job_type`; check `crates/data/migrations/0006_job_outbox.sql` and use the real name. If the outbox relay is not running (no worker in this test), rows stay in `job_outbox`, which is what this asserts.

- [ ] **Step 2: Write the redaction test**

`server/crates/api/tests/redaction.rs`:

```rust
use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::http::Method;
use postit_api::testkit::{ADMIN_SUB, TestApp};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bearer_tokens_never_reach_logs_or_responses(pool: PgPool) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let ok = app.call(Method::GET, "/api/v1/me", Some(&token), None).await;
    let bad_token = format!("{token}tampered");
    let bad = app.call(Method::GET, "/api/v1/me", Some(&bad_token), None).await;

    let logs = String::from_utf8_lossy(&captured.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)).to_string();
    let signature = token.rsplit('.').next().unwrap_or_default();
    assert!(!signature.is_empty());
    assert!(!logs.contains(signature), "token signature leaked into logs");
    assert!(!ok.body.to_string().contains(signature));
    assert!(!bad.body.to_string().contains(signature));
}
```

`#[sqlx::test]` runs on a current-thread runtime, so `set_default` covers the spawned work. If some log line is emitted from another thread and missed, that only weakens the test, never produces a false failure; keep it as is.

- [ ] **Step 3: Run tests**

Run: `cargo test -p postit-api --test end_to_end --test redaction`
Expected: PASS. A failure here is a real defect in Tasks 8–12 — fix it in the owning module, not in the test.

- [ ] **Step 4: Gates and commit**

```bash
git add server/crates/api/tests
git commit -m "postit-api: end-to-end exit flow and bearer redaction tests"
```

---

### Task 15: `postit-server` — composition root, roles, TLS, shutdown, healthcheck

**Files:**
- Modify: `server/crates/server/Cargo.toml`, `server/crates/server/src/main.rs`
- Create: `server/crates/server/src/lib.rs`, `server/crates/server/src/role.rs`, `server/crates/server/src/telemetry.rs`, `server/crates/server/src/compose.rs`, `server/crates/server/src/serve.rs`, `server/crates/server/src/healthcheck.rs`
- Test: `server/crates/server/tests/smoke.rs`

**Interfaces:**
- Consumes: every earlier task.
- Produces:

```rust
// role.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Role { All, Api, Worker }
impl Role { pub fn resolve(raw: Option<&str>, env: Environment) -> anyhow::Result<Self>; pub fn runs_api(self) -> bool; pub fn runs_worker(self) -> bool; }

// lib.rs
pub struct StartOptions { pub env: Environment, pub role: Role, pub settings: Settings, pub db: Option<postit_data::Db>,
    pub api_listener: Option<std::net::TcpListener>, pub worker_listener: Option<std::net::TcpListener> }
pub struct RunningServer { pub api_addr: Option<SocketAddr>, pub worker_addr: Option<SocketAddr>, pub shutdown: CancellationToken,
    pub handle: tokio::task::JoinHandle<anyhow::Result<()>> }
pub async fn start(opts: StartOptions) -> anyhow::Result<RunningServer>;
pub async fn run_from_env() -> anyhow::Result<()>;   // main: env, role, config, telemetry, start, signals
pub fn healthcheck_from_env() -> anyhow::Result<()>;
```

- [ ] **Step 1: Dependencies**

`server/Cargo.toml` workspace deps: add `tracing-subscriber = { version = "0", features = ["env-filter", "fmt", "json"] }` (replace the existing bare `"0"` entry) and `tempfile` is already there. Internal block: `postit-server = { path = "crates/server" }` is not needed.

`server/crates/server/Cargo.toml`:

```toml
[lib]
name = "postit_server"
path = "src/lib.rs"

[[bin]]
name = "postit"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
postit-api.workspace = true
postit-config.workspace = true
postit-core.workspace = true
postit-data.workspace = true
postit-http.workspace = true
postit-identity.workspace = true
postit-jobs.workspace = true
postit-mail.workspace = true
async-trait.workspace = true
axum.workspace = true
axum-server.workspace = true
jsonwebtoken.workspace = true
rustls.workspace = true
sqlx.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
url.workspace = true

[dev-dependencies]
postit-identity = { workspace = true, features = ["testkit"] }
tempfile.workspace = true
```

- [ ] **Step 2: Write the failing smoke test**

`server/crates/server/tests/smoke.rs`:

```rust
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use postit_config::Environment;
use postit_identity::testkit::TestIssuer;
use postit_server::{Role, StartOptions, start};
use sqlx::PgPool;

fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| unreachable!("connect: {e}"));
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap_or_default();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap_or_default();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap_or_default();
    let status = response.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, response)
}

fn config_dir(issuer: &TestIssuer) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let base = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/default.toml"))
        .unwrap_or_else(|e| unreachable!("default.toml: {e}"));
    std::fs::write(dir.path().join("default.toml"), base).unwrap_or_default();
    let issuer_url = issuer.issuer_url().to_string();
    let dev = format!(
        r#"[server]
public_url = "http://127.0.0.1"
[database]
url = "postgres://localhost:5432/unused"
[auth.oidc]
issuer = "{issuer}"
audiences = ["postit"]
[auth.bootstrap]
admin_email = "admin@postit.test"
[mail]
from_address = "noreply@postit.test"
[mail.smtp]
host = "127.0.0.1"
port = 2525
tls = "none"
[app]
public_url = "http://127.0.0.1:44315"
"#,
        issuer = issuer_url.trim_end_matches('/')
    );
    std::fs::write(dir.path().join("development.toml"), dev).unwrap_or_default();
    dir
}

#[sqlx::test(migrations = "../data/migrations")]
async fn role_all_serves_health_and_ready_on_both_ports_and_shuts_down(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer);
    let settings = postit_config::load_with(
        Environment::Development,
        dir.path(),
        [
            ("POSTIT__DATABASE__USERNAME".to_string(), "unused".to_string()),
            ("POSTIT__DATABASE__PASSWORD".to_string(), "unused".to_string()),
            ("POSTIT__AUDIT__PSEUDONYM_KEY".to_string(), "smoke-key".to_string()),
        ],
    )
    .unwrap_or_else(|e| unreachable!("config: {e}"));

    let api = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("bind: {e}"));
    let worker = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("bind: {e}"));
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::All,
        settings,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(api),
        worker_listener: Some(worker),
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));

    let (Some(api_addr), Some(worker_addr)) = (running.api_addr, running.worker_addr) else {
        unreachable!("role all binds both ports");
    };
    let ready = tokio::task::spawn_blocking(move || {
        for _ in 0..100 {
            if get(api_addr, "/ready").0 == 200 && get(worker_addr, "/ready").0 == 200 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(ready, "both ports must become ready");
    let health = tokio::task::spawn_blocking(move || (get(api_addr, "/health").0, get(worker_addr, "/health").0))
        .await
        .unwrap_or_default();
    assert_eq!(health, (200, 200));

    let probe = tokio::task::spawn_blocking(move || postit_server::healthcheck::probe(api_addr))
        .await
        .unwrap_or_else(|e| unreachable!("join: {e}"));
    assert!(probe.is_ok(), "healthcheck against the API port: {probe:?}");

    running.shutdown.cancel();
    let finished = tokio::time::timeout(Duration::from_secs(40), running.handle).await;
    assert!(matches!(finished, Ok(Ok(Ok(())))), "clean shutdown, got {finished:?}");
}

#[test]
fn role_defaults_to_all_in_development_only() {
    assert_eq!(Role::resolve(None, Environment::Development).ok(), Some(Role::All));
    assert!(Role::resolve(None, Environment::Production).is_err());
    assert_eq!(Role::resolve(Some("worker"), Environment::Qa).ok(), Some(Role::Worker));
    assert!(Role::resolve(Some("both"), Environment::Development).is_err());
}
```

If `development.toml`'s other required keys are not covered by `default.toml` plus this file, the config error names them; add them to the `dev` string. `SmtpMailer::new` must not connect at construction (it only builds the transport); if it does, point `mail.smtp` at an unused local port — the smoke test never sends mail.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p postit-server`
Expected: compile errors.

- [ ] **Step 4: Implement `role.rs`, `telemetry.rs`, `healthcheck.rs`**

```rust
// role.rs
use anyhow::bail;
use postit_config::Environment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    All,
    Api,
    Worker,
}

impl Role {
    /// `POSTIT_ROLE`: `all`, `api`, or `worker`. Defaults to `all` in development only.
    ///
    /// # Errors
    ///
    /// Fails on an unknown value, or when unset outside development.
    pub fn resolve(raw: Option<&str>, env: Environment) -> anyhow::Result<Self> {
        match raw {
            Some("all") => Ok(Self::All),
            Some("api") => Ok(Self::Api),
            Some("worker") => Ok(Self::Worker),
            Some(other) => bail!("POSTIT_ROLE must be all, api, or worker (got {other})"),
            None if env == Environment::Development => Ok(Self::All),
            None => bail!("POSTIT_ROLE is required outside development (all, api, or worker)"),
        }
    }

    #[must_use]
    pub fn runs_api(self) -> bool {
        matches!(self, Self::All | Self::Api)
    }

    #[must_use]
    pub fn runs_worker(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }
}
```

```rust
// telemetry.rs
use postit_config::Environment;
use tracing_subscriber::EnvFilter;

/// Pretty `debug` in development, JSON `info` elsewhere; `RUST_LOG` overrides the level.
pub fn init(env: Environment) {
    let default = if env == Environment::Development { "debug" } else { "info" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if env == Environment::Development {
        builder.pretty().try_init()
    } else {
        builder.json().try_init()
    };
}
```

```rust
// healthcheck.rs
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, bail};

/// `GET /health` over plain HTTP/1.1. Raw TCP on purpose: `postit-http` stays the only
/// `reqwest` factory, and a healthcheck needs nothing more.
///
/// # Errors
///
/// Fails when the connection fails or the status is not 200.
pub fn probe(addr: SocketAddr) -> anyhow::Result<()> {
    let timeout = Duration::from_secs(5);
    let mut stream = TcpStream::connect_timeout(&addr, timeout).with_context(|| format!("connecting to {addr}"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut head = [0_u8; 32];
    let read = stream.read(&mut head)?;
    let line = String::from_utf8_lossy(&head[..read]);
    if line.starts_with("HTTP/1.1 200") || line.starts_with("HTTP/1.0 200") {
        return Ok(());
    }
    bail!("unhealthy: {}", line.lines().next().unwrap_or_default())
}
```

- [ ] **Step 5: Implement `compose.rs` and `serve.rs` and `lib.rs`**

`compose.rs` builds the services (keep each builder a function so `start` reads as the plan's startup list):

```rust
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use postit_api::state::Readiness;
use postit_config::{Environment, Settings};
use postit_core::{IdGenerator, SystemIdGenerator};
use postit_identity::auth::{Authenticate, Authenticator, AuthenticatorParts};
use postit_identity::cache::PrincipalCache;
use postit_identity::claims::{ClaimsConfig, ClaimsTransformer};
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::verifier::Verifier;
use postit_jobs::{JobQueue, JobRegistry, WorkerHealth};
use postit_mail::{MailLoaders, MailOutbox, SendEmailDeps, SendEmailHandler, SmtpMailer};
use sqlx::PgPool;

pub fn algorithms(names: &[String]) -> anyhow::Result<Vec<jsonwebtoken::Algorithm>> {
    names
        .iter()
        .map(|n| n.parse::<jsonwebtoken::Algorithm>().with_context(|| format!("auth.oidc.accepted_algorithms: {n}")))
        .collect()
}

pub struct Identity {
    pub auth: Arc<Authenticator<HttpJwksSource>>,
    pub cache: PrincipalCache,
}

pub fn identity(
    settings: &Settings,
    pool: &PgPool,
    http: &reqwest::Client,
    ids: &Arc<dyn IdGenerator>,
    outbox: &MailOutbox,
) -> anyhow::Result<Identity> {
    let oidc = &settings.auth.oidc;
    let discovery = Arc::new(OidcDiscovery::new(
        HttpJwksSource::new(http.clone(), oidc.issuer.clone()),
        oidc.jwks_refresh_interval,
    ));
    let transformer = ClaimsTransformer::new(
        pool.clone(),
        Arc::clone(ids),
        http.clone(),
        Arc::clone(&discovery),
        outbox.clone(),
        ClaimsConfig {
            claim_names: oidc.claim_names.clone(),
            userinfo_mode: oidc.userinfo,
            bootstrap: settings.auth.bootstrap.clone(),
            approval_email_interval: settings.auth.approval_email_interval,
        },
    );
    let cache = PrincipalCache::new(settings.auth.principal_cache_ttl);
    let auth = Authenticator::new(AuthenticatorParts {
        verifier: Verifier::new(
            oidc.issuer.as_str().trim_end_matches('/'),
            oidc.audiences.clone(),
            algorithms(&oidc.accepted_algorithms)?,
            oidc.leeway,
        ),
        discovery,
        cache: cache.clone(),
        transformer,
        pool: pool.clone(),
    });
    Ok(Identity { auth: Arc::new(auth), cache })
}

pub fn registry(
    settings: &Settings,
    pool: &PgPool,
    ids: &Arc<dyn IdGenerator>,
    jobs: &JobQueue,
    outbox: &MailOutbox,
) -> anyhow::Result<JobRegistry> {
    let mut registry = JobRegistry::default();
    let mut loaders = MailLoaders::default();
    postit_identity::jobs::register(
        &mut registry,
        &mut loaders,
        postit_identity::jobs::IdentityJobs {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            jobs: jobs.clone(),
            pending_ttl: settings.auth.pending_ttl,
            approval_email_interval: settings.auth.approval_email_interval,
            audit: settings.audit.clone(),
            schedules: settings.jobs.schedules.clone(),
        },
    )?;
    postit_jobs::maintenance::register_data_retention(&mut registry, pool.clone(), &settings.jobs)?;
    postit_jobs::maintenance::register_job_history_purge(&mut registry, pool.clone(), &settings.jobs)?;
    let mailer = SmtpMailer::new(&settings.mail).context("building the SMTP mailer")?;
    postit_mail::register(
        &mut registry,
        SendEmailHandler::new(SendEmailDeps {
            pool: pool.clone(),
            ids: Arc::clone(ids),
            mailer: Arc::new(mailer),
            loaders,
            outbox: outbox.clone(),
            app_url: settings.app.public_url.clone(),
            max_attempts: settings.mail.send_email_max_attempts,
        }),
    )?;
    Ok(registry)
}

/// `/ready` for the role: the database always, the JWKS for API roles, the worker for
/// worker roles. Role `all` serves this same check on both ports.
pub struct RoleReadiness {
    pub pool: PgPool,
    pub auth: Option<Arc<dyn Authenticate>>,
    pub worker: Option<WorkerHealth>,
}

#[async_trait]
impl Readiness for RoleReadiness {
    async fn check(&self) -> Result<(), &'static str> {
        sqlx::query("SELECT 1").execute(&self.pool).await.map_err(|_| "database")?;
        if self.auth.as_ref().is_some_and(|a| !a.jwks_ready()) {
            return Err("jwks");
        }
        if self.worker.as_ref().is_some_and(|w| !w.is_ready()) {
            return Err("worker");
        }
        Ok(())
    }
}

pub fn system_ids() -> Arc<dyn IdGenerator> {
    Arc::new(SystemIdGenerator)
}

pub fn is_production(env: Environment) -> bool {
    env == Environment::Production
}
```

Add `reqwest.workspace = true` to the server's dependencies (the client is built by `postit_http::build_client`; the server only passes it along). Remove `is_production` if unused.

`serve.rs`:

```rust
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use tokio_util::sync::CancellationToken;

pub async fn tls_config(cert: &Path, key: &Path) -> anyhow::Result<RustlsConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    RustlsConfig::from_pem_file(cert, key)
        .await
        .with_context(|| format!("loading TLS cert {} / key {}", cert.display(), key.display()))
}

/// Serves `router` on `listener` until `shutdown`, then drains for up to `grace`.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    tls: Option<RustlsConfig>,
    shutdown: CancellationToken,
    grace: Duration,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let handle = Handle::new();
    let signal = handle.clone();
    tokio::spawn(async move {
        shutdown.cancelled().await;
        signal.graceful_shutdown(Some(grace));
    });
    let app = router.into_make_service_with_connect_info::<SocketAddr>();
    match tls {
        Some(config) => axum_server::from_tcp_rustls(listener, config)?.handle(handle).serve(app).await?,
        None => axum_server::from_tcp(listener)?.handle(handle).serve(app).await?,
    }
    Ok(())
}
```

If `axum_server::from_tcp` in 0.8 returns the server directly rather than a `Result`, drop the `?`.

`lib.rs`:

```rust
//! The composition root (plan 02, `postit-server`). `main.rs` calls [`run_from_env`];
//! tests call [`start`] with their own pool and ephemeral listeners.

pub mod compose;
pub mod healthcheck;
pub mod role;
pub mod serve;
pub mod telemetry;

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use postit_api::{ApiSettings, AppState, Limits, api_router, probe_router};
use postit_config::{Environment, Settings};
use postit_identity::admin::UserAdminService;
use postit_identity::auth::Authenticate;
use postit_jobs::{JobQueue, Worker};
use postit_mail::MailOutbox;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub use role::Role;

pub struct StartOptions {
    pub env: Environment,
    pub role: Role,
    pub settings: Settings,
    /// Use this pool instead of connecting from `settings.database` (tests).
    pub db: Option<postit_data::Db>,
    pub api_listener: Option<TcpListener>,
    pub worker_listener: Option<TcpListener>,
}

pub struct RunningServer {
    pub api_addr: Option<SocketAddr>,
    pub worker_addr: Option<SocketAddr>,
    pub shutdown: CancellationToken,
    pub handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

fn bind(host: &str, port: u16) -> anyhow::Result<TcpListener> {
    TcpListener::bind((host, port)).with_context(|| format!("binding {host}:{port}"))
}

/// Startup per plan 02: migrate, identity, bootstrap check, services, job registry, serve.
///
/// # Errors
///
/// Any startup failure; nothing is left running when this returns `Err`.
pub async fn start(opts: StartOptions) -> anyhow::Result<RunningServer> {
    let StartOptions { env, role, settings, db, api_listener, worker_listener } = opts;
    if let Ok(dump) = settings.redacted_dump() {
        tracing::info!(config = %dump, role = ?role, "starting postit");
    }
    if settings.server.web.enabled {
        tracing::warn!("server.web.enabled is set; serving the web client arrives in plan 02 P7 and is ignored");
    }

    let db = match db {
        Some(db) => db,
        None => postit_data::Db::connect(&settings.database).await.context("connecting to the database")?,
    };
    db.run_migrations().await.context("running postit migrations")?;
    postit_jobs::migrate(db.pool()).await.context("running job storage migrations")?;
    let pool = db.pool().clone();

    postit_identity::bootstrap::check_startup(&pool, &settings.auth.bootstrap, env).await?;

    let http = postit_http::build_client(&settings.http).context("building the HTTP client")?;
    let ids = compose::system_ids();
    let jobs = JobQueue::new(pool.clone(), Arc::clone(&ids));
    let outbox = MailOutbox::new(jobs.clone());
    let shutdown = CancellationToken::new();
    let grace = settings.server.shutdown_timeout;
    let tls = if settings.server.tls.enabled {
        let (Some(cert), Some(key)) = (&settings.server.tls.cert_path, &settings.server.tls.key_path) else {
            anyhow::bail!("server.tls.enabled requires server.tls.cert_path and server.tls.key_path");
        };
        Some(serve::tls_config(cert, key).await?)
    } else {
        None
    };

    let mut tasks: JoinSet<anyhow::Result<()>> = JoinSet::new();
    let mut background: JoinSet<()> = JoinSet::new();

    let auth: Option<Arc<dyn Authenticate>> = if role.runs_api() {
        let identity = compose::identity(&settings, &pool, &http, &ids, &outbox)?;
        let auth: Arc<dyn Authenticate> = identity.auth.clone();
        let prefetch = Arc::clone(&auth);
        background.spawn(async move {
            let mut delay = Duration::from_secs(1);
            while let Err(err) = prefetch.prefetch().await {
                tracing::warn!(error = %err, retry_in = ?delay, "JWKS not loaded yet");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(60));
            }
            tracing::info!("JWKS loaded");
        });
        background.spawn(postit_identity::cache::run_listener(pool.clone(), identity.cache.clone()));
        Some(auth)
    } else {
        None
    };

    let worker_health = if role.runs_worker() {
        let registry = compose::registry(&settings, &pool, &ids, &jobs, &outbox)?;
        let worker = Worker::new(pool.clone(), &settings.jobs, registry);
        let health = worker.health();
        let stop = shutdown.clone();
        tasks.spawn(async move {
            match tokio::time::timeout(grace + Duration::from_secs(5), worker.run(async move { stop.cancelled().await })).await {
                Ok(result) => result.map_err(anyhow::Error::from),
                Err(_) => {
                    tracing::warn!("job worker did not drain within server.shutdown_timeout");
                    Ok(())
                }
            }
        });
        Some(health)
    } else {
        None
    };

    let readiness: Arc<dyn postit_api::Readiness> = Arc::new(compose::RoleReadiness {
        pool: pool.clone(),
        auth: auth.clone(),
        worker: worker_health,
    });

    let mut api_addr = None;
    if let Some(auth) = auth {
        let api_settings = ApiSettings::from_settings(env, &settings).map_err(anyhow::Error::msg)?;
        let limits = Arc::new(Limits::new(&api_settings.rate_limit));
        let sweeper = Arc::clone(&limits);
        background.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                sweeper.retain_recent();
            }
        });
        let state = AppState {
            auth,
            admin: UserAdminService::new(pool.clone(), Arc::clone(&ids), jobs.clone(), outbox.clone()),
            pool: pool.clone(),
            settings: Arc::new(api_settings),
            limits,
            readiness: Arc::clone(&readiness),
        };
        let listener = match api_listener {
            Some(l) => l,
            None => bind(&settings.server.host, settings.server.api_port)?,
        };
        api_addr = Some(listener.local_addr()?);
        tasks.spawn(serve::serve(listener, api_router(state), tls.clone(), shutdown.clone(), grace));
    }

    let mut worker_addr = None;
    if role.runs_worker() {
        let listener = match worker_listener {
            Some(l) => l,
            None => bind(&settings.server.host, settings.server.worker_port)?,
        };
        worker_addr = Some(listener.local_addr()?);
        tasks.spawn(serve::serve(listener, probe_router(Arc::clone(&readiness)), tls, shutdown.clone(), grace));
    }

    let stop = shutdown.clone();
    let handle = tokio::spawn(async move {
        let mut first_error = None;
        while let Some(joined) = tasks.join_next().await {
            let result = joined.map_err(anyhow::Error::from).and_then(|r| r);
            if let Err(err) = result {
                tracing::error!(error = %format!("{err:#}"), "a server task failed; shutting down");
                stop.cancel();
                first_error.get_or_insert(err);
            }
        }
        background.shutdown().await;
        pool.close().await;
        first_error.map_or(Ok(()), Err)
    });

    Ok(RunningServer { api_addr, worker_addr, shutdown, handle })
}

async fn signal(shutdown: CancellationToken) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutdown signal received");
    shutdown.cancel();
}

fn config_dir() -> PathBuf {
    std::env::var_os("POSTIT_CONFIG_DIR").map_or_else(|| PathBuf::from("config"), PathBuf::from)
}

fn environment() -> anyhow::Result<Environment> {
    Environment::from_env().context("resolving POSTIT_ENV")
}

/// `postit` with no arguments.
///
/// # Errors
///
/// Startup failures, or the first failing server task.
pub async fn run_from_env() -> anyhow::Result<()> {
    let env = environment()?;
    let role = Role::resolve(std::env::var("POSTIT_ROLE").ok().as_deref(), env)?;
    let settings = postit_config::load(env, &config_dir()).context("loading configuration")?;
    telemetry::init(env);
    let running = start(StartOptions {
        env,
        role,
        settings,
        db: None,
        api_listener: None,
        worker_listener: None,
    })
    .await?;
    tracing::info!(api = ?running.api_addr, worker = ?running.worker_addr, "postit is listening");
    tokio::spawn(signal(running.shutdown.clone()));
    running.handle.await.context("server task panicked")?
}

/// `postit healthcheck`.
///
/// # Errors
///
/// Unhealthy, unreachable, or TLS enabled.
pub fn healthcheck_from_env() -> anyhow::Result<()> {
    let env = environment()?;
    let role = Role::resolve(std::env::var("POSTIT_ROLE").ok().as_deref(), env)?;
    let settings = postit_config::load(env, &config_dir()).context("loading configuration")?;
    if settings.server.tls.enabled {
        anyhow::bail!("healthcheck requires plain HTTP (server.tls.enabled = true)");
    }
    let port = if role.runs_api() { settings.server.api_port } else { settings.server.worker_port };
    healthcheck::probe(SocketAddr::from(([127, 0, 0, 1], port)))
}
```

Adjust the `Environment::from_env` call to its real signature (`rg "pub fn from_env" -A10 server/crates/config/src/environment.rs`; it may take `is_debug_build: bool` → pass `cfg!(debug_assertions)`).

`main.rs`:

```rust
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") => {
            println!("postit {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("healthcheck") => postit_server::healthcheck_from_env(),
        Some(other) => anyhow::bail!("unknown argument: {other} (usage: postit [--version | healthcheck])"),
        None => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(postit_server::run_from_env()),
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p postit-server`
Expected: PASS. Then `cargo run -p postit-server -- --version` prints the version.

- [ ] **Step 7: Gates and commit**

```bash
git add server/Cargo.toml server/Cargo.lock server/crates/server
git commit -m "postit-server: composition root with roles, rustls TLS, graceful shutdown, healthcheck, job registration"
```

---

### Task 16: Docker image, dev compose service, nginx proxy

**Files:**
- Modify: `docker/server.Dockerfile`, `docker/docker-compose.development.yml`, `docker/shared/nginx/app.conf`
- Modify: `README.md` (the dev-stack section: one paragraph on `./stack.ps1 up -App` now running the real server)

**Interfaces:**
- Consumes: the `postit` binary (`healthcheck`, roles), `POSTIT__…` list values (Task 1).

- [ ] **Step 1: Dockerfile**

Replace the builder stage with cargo-chef stages and add `EXPOSE`/`HEALTHCHECK`:

```dockerfile
FROM rust:1-slim-trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY server/ ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
# rust-toolchain.toml sits beside the recipe so the cooked dependencies use the same toolchain.
COPY server/rust-toolchain.toml ./
RUN cargo chef cook --profile dist --recipe-path recipe.json
COPY server/ ./
ENV SQLX_OFFLINE=true
RUN cargo build --profile dist --package postit-server
```

Keep the runtime stage as is and add, after `COPY server/config/...`:

```dockerfile
EXPOSE 8080 8081 8082
HEALTHCHECK --interval=15s --timeout=6s --start-period=30s --retries=3 CMD ["postit", "healthcheck"]
```

Update the header comment: drop "still to come" items that are now done (cargo-chef, `SQLX_OFFLINE`, `HEALTHCHECK`); the Flutter stage remains for P7.

- [ ] **Step 2: Dev compose service and subnet**

In `docker/docker-compose.development.yml`, add under `services:`:

```yaml
    # The real server (role all) behind postit-nginx-app. Plain HTTP inside the network;
    # nginx terminates TLS on 44310/44311 with the dev certificate.
    postit-server:
        profiles: ["app"]
        image: "postit-server:local"
        build:
            context: ..
            dockerfile: docker/server.Dockerfile
        container_name: postit-server
        restart: unless-stopped
        stop_grace_period: 40s
        env_file:
            # database.username / database.password, audit.pseudonym_key.
            - "../!ref/vault/development/postit.env"
        environment:
            POSTIT_ENV: development
            POSTIT_ROLE: all
            POSTIT__SERVER__TLS__ENABLED: "false"
            POSTIT__SERVER__API_PORT: "8080"
            POSTIT__SERVER__WORKER_PORT: "8081"
            POSTIT__DATABASE__URL: "postgres://postit-postgres:5432/postit"
            # The operator's local SMTP tool (e.g. Papercut) on the Docker host.
            POSTIT__MAIL__SMTP__HOST: host.docker.internal
            # postit-nginx-app forwards client IPs from inside this subnet.
            POSTIT__SERVER__TRUSTED_PROXIES: "[172.30.0.0/24]"
            # Trust the dev CA so https://postit.local:44300 (Zitadel via postit-nginx-infra)
            # verifies from inside the container.
            POSTIT__HTTP__EXTRA_CA_FILES: "[/certs/postit-dev-ca.crt]"
        extra_hosts:
            - "host.docker.internal:host-gateway"
        volumes:
            # development.toml and local.toml (issuer, audiences, client id from
            # `cargo xtask zitadel-bootstrap`) replace the image's baked config.
            - "../server/config:/app/config:ro"
            - "./shared/nginx/certs/postit-dev-ca.crt:/certs/postit-dev-ca.crt:ro"
        depends_on:
            postit-postgres:
                condition: service_healthy
            postit-nginx-infra:
                condition: service_started
        networks:
            - postit-net
```

Add `depends_on: [postit-server]` to `postit-nginx-app`. Append at the end of the file:

```yaml
networks:
    postit-net:
        ipam:
            config:
                # Fixed so POSTIT__SERVER__TRUSTED_PROXIES can name it. An existing
                # postit-net created without it must be recreated: `./stack.ps1 down`, then up.
                - subnet: 172.30.0.0/24
```

Run: `./stack.sh config development` (or `./stack.ps1 config development`) from the repo root.
Expected: the resolved config shows `postit-server` in profile `app` and the subnet on `postit-net`, with no errors.

- [ ] **Step 3: nginx proxy**

Replace the 44310 and 44311 `server` blocks in `docker/shared/nginx/app.conf` (keep 44315 as the placeholder) and update the header comment:

```nginx
# postit-nginx-app (compose profile "app"): TLS for the API (44310) and worker (44311),
# proxied to postit-server; web (44315) stays a placeholder until plan 02 P7.

server {
    listen 44310 ssl;
    server_name postit.local;

    ssl_certificate     /etc/nginx/certs/postit.local.crt;
    ssl_certificate_key /etc/nginx/certs/postit.local.key;

    location / {
        proxy_pass http://postit-server:8080;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $http_host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 90s;
    }
}

server {
    listen 44311 ssl;
    server_name postit.local;

    ssl_certificate     /etc/nginx/certs/postit.local.crt;
    ssl_certificate_key /etc/nginx/certs/postit.local.key;

    location / {
        proxy_pass http://postit-server:8081;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $http_host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

- [ ] **Step 4: Build the image**

Run from the repo root: `docker build -f docker/server.Dockerfile -t postit-server:local .`
Expected: success. Then `docker run --rm postit-server:local --version` prints `postit 0.7.0`.

- [ ] **Step 5: README and commit**

In `README.md`'s dev-stack section, state that `./stack.ps1 up -App` now builds and runs `postit-server` behind nginx on 44310/44311, that the cert files must be the current `postit.local.*` / `postit-dev-ca.crt` names from `./cert.ps1`, and that the first run after this change needs `./stack.ps1 down` so `postit-net` is recreated with its fixed subnet.

```bash
git add docker README.md
git commit -m "docker: cargo-chef image with HEALTHCHECK, dev postit-server service behind nginx-app"
```

---

### Task 17: Close-out — rulings in the spec, docs, manual exit verification

**Files:**
- Modify: `docs/superpowers/specs/2026-09-28-p6-api-server-design.md` (Decisions)
- Modify: `CLAUDE.md` (Commands: `cargo xtask openapi`)

- [ ] **Step 1: Record the rulings**

Append this plan's "Rulings made while writing this plan" bullets to the spec's Decisions section, plus any version pins changed in Task 7 Step 1 (utoipa / utoipa-swagger-ui / axum-server as resolved).

- [ ] **Step 2: CLAUDE.md**

Under Commands, add:

```sh
cargo xtask openapi             # regenerate api/openapi.json (--check to verify it is current)
```

- [ ] **Step 3: Full verification**

From `server/`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
cargo xtask openapi --check
```

and from `server/crates/data` with `DATABASE_URL` set: `cargo sqlx prepare --check`.
Expected: all pass.

- [ ] **Step 4: Manual exit — native (needs the maintainer's machine)**

Preconditions: `./cert.ps1` has generated `docker/shared/nginx/certs/postit.local.{crt,key}` and `postit-dev-ca.crt` (the directory may still hold stale `postly.*` files from before the rename), `./stack.ps1 up` is running Postgres, Zitadel, and `postit-nginx-infra`, and `cargo xtask zitadel-bootstrap` has written `server/config/local.toml`.

1. From `server/`: `cargo run -p postit-server`. Expect logs `JWKS loaded` and `postit is listening`.
2. `https://postit.local:44311/ready` returns `{"status":"ready"}`.
3. Open `https://postit.local:44310/docs/`, click Authorize, sign in as `admin@postit.com` through Zitadel.
4. In Swagger UI, run `GET /api/v1/me`: 200 with `role: admin`, `status: active`.
5. Stop the server with Ctrl-C: it exits 0 after draining.

- [ ] **Step 5: Manual exit — Docker**

1. `./stack.ps1 down`, then `./stack.ps1 up -App` (builds `postit-server:local`).
2. `docker ps` shows `postit-server` as `healthy`.
3. Repeat steps 2–4 above against the same URLs (now served by nginx → container).
4. `docker logs postit-server` shows JSON-free pretty logs (development) and no bearer token.

Report the outcome of Steps 4–5 to the maintainer; they are the P6 dev-stack exit criteria and cannot run in CI.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-09-28-p6-api-server-design.md CLAUDE.md
git commit -m "P6: record execution rulings in the design spec (close-out)"
```
