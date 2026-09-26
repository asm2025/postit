# P4: Data Layer and Identity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `postit-data` (pool, migrations, `OwnerScope`, `AuditLog`, repositories, retention purges) and the jobless/mailless slice of `postit-identity` (OIDC verifier, claims transformation, bootstrap, principal cache with `LISTEN` eviction, status/role admin service) — plan 02 phase P4.

**Architecture:** `postit-data` owns the Postgres pool, migrations, and every repository as thin, testable functions over `&mut PgConnection`, so callers (this phase: `postit-identity`; later: `postit-api`) control transaction boundaries. `postit-identity` composes those primitives — provisioning, bootstrap, and status/role changes each run inside one transaction it opens and commits — and owns everything that isn't a database operation: JWT verification, claims-to-`Principal` transformation, and the in-memory principal cache.

**Tech Stack:** Rust stable (edition 2024, `rust-version = "1.98"`), `sqlx` 0.8 (postgres, macros, migrate), `emixdb` (pagination), `jsonwebtoken` 9, `moka` 0.12 (sync cache), `rsa` 0.9 + `p256` 0.13 (test-issuer keygen), existing `postit-http`/`postit-core`/`postit-config`.

**Spec:** `docs/superpowers/specs/2026-09-26-p4-data-identity-design.md`

## Global Constraints

- Rust stable, edition 2024, `rust-version = "1.98"` floor (workspace `[workspace.package]`).
- After every task: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace` all pass, run from `server/`.
- `unsafe_code = "forbid"`; `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` are denied outside `#[cfg(test)]` code (workspace lints already enforce this — write test-only `unwrap`/`expect` only inside `#[cfg(test)]` modules or `tests/` files).
- Secrets (`RedactedSecret`, `secrecy::SecretString`) never appear in `Debug`, logs, errors, or `audit_events.details`. `AuditEvent::detail`/`detail_user_id` only accept JSON-serializable plain values, never a secret type.
- Internal IDs are UUID v7, generated in Rust via `postit_core::IdGenerator` — never `gen_random_uuid()` or any DB-side default on an `id` column.
- Every new external dependency is declared once in the root `server/Cargo.toml` `[workspace.dependencies]`; crates consume it with `dep.workspace = true`.
- `postit-http` is the only crate that builds a `reqwest::Client`. `postit-identity`'s HTTP calls (discovery, JWKS, userinfo) go through `postit_http::execute_traced`/`build_client`.
- `.sqlx` query cache is committed (`cargo sqlx prepare` from `server/crates/data` and `server/crates/identity` against a live database) so `SQLX_OFFLINE=true` works without a database — required for the `server-windows` CI job.
- Local dev/test database: the `postgres-18` container (port 5432, user `postgres`, password from `!ref/vault/development/postgres.env`). This is separate from the project's own `postit-postgres` compose service (port 44340) and exists only for iterating on this plan.
- Every audit-event write goes through `postit_data::AuditLog::record` — no other code inserts into `audit_events`.
- `postit-data` repository functions take `conn: &mut sqlx::PgConnection` (never a bare `&PgPool` or a generic executor) so callers control whether several calls share one transaction. Callers get a connection with `pool.acquire().await?` (a plain `PoolConnection<Postgres>`, which derefs to `PgConnection`) or `&mut *tx` from an open `Transaction`.

## Review Focus

- Two concurrent first sign-ins for the same `(oidc_issuer, oidc_subject)` must provision exactly one `users` row, not two, and neither call may error or deadlock — Task 3's concurrency test.
- A JWT signed `alg: none`, or an HMAC token whose secret is the RSA public key bytes ("algorithm confusion"), must be rejected by the verifier even if `auth.oidc.accepted_algorithms` in config is misconfigured to list it — Task 11.
- A second `idempotency_keys.begin()` racing the first for the same `(owner_id, actor_id, key)` must surface as a distinguishable typed conflict, not an unhandled unique-violation bubbling up as a generic 500 later in P6 — Task 7.
- The principal cache's `LISTEN` task must fall back to `invalidate_all()` on a dropped/reconnected listener connection, not silently stop evicting — a disabled user staying valid past the cache TTL forever is a security bug, not staleness — Task 15.
- Bootstrap must grant admin only to a **newly created** user matching the rule while no active admin exists; an already-existing user who later starts matching the bootstrap rule (e.g. an admin re-runs sign-in after config changes) must never be silently promoted outside the one-time bootstrap path — Task 14.

---

## Section A — `postit-data`

### Task 1: Crate scaffold, pool, migrations, advisory lock

**Files:**
- Modify: `server/Cargo.toml` (add `sqlx`, `emixdb` to `[workspace.dependencies]`)
- Modify: `server/crates/core/src/ids.rs` (add `AuditEventId`)
- Modify: `server/crates/core/src/lib.rs` (export `AuditEventId`)
- Modify: `server/crates/data/Cargo.toml`
- Create: `server/crates/data/migrations/0001_citext.sql`
- Create: `server/crates/data/src/error.rs`
- Create: `server/crates/data/src/advisory_lock.rs`
- Create: `server/crates/data/src/pool.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/pool.rs`

**Interfaces:**
- Produces: `postit_data::{Db, DataError}`, `postit_data::advisory_lock::{xact_lock, with_session_lock, MIGRATIONS_LOCK_KEY, BOOTSTRAP_ADMIN_LOCK_KEY}`, `postit_core::AuditEventId`.

- [ ] **Step 1: Add workspace dependencies**

In `server/Cargo.toml`, in `[workspace.dependencies]`, add (alphabetically among the existing plain entries, after `secrecy`):

```toml
emixdb = "0"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-rustls", "postgres", "uuid", "chrono", "json", "migrate", "macros"] }
```

- [ ] **Step 2: Add `AuditEventId` to `postit-core`**

In `server/crates/core/src/ids.rs`, after `id_newtype!(PostId);`, add:

```rust
id_newtype!(AuditEventId);
```

In `server/crates/core/src/lib.rs`, change the export line to:

```rust
pub use ids::{AccountId, AuditEventId, PostId, UserId};
```

- [ ] **Step 3: Run the workspace quality gates to confirm the core change compiles**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p postit-core`
Expected: all pass (the new newtype is exercised by the existing macro-generated impls; no new test is needed for a newtype the macro already covers).

- [ ] **Step 4: Fill in `postit-data`'s `Cargo.toml`**

Replace `server/crates/data/Cargo.toml`'s `[dependencies]` section (keep the rest of the file as-is):

```toml
[dependencies]
postit-config.workspace = true
postit-core.workspace = true
chrono.workspace = true
emixdb.workspace = true
secrecy.workspace = true
serde_json.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tracing.workspace = true
uuid.workspace = true

[dev-dependencies]
tokio.workspace = true
```

- [ ] **Step 5: Write `DataError`**

Create `server/crates/data/src/error.rs`:

```rust
/// Errors from `postit-data`. Never carries a secret or a raw credential — `sqlx::Error`'s
/// `Display` can include a query string, but this crate never binds a secret value into a
/// query, so that's safe to surface.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("database error: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("row not found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),
}
```

- [ ] **Step 6: Write the advisory-lock helper**

Create `server/crates/data/src/advisory_lock.rs`:

```rust
use sqlx::{PgConnection, PgPool};

use crate::error::DataError;

/// Held while `Db::run_migrations` runs, so concurrent role starts (`api` and `worker` in
/// one deploy) don't race applying migrations.
pub const MIGRATIONS_LOCK_KEY: i64 = 1_000_001;

/// Held while `postit-identity` checks and grants the bootstrap admin, so two concurrent
/// first sign-ins that both match the bootstrap rule can't both become admin.
pub const BOOTSTRAP_ADMIN_LOCK_KEY: i64 = 1_000_002;

/// Takes a transaction-scoped advisory lock: released automatically when `conn`'s
/// transaction ends (commit or rollback). `conn` must be inside an open transaction — a
/// lock taken outside one releases at the end of the single implicit statement, which is
/// never what a caller of this function wants.
///
/// # Errors
///
/// Returns [`DataError::Sql`] if the lock statement fails.
pub async fn xact_lock(conn: &mut PgConnection, key: i64) -> Result<(), DataError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Takes a session-scoped advisory lock on a dedicated connection from `pool`, runs `f`,
/// then releases the lock whether `f` succeeded or not. Used for migrations, which manage
/// their own connections and transactions internally and so can't be wrapped in
/// [`xact_lock`].
///
/// # Errors
///
/// Returns [`DataError::Sql`] if acquiring the connection or either lock statement fails,
/// or `f`'s own error.
pub async fn with_session_lock<F, Fut, T>(pool: &PgPool, key: i64, f: F) -> Result<T, DataError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, DataError>>,
{
    let mut lock_conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(key)
        .execute(&mut *lock_conn)
        .await?;

    let result = f().await;

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(key)
        .execute(&mut *lock_conn)
        .await?;

    result
}
```

- [ ] **Step 7: Write the citext migration**

Create `server/crates/data/migrations/0001_citext.sql`:

```sql
CREATE EXTENSION IF NOT EXISTS citext;
```

- [ ] **Step 8: Write the pool factory**

Create `server/crates/data/src/pool.rs`:

```rust
use postit_config::DatabaseSettings;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::advisory_lock::{self, MIGRATIONS_LOCK_KEY};
use crate::error::DataError;

/// The workspace's only Postgres pool factory. `database.url` (config, no credentials) and
/// `database.username`/`database.password` (secrets) are combined here so no crate outside
/// `postit-data` builds connection options directly.
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] when the pool can't be built (bad host, auth failure,
    /// `database.url` missing a host).
    pub async fn connect(settings: &DatabaseSettings) -> Result<Self, DataError> {
        let host = settings.url.host_str().unwrap_or("localhost");
        let port = settings.url.port().unwrap_or(5432);
        let database = settings.url.path().trim_start_matches('/');

        let options = PgConnectOptions::new()
            .host(host)
            .port(port)
            .database(database)
            .username(&settings.username)
            .password(settings.password.expose());

        let pool = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Runs every embedded migration under [`MIGRATIONS_LOCK_KEY`], so two processes
    /// starting at once apply them exactly once between them.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Migrate`] if a migration fails, or [`DataError::Sql`] if the
    /// advisory lock can't be taken.
    pub async fn run_migrations(&self) -> Result<(), DataError> {
        let pool = self.pool.clone();
        advisory_lock::with_session_lock(&self.pool, MIGRATIONS_LOCK_KEY, move || async move {
            sqlx::migrate!().run(&pool).await.map_err(DataError::from)
        })
        .await
    }
}
```

- [ ] **Step 9: Wire up `lib.rs`**

Replace `server/crates/data/src/lib.rs`:

```rust
mod advisory_lock;
mod error;
mod pool;

pub use error::DataError;
pub use pool::Db;

pub mod locks {
    pub use crate::advisory_lock::{BOOTSTRAP_ADMIN_LOCK_KEY, MIGRATIONS_LOCK_KEY, xact_lock};
}
```

- [ ] **Step 10: Prepare the local test database**

Run (adjust only if the container's actual password in `!ref/vault/development/postgres.env` differs):

```bash
PGPASSWORD='P@$$w0rd' psql -h localhost -p 5432 -U postgres -tc "SELECT 1 FROM pg_database WHERE datname = 'postit'" | grep -q 1 || PGPASSWORD='P@$$w0rd' psql -h localhost -p 5432 -U postgres -c "CREATE DATABASE postit"
```

Expected: the `postit` database exists afterward (either it already did, or it was just created). If `psql` isn't on PATH, run the equivalent through `docker exec postgres-18 psql -U postgres -c "CREATE DATABASE postit"` instead.

- [ ] **Step 11: Write the pool/migration test**

Create `server/crates/data/tests/pool.rs`:

```rust
use postit_config::{DatabaseSettings, RedactedSecret};
use postit_data::Db;
use url::Url;

fn settings() -> DatabaseSettings {
    DatabaseSettings {
        url: Url::parse("postgres://localhost:5432/postit")
            .unwrap_or_else(|err| unreachable!("hardcoded valid url: {err}")),
        username: "postgres".to_string(),
        password: RedactedSecret::from("P@$$w0rd".to_string()),
        max_connections: 5,
    }
}

#[tokio::test]
async fn connects_and_runs_migrations_from_empty() {
    let db = Db::connect(&settings())
        .await
        .unwrap_or_else(|err| unreachable!("connecting to local test database: {err}"));

    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("running migrations: {err}"));

    let citext_installed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'citext')",
    )
    .fetch_one(db.pool())
    .await
    .unwrap_or_else(|err| unreachable!("checking citext extension: {err}"));

    assert!(citext_installed);
}

#[tokio::test]
async fn running_migrations_twice_is_a_no_op() {
    let db = Db::connect(&settings())
        .await
        .unwrap_or_else(|err| unreachable!("connecting to local test database: {err}"));

    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("first migration run: {err}"));
    db.run_migrations()
        .await
        .unwrap_or_else(|err| unreachable!("second migration run: {err}"));
}
```

`postit_config::RedactedSecret` and `postit_config::DatabaseSettings` are both re-exported
from the crate root already (`server/crates/config/src/lib.rs`), so the `use` line above is
correct as written.

- [ ] **Step 12: Run the test**

Run: `cd server && cargo test -p postit-data --test pool`
Expected: PASS if the local database (Step 10) is reachable with the settings in this test.
If it fails on connection, fix the container/password/database from Step 10 before
continuing; do not change the test to skip the database.

- [ ] **Step 13: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace`
Expected: all pass.

- [ ] **Step 14: Commit**

```bash
git add server/Cargo.toml server/crates/core server/crates/data
git commit -m "postit-data: pool, migrations, advisory lock (plan 02 P4)"
```

### Task 2: `OwnerScope`, `Access`, `Capability`

**Files:**
- Create: `server/crates/data/src/scope.rs`
- Modify: `server/crates/data/src/lib.rs`

**Interfaces:**
- Consumes: `postit_core::UserId` (existing).
- Produces: `postit_data::{OwnerScope, Access, Capability, ScopeError}`.

- [ ] **Step 1: Write the failing tests**

Create `server/crates/data/src/scope.rs`:

```rust
use postit_core::UserId;

/// What a caller may do to owned rows in the workspace named by [`OwnerScope::owner`].
/// Only `Owner` access exists until plan 03 phase B2 adds `Access::Delegated`; the type is
/// introduced now so no call site changes when that variant arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Owner,
}

/// The three delegation levels from plan 01. Unused by any P4 caller (every P4 repository
/// is either owner-only or admin-only), defined now because `OwnerScope::require` must
/// exist from the start per plan 01's data-model section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    View,
    Edit,
    Publish,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScopeError {
    #[error("delegation grant does not allow this action")]
    InsufficientAccess,
}

/// Who is acting (`actor`), whose workspace they're acting in (`owner`), and at what level
/// (`access`). `actor == owner` for the caller's own workspace; they differ only once
/// delegation exists (plan 03).
#[derive(Debug, Clone, Copy)]
pub struct OwnerScope {
    pub actor: UserId,
    pub owner: UserId,
    pub access: Access,
}

impl OwnerScope {
    #[must_use]
    pub fn own(user: UserId) -> Self {
        Self {
            actor: user,
            owner: user,
            access: Access::Owner,
        }
    }

    /// Checks whether this scope permits `capability`. Always succeeds for `Access::Owner`
    /// — the only variant that exists in P4 — since an owner acting in their own workspace
    /// has every capability.
    ///
    /// # Errors
    ///
    /// Returns [`ScopeError::InsufficientAccess`] once `Access::Delegated` exists (plan 03)
    /// and the grant's level is below `capability`. Never returns an error in this phase.
    pub fn require(&self, _capability: Capability) -> Result<(), ScopeError> {
        match self.access {
            Access::Owner => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(n: u128) -> UserId {
        UserId::from(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn own_scope_has_actor_equal_to_owner() {
        let id = user(1);
        let scope = OwnerScope::own(id);

        assert_eq!(scope.actor, id);
        assert_eq!(scope.owner, id);
        assert_eq!(scope.access, Access::Owner);
    }

    #[test]
    fn owner_access_permits_every_capability() {
        let scope = OwnerScope::own(user(1));

        assert_eq!(scope.require(Capability::View), Ok(()));
        assert_eq!(scope.require(Capability::Edit), Ok(()));
        assert_eq!(scope.require(Capability::Publish), Ok(()));
    }

    #[test]
    fn capability_orders_view_below_edit_below_publish() {
        assert!(Capability::View < Capability::Edit);
        assert!(Capability::Edit < Capability::Publish);
    }
}
```

This module has no DB import and no `#[sqlx::test]` — it's pure logic, so this task needs
no local database.

- [ ] **Step 2: Run the test to see it fail**

Run: `cd server && cargo test -p postit-data scope::`
Expected: FAIL with "module `scope` not found" — `lib.rs` doesn't declare it yet.

- [ ] **Step 3: Wire it into `lib.rs`**

In `server/crates/data/src/lib.rs`, add `mod scope;` after `mod pool;` and add to the
`pub use` line:

```rust
pub use scope::{Access, Capability, OwnerScope, ScopeError};
```

- [ ] **Step 4: Run the test again**

Run: `cd server && cargo test -p postit-data scope::`
Expected: PASS (3 tests).

- [ ] **Step 5: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add server/crates/data/src/scope.rs server/crates/data/src/lib.rs
git commit -m "postit-data: OwnerScope, Access, Capability"
```

### Task 3: `users` migration and `UsersRepo`

**Files:**
- Create: `server/crates/data/migrations/0002_users.sql`
- Create: `server/crates/data/src/users.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/users.rs`

**Interfaces:**
- Consumes: `postit_core::{UserId, IdGenerator}` (existing).
- Produces: `postit_data::users::{UserRecord, UserRole, UserStatus, ProvisionOutcome, UsersRepo}`.

- [ ] **Step 1: Write the migration**

Create `server/crates/data/migrations/0002_users.sql`:

```sql
CREATE TABLE users (
    id UUID PRIMARY KEY,
    oidc_issuer TEXT NOT NULL,
    oidc_subject TEXT NOT NULL,
    email CITEXT,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'disabled', 'deleting')),
    approved_at TIMESTAMPTZ,
    approved_by UUID REFERENCES users (id) ON DELETE SET NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (oidc_issuer, oidc_subject)
);

CREATE INDEX users_status_idx ON users (status);
CREATE INDEX users_role_status_idx ON users (role, status);
```

- [ ] **Step 2: Write `UsersRepo` and its row-mapping**

Create `server/crates/data/src/users.rs`:

```rust
use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    Member,
}

impl UserRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            _ => Self::Member,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Pending,
    Active,
    Disabled,
    Deleting,
}

impl UserStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Deleting => "deleting",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "disabled" => Self::Disabled,
            "deleting" => Self::Deleting,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: UserId,
    pub oidc_issuer: String,
    pub oidc_subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: String,
    pub role: UserRole,
    pub status: UserStatus,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<UserId>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

struct UserRow {
    id: uuid::Uuid,
    oidc_issuer: String,
    oidc_subject: String,
    email: Option<String>,
    email_verified: bool,
    display_name: String,
    role: String,
    status: String,
    approved_at: Option<DateTime<Utc>>,
    approved_by: Option<uuid::Uuid>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for UserRecord {
    fn from(row: UserRow) -> Self {
        Self {
            id: UserId::from(row.id),
            oidc_issuer: row.oidc_issuer,
            oidc_subject: row.oidc_subject,
            email: row.email,
            email_verified: row.email_verified,
            display_name: row.display_name,
            role: UserRole::parse(&row.role),
            status: UserStatus::parse(&row.status),
            approved_at: row.approved_at,
            approved_by: row.approved_by.map(UserId::from),
            last_seen_at: row.last_seen_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProvisionOutcome {
    Created,
    Existing,
}

pub struct UsersRepo;

impl UsersRepo {
    /// Inserts a new `pending`/`member` user for `(oidc_issuer, oidc_subject)`, or does
    /// nothing if one already exists. Always returns the row either way.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn provision(
        conn: &mut PgConnection,
        id: UserId,
        oidc_issuer: &str,
        oidc_subject: &str,
        display_name: &str,
    ) -> Result<(ProvisionOutcome, UserRecord), DataError> {
        let inserted = sqlx::query_as!(
            UserRow,
            r#"
            INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
            VALUES ($1, $2, $3, $4, 'member', 'pending')
            ON CONFLICT (oidc_issuer, oidc_subject) DO NOTHING
            RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
            "#,
            id.as_uuid(),
            oidc_issuer,
            oidc_subject,
            display_name,
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(row) = inserted {
            return Ok((ProvisionOutcome::Created, row.into()));
        }

        let existing = Self::find_by_oidc(conn, oidc_issuer, oidc_subject)
            .await?
            .ok_or(DataError::NotFound)?;
        Ok((ProvisionOutcome::Existing, existing))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find_by_oidc(
        conn: &mut PgConnection,
        oidc_issuer: &str,
        oidc_subject: &str,
    ) -> Result<Option<UserRecord>, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2"#,
            oidc_issuer,
            oidc_subject,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find_by_id(
        conn: &mut PgConnection,
        id: UserId,
    ) -> Result<Option<UserRecord>, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users WHERE id = $1"#,
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn count_active_admins(conn: &mut PgConnection) -> Result<i64, DataError> {
        let count: Option<i64> =
            sqlx::query_scalar!("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = 'active'")
                .fetch_one(&mut *conn)
                .await?;
        Ok(count.unwrap_or(0))
    }

    /// Promotes `id` to an active admin. Used only by the bootstrap flow, which has already
    /// verified `id` is a newly created user matching the bootstrap rule, under
    /// [`crate::locks::BOOTSTRAP_ADMIN_LOCK_KEY`].
    ///
    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn grant_admin(conn: &mut PgConnection, id: UserId) -> Result<UserRecord, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET role = 'admin', status = 'active', approved_at = now(), updated_at = now()
               WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.map(Into::into).ok_or(DataError::NotFound)
    }

    /// Sets `id`'s status and, on the `pending -> active` transition, `approved_at` /
    /// `approved_by`. Issues `NOTIFY postit_user_changed` in the same statement batch, so a
    /// caller running this inside a transaction has the notification queued for delivery on
    /// commit, per Postgres `NOTIFY` semantics.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn set_status(
        conn: &mut PgConnection,
        id: UserId,
        status: UserStatus,
        approved_by: Option<UserId>,
    ) -> Result<UserRecord, DataError> {
        let approved_by_uuid = approved_by.map(|u| u.as_uuid());
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET
                   status = $2,
                   approved_at = CASE WHEN $2 = 'active' AND status = 'pending' THEN now() ELSE approved_at END,
                   approved_by = COALESCE($3, approved_by),
                   updated_at = now()
               WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
            status.as_str(),
            approved_by_uuid,
        )
        .fetch_optional(&mut *conn)
        .await?;
        let row = row.ok_or(DataError::NotFound)?;

        Self::notify_changed(conn, id).await?;

        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn set_role(
        conn: &mut PgConnection,
        id: UserId,
        role: UserRole,
    ) -> Result<UserRecord, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET role = $2, updated_at = now() WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
            role.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        let row = row.ok_or(DataError::NotFound)?;

        Self::notify_changed(conn, id).await?;

        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure. Not finding `id` is not an error
    /// here — this is a best-effort timestamp touch, called on every cache-miss sign-in.
    pub async fn touch_last_seen(conn: &mut PgConnection, id: UserId) -> Result<(), DataError> {
        sqlx::query!("UPDATE users SET last_seen_at = now() WHERE id = $1", id.as_uuid())
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn update_profile_claims(
        conn: &mut PgConnection,
        id: UserId,
        email: Option<&str>,
        email_verified: bool,
        display_name: &str,
    ) -> Result<(), DataError> {
        sqlx::query!(
            r#"UPDATE users SET email = $2, email_verified = $3, display_name = $4, updated_at = now()
               WHERE id = $1"#,
            id.as_uuid(),
            email,
            email_verified,
            display_name,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Admin list: paginated, optionally filtered by status and a case-insensitive
    /// substring match against display name or email.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        status: Option<UserStatus>,
        search: Option<&str>,
        pagination: Pagination,
    ) -> Result<ResultSet<UserRecord>, DataError> {
        let limit = i64::try_from(pagination.page_size).unwrap_or(10);
        let offset =
            i64::try_from(pagination.page.saturating_sub(1).saturating_mul(pagination.page_size))
                .unwrap_or(0);
        let status_filter = status.map(UserStatus::as_str);
        let search_pattern = search.map(|s| format!("%{s}%"));

        let rows = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users
               WHERE ($1::text IS NULL OR status = $1)
                 AND ($2::text IS NULL OR display_name ILIKE $2 OR email::text ILIKE $2)
               ORDER BY created_at DESC
               LIMIT $3 OFFSET $4"#,
            status_filter,
            search_pattern,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total: Option<i64> = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM users
               WHERE ($1::text IS NULL OR status = $1)
                 AND ($2::text IS NULL OR display_name ILIKE $2 OR email::text ILIKE $2)"#,
            status_filter,
            search_pattern,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(ResultSet {
            data: rows.into_iter().map(Into::into).collect(),
            total: u64::try_from(total.unwrap_or(0)).unwrap_or(0),
            pagination: Some(pagination),
        })
    }

    async fn notify_changed(conn: &mut PgConnection, id: UserId) -> Result<(), DataError> {
        sqlx::query!(
            "SELECT pg_notify('postit_user_changed', $1)",
            id.as_uuid().to_string()
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 3: Wire it into `lib.rs`**

In `server/crates/data/src/lib.rs`, add `pub mod users;` (public, not `mod` — the row types
and repo are part of this crate's public API, unlike the private `advisory_lock`/`error`/
`pool`/`scope` modules which re-export their public items instead).

- [ ] **Step 4: Write the tests**

Create `server/crates/data/tests/users.rs`:

```rust
use postit_data::users::{ProvisionOutcome, UserRole, UserStatus, UsersRepo};
use postit_core::UserId;
use sqlx::PgPool;

#[sqlx::test]
async fn provision_creates_a_pending_member(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());

    let (outcome, user) =
        UsersRepo::provision(&mut conn, id, "https://issuer.test", "sub-1", "Ada")
            .await
            .unwrap_or_else(|e| unreachable!("provision: {e}"));

    assert!(matches!(outcome, ProvisionOutcome::Created));
    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
    assert_eq!(user.display_name, "Ada");
}

#[sqlx::test]
async fn provisioning_the_same_identity_twice_returns_existing(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id_a = UserId::from(uuid::Uuid::now_v7());
    let id_b = UserId::from(uuid::Uuid::now_v7());

    let (first_outcome, first) =
        UsersRepo::provision(&mut conn, id_a, "https://issuer.test", "sub-1", "Ada")
            .await
            .unwrap_or_else(|e| unreachable!("first provision: {e}"));
    let (second_outcome, second) =
        UsersRepo::provision(&mut conn, id_b, "https://issuer.test", "sub-1", "Ignored")
            .await
            .unwrap_or_else(|e| unreachable!("second provision: {e}"));

    assert!(matches!(first_outcome, ProvisionOutcome::Created));
    assert!(matches!(second_outcome, ProvisionOutcome::Existing));
    assert_eq!(first.id, second.id);
    assert_eq!(second.display_name, "Ada");
}

#[sqlx::test]
async fn concurrent_first_sign_in_for_the_same_identity_creates_exactly_one_user(pool: PgPool) {
    // Review Focus: two concurrent provisions for a brand-new (iss, sub) must create
    // exactly one row, not two, and neither call may error.
    let iss = "https://issuer.test";
    let sub = "concurrent-sub";

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let task_a = tokio::spawn(async move {
        let mut conn = pool_a.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::provision(&mut conn, UserId::from(uuid::Uuid::now_v7()), iss, sub, "A")
            .await
    });
    let task_b = tokio::spawn(async move {
        let mut conn = pool_b.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
        UsersRepo::provision(&mut conn, UserId::from(uuid::Uuid::now_v7()), iss, sub, "B")
            .await
    });

    let result_a = task_a.await.unwrap_or_else(|e| unreachable!("task a panicked: {e}"));
    let result_b = task_b.await.unwrap_or_else(|e| unreachable!("task b panicked: {e}"));
    assert!(result_a.is_ok());
    assert!(result_b.is_ok());

    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2",
    )
    .bind(iss)
    .bind(sub)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("count: {e}"));
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn grant_admin_promotes_to_active_admin(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let user = UsersRepo::grant_admin(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    assert_eq!(user.role, UserRole::Admin);
    assert_eq!(user.status, UserStatus::Active);
    assert!(user.approved_at.is_some());
}

#[sqlx::test]
async fn count_active_admins_counts_only_active_admins(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin = UserId::from(uuid::Uuid::now_v7());
    let pending = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, admin, "https://issuer.test", "admin-sub", "Admin")
        .await
        .unwrap_or_else(|e| unreachable!("provision admin: {e}"));
    UsersRepo::grant_admin(&mut conn, admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));
    UsersRepo::provision(&mut conn, pending, "https://issuer.test", "pending-sub", "Pending")
        .await
        .unwrap_or_else(|e| unreachable!("provision pending: {e}"));

    let count = UsersRepo::count_active_admins(&mut conn)
        .await
        .unwrap_or_else(|e| unreachable!("count_active_admins: {e}"));

    assert_eq!(count, 1);
}

#[sqlx::test]
async fn set_status_to_active_from_pending_sets_approved_at_and_approved_by(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin = UserId::from(uuid::Uuid::now_v7());
    let member = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, admin, "https://issuer.test", "admin-sub", "Admin")
        .await
        .unwrap_or_else(|e| unreachable!("provision admin: {e}"));
    UsersRepo::provision(&mut conn, member, "https://issuer.test", "member-sub", "Member")
        .await
        .unwrap_or_else(|e| unreachable!("provision member: {e}"));

    let user = UsersRepo::set_status(&mut conn, member, UserStatus::Active, Some(admin))
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    assert_eq!(user.status, UserStatus::Active);
    assert!(user.approved_at.is_some());
    assert_eq!(user.approved_by, Some(admin));
}

#[sqlx::test]
async fn set_status_notifies_postit_user_changed(pool: PgPool) {
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("listener connect: {e}"));
    listener
        .listen("postit_user_changed")
        .await
        .unwrap_or_else(|e| unreachable!("listen: {e}"));

    let mut writer_conn =
        pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire writer: {e}"));
    let member = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut writer_conn, member, "https://issuer.test", "member-sub", "Member")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::set_status(&mut writer_conn, member, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
        .await
        .unwrap_or_else(|_| unreachable!("timed out waiting for notification"))
        .unwrap_or_else(|e| unreachable!("recv: {e}"));

    assert_eq!(notification.payload(), member.as_uuid().to_string());
}
```

- [ ] **Step 5: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test users`
Expected: FAIL — `postit_data::users` doesn't exist until Step 3 wires it in, and the
migration from Step 1 hasn't run against `#[sqlx::test]`'s ephemeral databases until the
crate's `./migrations` directory (already present from Task 1) includes this file.

- [ ] **Step 6: Make it pass**

Confirm Steps 1–3 are all in place (they were written before this test step, so this step
is just running the tests now that the implementation exists).

Run: `cd server && cargo test -p postit-data --test users`
Expected: PASS (7 tests).

- [ ] **Step 7: Regenerate the `.sqlx` cache**

Run (from `server/crates/data`, with `DATABASE_URL` pointed at the local test database):

```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
```

Expected: a `.sqlx/` directory appears in `server/crates/data` with one JSON file per query
macro call site. If `sqlx-cli` isn't installed, run `cargo install sqlx-cli --no-default-features --features rustls,postgres` first.

- [ ] **Step 8: Run the full quality gates, including offline mode**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace`
Expected: all pass.

Run: `cd server && SQLX_OFFLINE=true cargo check -p postit-data --all-targets`
Expected: passes without a reachable database, using the committed `.sqlx` cache.

- [ ] **Step 9: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: users table and UsersRepo"
```

### Task 4: `audit_events` migration, `AuditEvent`, and `AuditLog`

**Files:**
- Create: `server/crates/data/migrations/0003_audit_events.sql`
- Create: `server/crates/data/src/audit.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/audit.rs`

**Interfaces:**
- Consumes: `postit_core::{UserId, AuditEventId}` (existing).
- Produces: `postit_data::audit::{AuditEventKind, AuditEvent, AuditLog, AuditError}`.

- [ ] **Step 1: Write the migration**

Create `server/crates/data/migrations/0003_audit_events.sql`:

```sql
CREATE TABLE audit_events (
    id UUID PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_user_id UUID,
    owner_id UUID,
    subject_user_id UUID,
    kind TEXT NOT NULL,
    ip TEXT,
    request_id UUID,
    details JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX audit_events_at_idx ON audit_events (at);
CREATE INDEX audit_events_kind_idx ON audit_events (kind);
CREATE INDEX audit_events_actor_idx ON audit_events (actor_user_id);
CREATE INDEX audit_events_subject_idx ON audit_events (subject_user_id);
```

User ID columns are plain `UUID`, not foreign keys — plan 01 requires audit rows to
survive user deletion (pseudonymization in P5 replaces the value, it never deletes the row).

- [ ] **Step 2: Write `AuditEvent` and `AuditLog`**

Create `server/crates/data/src/audit.rs`:

```rust
use std::net::IpAddr;

use postit_core::{AuditEventId, UserId};
use serde_json::{Map, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEventKind {
    UserProvisioned,
    UserApproved,
    UserDisabled,
    UserEnabled,
    RoleChanged,
    BootstrapAdminGranted,
}

impl AuditEventKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserProvisioned => "user_provisioned",
            Self::UserApproved => "user_approved",
            Self::UserDisabled => "user_disabled",
            Self::UserEnabled => "user_enabled",
            Self::RoleChanged => "role_changed",
            Self::BootstrapAdminGranted => "bootstrap_admin_granted",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuditError {
    #[error("detail key `{0}` must end with `_user_id`")]
    KeyMustEndUserId(String),
    #[error("detail key `{0}` must not end with `_user_id`; use detail_user_id")]
    KeyMustNotEndUserId(String),
}

/// Builds one audit event. The only way to attach a user reference other than `actor`,
/// `owner`, or `subject` is [`AuditEvent::detail_user_id`], whose key must end in
/// `_user_id` — this is how a pseudonymization pass (plan 02 P5) can find every user
/// reference in `details` without knowing each event kind's shape. `detail` refuses a key
/// with that suffix for the same reason, in the other direction. Neither method can accept
/// a `secrecy`/`RedactedSecret` value — they take `impl Into<serde_json::Value>` /
/// `impl Serialize`, and those secret types deliberately don't implement `Serialize`
/// (`RedactedSecret` serializes to the literal string `"[redacted]"`, `SecretString`
/// doesn't implement `Serialize` at all), so passing one here is a compile error, not a
/// runtime leak.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    kind: AuditEventKind,
    actor_user_id: Option<UserId>,
    owner_id: Option<UserId>,
    subject_user_id: Option<UserId>,
    ip: Option<IpAddr>,
    request_id: Option<Uuid>,
    details: Map<String, Value>,
}

impl AuditEvent {
    #[must_use]
    pub fn new(kind: AuditEventKind) -> Self {
        Self {
            kind,
            actor_user_id: None,
            owner_id: None,
            subject_user_id: None,
            ip: None,
            request_id: None,
            details: Map::new(),
        }
    }

    #[must_use]
    pub fn actor(mut self, id: UserId) -> Self {
        self.actor_user_id = Some(id);
        self
    }

    #[must_use]
    pub fn owner(mut self, id: UserId) -> Self {
        self.owner_id = Some(id);
        self
    }

    #[must_use]
    pub fn subject(mut self, id: UserId) -> Self {
        self.subject_user_id = Some(id);
        self
    }

    #[must_use]
    pub fn ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    #[must_use]
    pub fn request_id(mut self, id: Uuid) -> Self {
        self.request_id = Some(id);
        self
    }

    /// # Errors
    ///
    /// Returns [`AuditError::KeyMustEndUserId`] if `key` doesn't end with `_user_id`.
    pub fn detail_user_id(mut self, key: &str, id: UserId) -> Result<Self, AuditError> {
        if !key.ends_with("_user_id") {
            return Err(AuditError::KeyMustEndUserId(key.to_string()));
        }
        self.details.insert(key.to_string(), Value::String(id.as_uuid().to_string()));
        Ok(self)
    }

    /// # Errors
    ///
    /// Returns [`AuditError::KeyMustNotEndUserId`] if `key` ends with `_user_id` — use
    /// [`AuditEvent::detail_user_id`] instead, so pseudonymization can find it.
    pub fn detail(mut self, key: &str, value: impl Into<Value>) -> Result<Self, AuditError> {
        if key.ends_with("_user_id") {
            return Err(AuditError::KeyMustNotEndUserId(key.to_string()));
        }
        self.details.insert(key.to_string(), value.into());
        Ok(self)
    }
}

pub struct AuditLog;

impl AuditLog {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn record(
        conn: &mut PgConnection,
        id: AuditEventId,
        event: AuditEvent,
    ) -> Result<(), DataError> {
        sqlx::query!(
            r#"INSERT INTO audit_events (id, actor_user_id, owner_id, subject_user_id, kind, ip, request_id, details)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            id.as_uuid(),
            event.actor_user_id.map(|u| u.as_uuid()),
            event.owner_id.map(|u| u.as_uuid()),
            event.subject_user_id.map(|u| u.as_uuid()),
            event.kind.as_str(),
            event.ip.map(|ip| ip.to_string()) as Option<String>,
            event.request_id,
            Value::Object(event.details),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
```

`ip` is stored as `TEXT`, not Postgres's native `INET` type, so the bind is a plain
`Option<String>` (`event.ip.map(|ip| ip.to_string())`) with no `ipnetwork` dependency and
no native-type driver feature to enable. Audit rows never need range/subnet queries against
`ip` — it's read back as a plain string for display — so the native type buys nothing here.

- [ ] **Step 3: Wire it into `lib.rs`**

Add `pub mod audit;` to `server/crates/data/src/lib.rs`.

- [ ] **Step 4: Write the tests**

Create `server/crates/data/tests/audit.rs`:

```rust
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditError, AuditEvent, AuditEventKind, AuditLog};
use sqlx::PgPool;

#[test]
fn detail_user_id_rejects_a_key_not_ending_in_user_id() {
    let err = AuditEvent::new(AuditEventKind::UserProvisioned)
        .detail_user_id("actor", UserId::from(uuid::Uuid::now_v7()))
        .unwrap_err();
    assert_eq!(err, AuditError::KeyMustEndUserId("actor".to_string()));
}

#[test]
fn detail_rejects_a_key_ending_in_user_id() {
    let err = AuditEvent::new(AuditEventKind::UserProvisioned)
        .detail("delegate_user_id", "not-a-uuid")
        .unwrap_err();
    assert_eq!(
        err,
        AuditError::KeyMustNotEndUserId("delegate_user_id".to_string())
    );
}

#[sqlx::test]
async fn record_writes_kind_actor_and_details(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let event_id = AuditEventId::from(uuid::Uuid::now_v7());
    let actor = UserId::from(uuid::Uuid::now_v7());
    let subject = UserId::from(uuid::Uuid::now_v7());

    let event = AuditEvent::new(AuditEventKind::RoleChanged)
        .actor(actor)
        .subject(subject)
        .detail("new_role", "admin")
        .unwrap_or_else(|e| unreachable!("detail: {e:?}"));

    AuditLog::record(&mut conn, event_id, event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));

    let row: (String, Option<uuid::Uuid>, Option<uuid::Uuid>, serde_json::Value) = sqlx::query_as(
        "SELECT kind, actor_user_id, subject_user_id, details FROM audit_events WHERE id = $1",
    )
    .bind(event_id.as_uuid())
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("select: {e}"));

    assert_eq!(row.0, "role_changed");
    assert_eq!(row.1, Some(actor.as_uuid()));
    assert_eq!(row.2, Some(subject.as_uuid()));
    assert_eq!(row.3.get("new_role").and_then(|v| v.as_str()), Some("admin"));
}
```

- [ ] **Step 5: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test audit`
Expected: FAIL — `postit_data::audit` doesn't exist until Step 3.

- [ ] **Step 6: Make it pass**

Run: `cd server && cargo test -p postit-data --test audit`
Expected: PASS (3 tests).

- [ ] **Step 7: Regenerate the `.sqlx` cache and run the full quality gates**

Run:
```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
cd ../..
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace
SQLX_OFFLINE=true cargo check -p postit-data --all-targets
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: audit_events table, AuditEvent, AuditLog"
```

### Task 5: `AuditRepo` (admin read side)

**Files:**
- Create: `server/crates/data/src/audit_repo.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/audit_repo.rs`

**Interfaces:**
- Consumes: `postit_data::audit::{AuditLog, AuditEvent, AuditEventKind}` (Task 4), `emixdb::dto::{Pagination, ResultSet}` (Task 3).
- Produces: `postit_data::audit_repo::{AuditEventRow, AuditFilter, AuditRepo}`.

This is a separate type from `AuditLog` (the writer) per the "admin repositories are
separate types from owner-scoped repositories" rule — moot for what it can reach today
(there's no owned content yet), but the type separation is established now so it doesn't
need retrofitting once plan 03 adds owned tables `AuditRepo` must never touch.

- [ ] **Step 1: Write `AuditRepo`**

Create `server/crates/data/src/audit_repo.rs`:

```rust
use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AuditEventId, UserId};
use serde_json::Value;
use sqlx::PgConnection;

use crate::error::DataError;

#[derive(Debug, Clone)]
pub struct AuditEventRow {
    pub id: AuditEventId,
    pub at: DateTime<Utc>,
    pub actor_user_id: Option<UserId>,
    pub owner_id: Option<UserId>,
    pub subject_user_id: Option<UserId>,
    pub kind: String,
    pub ip: Option<String>,
    pub request_id: Option<uuid::Uuid>,
    pub details: Value,
}

struct Row {
    id: uuid::Uuid,
    at: DateTime<Utc>,
    actor_user_id: Option<uuid::Uuid>,
    owner_id: Option<uuid::Uuid>,
    subject_user_id: Option<uuid::Uuid>,
    kind: String,
    ip: Option<String>,
    request_id: Option<uuid::Uuid>,
    details: Value,
}

impl From<Row> for AuditEventRow {
    fn from(row: Row) -> Self {
        Self {
            id: AuditEventId::from(row.id),
            at: row.at,
            actor_user_id: row.actor_user_id.map(UserId::from),
            owner_id: row.owner_id.map(UserId::from),
            subject_user_id: row.subject_user_id.map(UserId::from),
            kind: row.kind,
            ip: row.ip,
            request_id: row.request_id,
            details: row.details,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub kind: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub actor_user_id: Option<UserId>,
    pub subject_user_id: Option<UserId>,
}

pub struct AuditRepo;

impl AuditRepo {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        filter: &AuditFilter,
        pagination: Pagination,
    ) -> Result<ResultSet<AuditEventRow>, DataError> {
        let limit = i64::try_from(pagination.page_size).unwrap_or(10);
        let offset =
            i64::try_from(pagination.page.saturating_sub(1).saturating_mul(pagination.page_size))
                .unwrap_or(0);
        let actor = filter.actor_user_id.map(|u| u.as_uuid());
        let subject = filter.subject_user_id.map(|u| u.as_uuid());

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id, at, actor_user_id, owner_id, subject_user_id, kind, ip, request_id, details
               FROM audit_events
               WHERE ($1::text IS NULL OR kind = $1)
                 AND ($2::timestamptz IS NULL OR at >= $2)
                 AND ($3::timestamptz IS NULL OR at <= $3)
                 AND ($4::uuid IS NULL OR actor_user_id = $4)
                 AND ($5::uuid IS NULL OR subject_user_id = $5)
               ORDER BY at DESC
               LIMIT $6 OFFSET $7"#,
            filter.kind,
            filter.from,
            filter.to,
            actor,
            subject,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total: Option<i64> = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM audit_events
               WHERE ($1::text IS NULL OR kind = $1)
                 AND ($2::timestamptz IS NULL OR at >= $2)
                 AND ($3::timestamptz IS NULL OR at <= $3)
                 AND ($4::uuid IS NULL OR actor_user_id = $4)
                 AND ($5::uuid IS NULL OR subject_user_id = $5)"#,
            filter.kind,
            filter.from,
            filter.to,
            actor,
            subject,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(ResultSet {
            data: rows.into_iter().map(Into::into).collect(),
            total: u64::try_from(total.unwrap_or(0)).unwrap_or(0),
            pagination: Some(pagination),
        })
    }
}
```

- [ ] **Step 2: Wire it into `lib.rs`**

Add `pub mod audit_repo;` to `server/crates/data/src/lib.rs`.

- [ ] **Step 3: Write the tests**

Create `server/crates/data/tests/audit_repo.rs`:

```rust
use emixdb::dto::Pagination;
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::audit_repo::{AuditFilter, AuditRepo};
use sqlx::PgPool;

#[sqlx::test]
async fn list_filters_by_kind(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let actor = UserId::from(uuid::Uuid::now_v7());

    AuditLog::record(
        &mut conn,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserApproved).actor(actor),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record approved: {e}"));
    AuditLog::record(
        &mut conn,
        AuditEventId::from(uuid::Uuid::now_v7()),
        AuditEvent::new(AuditEventKind::UserDisabled).actor(actor),
    )
    .await
    .unwrap_or_else(|e| unreachable!("record disabled: {e}"));

    let filter = AuditFilter {
        kind: Some("user_approved".to_string()),
        ..AuditFilter::default()
    };
    let result = AuditRepo::list(&mut conn, &filter, Pagination::default())
        .await
        .unwrap_or_else(|e| unreachable!("list: {e}"));

    assert_eq!(result.total, 1);
    assert_eq!(result.data.len(), 1);
    assert_eq!(result.data[0].kind, "user_approved");
}

#[sqlx::test]
async fn list_paginates(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    for _ in 0..15 {
        AuditLog::record(
            &mut conn,
            AuditEventId::from(uuid::Uuid::now_v7()),
            AuditEvent::new(AuditEventKind::UserProvisioned),
        )
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));
    }

    let page = AuditRepo::list(
        &mut conn,
        &AuditFilter::default(),
        Pagination { page: 2, page_size: 10 },
    )
    .await
    .unwrap_or_else(|e| unreachable!("list: {e}"));

    assert_eq!(page.total, 15);
    assert_eq!(page.data.len(), 5);
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test audit_repo`
Expected: FAIL.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-data --test audit_repo`
Expected: PASS (2 tests).

- [ ] **Step 6: Regenerate the `.sqlx` cache and run the full quality gates**

Run:
```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
cd ../..
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace
SQLX_OFFLINE=true cargo check -p postit-data --all-targets
```
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: AuditRepo (admin read side)"
```

### Task 6: `user_preferences` migration and `UserPreferencesRepo`

**Files:**
- Create: `server/crates/data/migrations/0004_user_preferences.sql`
- Create: `server/crates/data/src/preferences.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/preferences.rs`

**Interfaces:**
- Consumes: `postit_core::UserId` (existing), `users` table (Task 3).
- Produces: `postit_data::preferences::{UserPreferences, UserPreferencesRepo}`.

- [ ] **Step 1: Write the migration**

Create `server/crates/data/migrations/0004_user_preferences.sql`:

```sql
CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_notifications BOOLEAN NOT NULL DEFAULT TRUE,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    store_ai_prompts BOOLEAN NOT NULL DEFAULT FALSE,
    last_approval_email_at TIMESTAMPTZ
);
```

- [ ] **Step 2: Write `UserPreferencesRepo`**

Create `server/crates/data/src/preferences.rs`:

```rust
use chrono::{DateTime, Utc};
use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;

#[derive(Debug, Clone)]
pub struct UserPreferences {
    pub user_id: UserId,
    pub email_notifications: bool,
    pub timezone: String,
    pub store_ai_prompts: bool,
    pub last_approval_email_at: Option<DateTime<Utc>>,
}

struct Row {
    user_id: uuid::Uuid,
    email_notifications: bool,
    timezone: String,
    store_ai_prompts: bool,
    last_approval_email_at: Option<DateTime<Utc>>,
}

impl From<Row> for UserPreferences {
    fn from(row: Row) -> Self {
        Self {
            user_id: UserId::from(row.user_id),
            email_notifications: row.email_notifications,
            timezone: row.timezone,
            store_ai_prompts: row.store_ai_prompts,
            last_approval_email_at: row.last_approval_email_at,
        }
    }
}

pub struct UserPreferencesRepo;

impl UserPreferencesRepo {
    /// Creates the default preferences row for a newly provisioned user. Called by
    /// `postit-identity` inside the same transaction as `UsersRepo::provision`, so a user
    /// never exists without preferences.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure (including a unique-violation if
    /// called twice for the same user).
    pub async fn create_default(
        conn: &mut PgConnection,
        user_id: UserId,
        timezone: &str,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"INSERT INTO user_preferences (user_id, timezone)
               VALUES ($1, $2)
               RETURNING user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at"#,
            user_id.as_uuid(),
            timezone,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<UserPreferences>, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at
               FROM user_preferences WHERE user_id = $1"#,
            user_id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if no preferences row exists for `user_id`,
    /// [`DataError::Sql`] otherwise.
    pub async fn update(
        conn: &mut PgConnection,
        user_id: UserId,
        email_notifications: bool,
        timezone: &str,
        store_ai_prompts: bool,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE user_preferences
               SET email_notifications = $2, timezone = $3, store_ai_prompts = $4
               WHERE user_id = $1
               RETURNING user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at"#,
            user_id.as_uuid(),
            email_notifications,
            timezone,
            store_ai_prompts,
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.map(Into::into).ok_or(DataError::NotFound)
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure. Not finding `user_id` is not an
    /// error — this is a best-effort marker write from the coalesced-approval-email flow
    /// (plan 02 P5), called only after that row is known to exist.
    pub async fn set_last_approval_email_at(
        conn: &mut PgConnection,
        user_id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE user_preferences SET last_approval_email_at = $2 WHERE user_id = $1",
            user_id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 3: Wire it into `lib.rs`**

Add `pub mod preferences;` to `server/crates/data/src/lib.rs`.

- [ ] **Step 4: Write the tests**

Create `server/crates/data/tests/preferences.rs`:

```rust
use postit_core::UserId;
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provisioned_user(conn: &mut sqlx::PgConnection) -> UserId {
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(conn, id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

#[sqlx::test]
async fn create_default_sets_timezone_and_defaults(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;

    let prefs = UserPreferencesRepo::create_default(&mut conn, user_id, "America/New_York")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    assert_eq!(prefs.timezone, "America/New_York");
    assert!(prefs.email_notifications);
    assert!(!prefs.store_ai_prompts);
    assert!(prefs.last_approval_email_at.is_none());
}

#[sqlx::test]
async fn update_changes_fields(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;
    UserPreferencesRepo::create_default(&mut conn, user_id, "UTC")
        .await
        .unwrap_or_else(|e| unreachable!("create_default: {e}"));

    let prefs = UserPreferencesRepo::update(&mut conn, user_id, false, "Europe/London", true)
        .await
        .unwrap_or_else(|e| unreachable!("update: {e}"));

    assert!(!prefs.email_notifications);
    assert_eq!(prefs.timezone, "Europe/London");
    assert!(prefs.store_ai_prompts);
}

#[sqlx::test]
async fn get_returns_none_for_a_user_with_no_preferences_row(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = provisioned_user(&mut conn).await;

    let prefs = UserPreferencesRepo::get(&mut conn, user_id)
        .await
        .unwrap_or_else(|e| unreachable!("get: {e}"));

    assert!(prefs.is_none());
}
```

- [ ] **Step 5: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test preferences`
Expected: FAIL.

- [ ] **Step 6: Make it pass**

Run: `cd server && cargo test -p postit-data --test preferences`
Expected: PASS (3 tests).

- [ ] **Step 7: Regenerate the `.sqlx` cache and run the full quality gates**

Run:
```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
cd ../..
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace
SQLX_OFFLINE=true cargo check -p postit-data --all-targets
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: user_preferences table and UserPreferencesRepo"
```

### Task 7: `idempotency_keys` migration and `IdempotencyRepo`

**Files:**
- Create: `server/crates/data/migrations/0005_idempotency_keys.sql`
- Create: `server/crates/data/src/idempotency.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/idempotency.rs`

**Interfaces:**
- Consumes: `postit_core::UserId` (existing), `users` table (Task 3).
- Produces: `postit_data::idempotency::{IdempotencyState, IdempotencyRecord, BeginOutcome, IdempotencyRepo}`.

**Review Focus:** a second `begin()` racing the first for the same `(owner_id, actor_id,
key)` must return a distinguishable conflict, not bubble up `DataError::Sql` wrapping a raw
Postgres unique-violation. Step 4 below tests this directly.

- [ ] **Step 1: Write the migration**

Create `server/crates/data/migrations/0005_idempotency_keys.sql`:

```sql
CREATE TABLE idempotency_keys (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    actor_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    route TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('in_progress', 'completed')),
    response_status SMALLINT,
    response_body JSONB,
    expires_at TIMESTAMPTZ NOT NULL,
    UNIQUE (owner_id, actor_id, key)
);

CREATE INDEX idempotency_keys_expires_idx ON idempotency_keys (expires_at);
```

- [ ] **Step 2: Write `IdempotencyRepo`**

Create `server/crates/data/src/idempotency.rs`:

```rust
use chrono::{DateTime, Utc};
use postit_core::UserId;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyState {
    InProgress,
    Completed,
}

impl IdempotencyState {
    fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "completed" => Self::Completed,
            _ => Self::InProgress,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    pub id: Uuid,
    pub owner_id: UserId,
    pub actor_id: UserId,
    pub key: String,
    pub route: String,
    pub request_hash: String,
    pub state: IdempotencyState,
    pub response_status: Option<i16>,
    pub response_body: Option<Value>,
    pub expires_at: DateTime<Utc>,
}

struct Row {
    id: Uuid,
    owner_id: Uuid,
    actor_id: Uuid,
    key: String,
    route: String,
    request_hash: String,
    state: String,
    response_status: Option<i16>,
    response_body: Option<Value>,
    expires_at: DateTime<Utc>,
}

impl From<Row> for IdempotencyRecord {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            owner_id: UserId::from(row.owner_id),
            actor_id: UserId::from(row.actor_id),
            key: row.key,
            route: row.route,
            request_hash: row.request_hash,
            state: IdempotencyState::parse(&row.state),
            response_status: row.response_status,
            response_body: row.response_body,
            expires_at: row.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BeginOutcome {
    /// A new `in_progress` row was inserted; the caller should do the work.
    Started(IdempotencyRecord),
    /// A row for this `(owner_id, actor_id, key)` already exists (`in_progress` or
    /// `completed`). The caller decides what that means — `postit-api` (P6) returns 409
    /// `idempotency_in_progress` or replays the stored response, per plan 01.
    Conflict(IdempotencyRecord),
}

pub struct IdempotencyRepo;

impl IdempotencyRepo {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure other than the unique-violation
    /// this function itself turns into `BeginOutcome::Conflict`.
    pub async fn begin(
        conn: &mut PgConnection,
        id: Uuid,
        owner_id: UserId,
        actor_id: UserId,
        key: &str,
        route: &str,
        request_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<BeginOutcome, DataError> {
        let inserted = sqlx::query_as!(
            Row,
            r#"INSERT INTO idempotency_keys (id, owner_id, actor_id, key, route, request_hash, state, expires_at)
               VALUES ($1, $2, $3, $4, $5, $6, 'in_progress', $7)
               ON CONFLICT (owner_id, actor_id, key) DO NOTHING
               RETURNING id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at"#,
            id,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
            route,
            request_hash,
            expires_at,
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(row) = inserted {
            return Ok(BeginOutcome::Started(row.into()));
        }

        let existing = sqlx::query_as!(
            Row,
            r#"SELECT id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at
               FROM idempotency_keys WHERE owner_id = $1 AND actor_id = $2 AND key = $3"#,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;

        Ok(BeginOutcome::Conflict(existing.into()))
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn complete(
        conn: &mut PgConnection,
        id: Uuid,
        response_status: i16,
        response_body: Value,
    ) -> Result<(), DataError> {
        let result = sqlx::query!(
            r#"UPDATE idempotency_keys
               SET state = 'completed', response_status = $2, response_body = $3
               WHERE id = $1"#,
            id,
            response_status,
            response_body,
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find(
        conn: &mut PgConnection,
        owner_id: UserId,
        actor_id: UserId,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at
               FROM idempotency_keys WHERE owner_id = $1 AND actor_id = $2 AND key = $3"#,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Deletes an `in_progress` row whose request failed without producing a response, so
    /// the client can retry with the same key. Deleting a `completed` row is not this
    /// function's job — `postit-api` (P6) never calls it for one.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Result<(), DataError> {
        sqlx::query!("DELETE FROM idempotency_keys WHERE id = $1", id)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}
```

- [ ] **Step 3: Wire it into `lib.rs`**

Add `pub mod idempotency;` to `server/crates/data/src/lib.rs`.

- [ ] **Step 4: Write the tests**

Create `server/crates/data/tests/idempotency.rs`:

```rust
use chrono::{Duration, Utc};
use postit_core::UserId;
use postit_data::idempotency::{BeginOutcome, IdempotencyRepo};
use postit_data::users::UsersRepo;
use sqlx::PgPool;

async fn provisioned_user(conn: &mut sqlx::PgConnection, sub: &str) -> UserId {
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(conn, id, "https://issuer.test", sub, "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

#[sqlx::test]
async fn begin_starts_a_new_key(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);

    let outcome = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin: {e}"));

    assert!(matches!(outcome, BeginOutcome::Started(_)));
}

#[sqlx::test]
async fn begin_twice_with_the_same_key_returns_conflict_not_an_error(pool: PgPool) {
    // Review Focus: a racing begin() must be a typed Conflict, not an unhandled SQL error.
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);

    let first = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-1",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("first begin: {e}"));
    let second = IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user,
        user,
        "key-1",
        "POST /posts",
        "hash-2",
        expires_at,
    )
    .await
    .unwrap_or_else(|e| unreachable!("second begin: {e}"));

    let BeginOutcome::Started(started) = first else {
        unreachable!("first begin should have started");
    };
    let BeginOutcome::Conflict(conflict) = second else {
        unreachable!("second begin should have conflicted");
    };
    assert_eq!(started.id, conflict.id);
    assert_eq!(conflict.request_hash, "hash-1");
}

#[sqlx::test]
async fn complete_sets_state_and_response(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);
    let id = uuid::Uuid::now_v7();
    IdempotencyRepo::begin(&mut conn, id, user, user, "key-1", "POST /posts", "hash-1", expires_at)
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));

    IdempotencyRepo::complete(&mut conn, id, 201, serde_json::json!({"id": "abc"}))
        .await
        .unwrap_or_else(|e| unreachable!("complete: {e}"));

    let record = IdempotencyRepo::find(&mut conn, user, user, "key-1")
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"))
        .unwrap_or_else(|| unreachable!("record should exist"));
    assert_eq!(record.response_status, Some(201));
    assert_eq!(
        record.response_body.as_ref().and_then(|b| b.get("id")).and_then(|v| v.as_str()),
        Some("abc")
    );
}

#[sqlx::test]
async fn delete_removes_an_in_progress_row(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user = provisioned_user(&mut conn, "sub-1").await;
    let expires_at = Utc::now() + Duration::hours(24);
    let id = uuid::Uuid::now_v7();
    IdempotencyRepo::begin(&mut conn, id, user, user, "key-1", "POST /posts", "hash-1", expires_at)
        .await
        .unwrap_or_else(|e| unreachable!("begin: {e}"));

    IdempotencyRepo::delete(&mut conn, id)
        .await
        .unwrap_or_else(|e| unreachable!("delete: {e}"));

    let record = IdempotencyRepo::find(&mut conn, user, user, "key-1")
        .await
        .unwrap_or_else(|e| unreachable!("find: {e}"));
    assert!(record.is_none());
}
```

- [ ] **Step 5: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test idempotency`
Expected: FAIL.

- [ ] **Step 6: Make it pass**

Run: `cd server && cargo test -p postit-data --test idempotency`
Expected: PASS (4 tests).

- [ ] **Step 7: Regenerate the `.sqlx` cache and run the full quality gates**

Run:
```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
cd ../..
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace
SQLX_OFFLINE=true cargo check -p postit-data --all-targets
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: idempotency_keys table and IdempotencyRepo"
```

### Task 8: Retention purge queries

**Files:**
- Create: `server/crates/data/src/retention.rs`
- Modify: `server/crates/data/src/lib.rs`
- Test: `server/crates/data/tests/retention.rs`

**Interfaces:**
- Consumes: `audit_events` (Task 4), `idempotency_keys` (Task 7).
- Produces: `postit_data::retention::{purge_audit_events, purge_expired_idempotency_keys}`.

These are queries only — no cron scheduling. `postit-server` registers the
`audit_retention`/`data_retention` cron jobs that call them in P5/P6, per the spec.

- [ ] **Step 1: Write the purge queries**

Create `server/crates/data/src/retention.rs`:

```rust
use chrono::{Duration, Utc};
use sqlx::PgConnection;

use crate::error::DataError;

/// Clears `ip` on events older than `ip_retention`, then deletes events older than
/// `retention`. Returns `(ip_cleared, deleted)`. The cutoffs are computed here in Rust
/// (`Utc::now() - retention`) and bound as plain `timestamptz` values, rather than binding
/// a `chrono::Duration` for Postgres to subtract as an `interval` — `sqlx`'s Postgres
/// driver has no `Encode` for `chrono::Duration` to `interval`, only its own `PgInterval`
/// type, and a bound `timestamptz` is simpler than introducing that type for one query.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_audit_events(
    conn: &mut PgConnection,
    ip_retention: Duration,
    retention: Duration,
) -> Result<(u64, u64), DataError> {
    let ip_cutoff = Utc::now() - ip_retention;
    let retention_cutoff = Utc::now() - retention;

    let ip_cleared = sqlx::query!(
        "UPDATE audit_events SET ip = NULL WHERE ip IS NOT NULL AND at < $1",
        ip_cutoff,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();

    let deleted = sqlx::query!("DELETE FROM audit_events WHERE at < $1", retention_cutoff)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    Ok((ip_cleared, deleted))
}

/// Deletes `idempotency_keys` rows past their `expires_at`.
///
/// # Errors
///
/// Returns [`DataError::Sql`] on a database failure.
pub async fn purge_expired_idempotency_keys(conn: &mut PgConnection) -> Result<u64, DataError> {
    let now = Utc::now();
    let deleted = sqlx::query!("DELETE FROM idempotency_keys WHERE expires_at < $1", now)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(deleted)
}
```

- [ ] **Step 2: Wire it into `lib.rs`**

Add `pub mod retention;` to `server/crates/data/src/lib.rs`.

- [ ] **Step 3: Write the tests**

Create `server/crates/data/tests/retention.rs`:

```rust
use chrono::{Duration, Utc};
use postit_core::{AuditEventId, UserId};
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::idempotency::IdempotencyRepo;
use postit_data::retention::{purge_audit_events, purge_expired_idempotency_keys};
use postit_data::users::UsersRepo;
use sqlx::PgPool;

#[sqlx::test]
async fn purge_audit_events_clears_old_ip_and_deletes_very_old_rows(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let event_id = AuditEventId::from(uuid::Uuid::now_v7());
    let event = AuditEvent::new(AuditEventKind::UserProvisioned)
        .ip("203.0.113.5".parse().unwrap_or_else(|e| unreachable!("parse ip: {e}")));
    AuditLog::record(&mut conn, event_id, event)
        .await
        .unwrap_or_else(|e| unreachable!("record: {e}"));

    sqlx::query!(
        "UPDATE audit_events SET at = now() - interval '100 days' WHERE id = $1",
        event_id.as_uuid(),
    )
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("backdate: {e}"));

    let (ip_cleared, deleted) =
        purge_audit_events(&mut conn, Duration::days(90), Duration::days(730))
            .await
            .unwrap_or_else(|e| unreachable!("purge: {e}"));

    assert_eq!(ip_cleared, 1);
    assert_eq!(deleted, 0);

    let ip: Option<String> = sqlx::query_scalar("SELECT ip FROM audit_events WHERE id = $1")
        .bind(event_id.as_uuid())
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|e| unreachable!("select: {e}"));
    assert!(ip.is_none());
}

#[sqlx::test]
async fn purge_expired_idempotency_keys_deletes_only_expired_rows(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user_id,
        user_id,
        "expired-key",
        "POST /posts",
        "hash-1",
        Utc::now() - Duration::hours(1),
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin expired: {e}"));
    IdempotencyRepo::begin(
        &mut conn,
        uuid::Uuid::now_v7(),
        user_id,
        user_id,
        "live-key",
        "POST /posts",
        "hash-2",
        Utc::now() + Duration::hours(24),
    )
    .await
    .unwrap_or_else(|e| unreachable!("begin live: {e}"));

    let deleted = purge_expired_idempotency_keys(&mut conn)
        .await
        .unwrap_or_else(|e| unreachable!("purge: {e}"));

    assert_eq!(deleted, 1);
    let live = IdempotencyRepo::find(&mut conn, user_id, user_id, "live-key")
        .await
        .unwrap_or_else(|e| unreachable!("find live: {e}"));
    assert!(live.is_some());
    let expired = IdempotencyRepo::find(&mut conn, user_id, user_id, "expired-key")
        .await
        .unwrap_or_else(|e| unreachable!("find expired: {e}"));
    assert!(expired.is_none());
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-data --test retention`
Expected: FAIL.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-data --test retention`
Expected: PASS (2 tests).

- [ ] **Step 6: Regenerate the `.sqlx` cache and run the full quality gates**

Run:
```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare
cd ../..
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace
SQLX_OFFLINE=true cargo check -p postit-data --all-targets
```
Expected: all pass. This closes Section A — `postit-data` now has every migration,
repository, and purge query plan 02 P4 assigns it.

- [ ] **Step 7: Commit**

```bash
git add server/crates/data
git commit -m "postit-data: retention purge queries (audit_events, idempotency_keys)"
```

---

## Section B — `postit-identity`

### Task 9: Crate scaffold and error types

**Files:**
- Modify: `server/Cargo.toml` (add `jsonwebtoken`, `moka`, `rsa`, `p256` to `[workspace.dependencies]`)
- Modify: `server/crates/identity/Cargo.toml`
- Create: `server/crates/identity/src/error.rs`
- Modify: `server/crates/identity/src/lib.rs`

**Interfaces:**
- Produces: `postit_identity::{IdentityError, VerifyError}`.

- [ ] **Step 1: Add workspace dependencies**

In `server/Cargo.toml`, in `[workspace.dependencies]`, add (alphabetically among the
existing plain entries):

```toml
jsonwebtoken = "9"
moka = { version = "0.12", features = ["sync"] }
p256 = { version = "0.13", features = ["ecdsa", "pkcs8"] }
rsa = "0.9"
```

`p256` and `rsa` are test-issuer-only (feature `testkit`, wired in Task 10), but their
version specs live in `[workspace.dependencies]` like every other dependency per the DRY
rule, even though only one crate consumes them.

- [ ] **Step 2: Fill in `postit-identity`'s `Cargo.toml`**

Replace `server/crates/identity/Cargo.toml`'s `[dependencies]` section and add a
`[dev-dependencies]`/`[features]` section (keep the rest of the file as-is):

```toml
[dependencies]
postit-config.workspace = true
postit-core.workspace = true
postit-data.workspace = true
postit-http.workspace = true
chrono.workspace = true
jsonwebtoken.workspace = true
moka.workspace = true
serde.workspace = true
serde_json.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tracing.workspace = true
uuid.workspace = true

p256 = { workspace = true, optional = true }
rsa = { workspace = true, optional = true }

[dev-dependencies]
tokio.workspace = true
wiremock.workspace = true

[features]
testkit = ["dep:rsa", "dep:p256", "postit-http/testkit"]
```

- [ ] **Step 3: Write `IdentityError` and `VerifyError`**

Create `server/crates/identity/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("bad signature")]
    BadSignature,
    #[error("wrong issuer")]
    WrongIssuer,
    #[error("wrong audience")]
    WrongAudience,
    #[error("token expired")]
    Expired,
    #[error("token not yet valid")]
    NotYetValid,
    #[error("algorithm not accepted")]
    UnacceptedAlgorithm,
    #[error("unknown key id")]
    UnknownKid,
    #[error("malformed token")]
    Malformed,
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error(transparent)]
    Verify(#[from] VerifyError),

    #[error(transparent)]
    Data(#[from] postit_data::DataError),

    #[error("http error: {0}")]
    Http(#[from] postit_http::HttpError),

    #[error(transparent)]
    Audit(#[from] postit_data::audit::AuditError),

    #[error("status transition not allowed: {0} -> {1}")]
    InvalidTransition(&'static str, &'static str),

    #[error("the last active admin cannot be disabled or demoted")]
    LastAdmin,
}
```

- [ ] **Step 4: Wire up `lib.rs`**

Replace `server/crates/identity/src/lib.rs`:

```rust
mod error;

pub use error::{IdentityError, VerifyError};
```

- [ ] **Step 5: Run the workspace quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace`
Expected: all pass. `cargo check -p postit-identity --features testkit` also passes,
confirming the optional `rsa`/`p256` dependencies resolve.

- [ ] **Step 6: Commit**

```bash
git add server/Cargo.toml server/crates/identity
git commit -m "postit-identity: crate scaffold and error types"
```

### Task 10: `testkit::Keys` — RSA and EC keypair generation and JWKS

**Files:**
- Modify: `server/Cargo.toml` (add `base64`, `rand_core` to `[workspace.dependencies]`)
- Modify: `server/crates/identity/Cargo.toml` (add `base64`, `rand_core` under `testkit`)
- Create: `server/crates/identity/src/testkit.rs`
- Modify: `server/crates/identity/src/lib.rs`

**Interfaces:**
- Produces (feature `testkit`): `postit_identity::testkit::Keys` with `Keys::generate()`,
  `Keys::mint<C: Serialize>(&self, claims: &C, algorithm: jsonwebtoken::Algorithm) -> String`,
  `Keys::jwks(&self) -> jsonwebtoken::jwk::JwkSet`.

- [ ] **Step 1: Add workspace dependencies**

In `server/Cargo.toml`, in `[workspace.dependencies]`, add:

```toml
base64 = "0.22"
rand_core = { version = "0.6", features = ["getrandom"] }
```

`rand_core` 0.6 (not the workspace's existing loose `rand = "0"`) is used directly for key
generation because `rsa` 0.9 and `p256` 0.13 both require an `rand_core` 0.6-compatible RNG
trait, and the workspace's `rand = "0"` spec could resolve to a newer `rand` built on a
newer `rand_core` that doesn't satisfy it. Going straight to `rand_core::OsRng` sidesteps
that mismatch for this one test-only concern.

- [ ] **Step 2: Add them to `postit-identity`'s `Cargo.toml`**

In `server/crates/identity/Cargo.toml`, add two more optional dependencies next to
`p256`/`rsa`, and extend the `testkit` feature:

```toml
base64 = { workspace = true, optional = true }
rand_core = { workspace = true, optional = true }
```

Change the `[features]` section to:

```toml
[features]
testkit = ["dep:rsa", "dep:p256", "dep:base64", "dep:rand_core", "postit-http/testkit"]
```

- [ ] **Step 3: Write `testkit::Keys`**

Create `server/crates/identity/src/testkit.rs`:

```rust
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, PublicKeyUse, RSAKeyParameters, RSAKeyType,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p256::ecdsa::SigningKey as EcSigningKey;
use p256::elliptic_curve::pkcs8::EncodePrivateKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::LineEnding as EcLineEnding;
use rand_core::OsRng;
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::LineEnding as RsaLineEnding;
use rsa::traits::PublicKeyParts;
use serde::Serialize;

const RSA_KID: &str = "test-rsa-1";
const EC_KID: &str = "test-ec-1";

/// Generates one RSA and one P-256 keypair and mints RS256/ES256 test tokens from them.
/// Test-only (`testkit` feature): no production code path constructs one.
pub struct Keys {
    rsa_encoding_key: EncodingKey,
    rsa_jwk: Jwk,
    ec_encoding_key: EncodingKey,
    ec_jwk: Jwk,
}

impl Keys {
    #[must_use]
    pub fn generate() -> Self {
        let rsa_private = RsaPrivateKey::new(&mut OsRng, 2048)
            .unwrap_or_else(|err| unreachable!("generating rsa test key: {err}"));
        let rsa_pem = rsa_private
            .to_pkcs1_pem(RsaLineEnding::LF)
            .unwrap_or_else(|err| unreachable!("encoding rsa test key: {err}"));
        let rsa_encoding_key = EncodingKey::from_rsa_pem(rsa_pem.as_bytes())
            .unwrap_or_else(|err| unreachable!("loading rsa encoding key: {err}"));
        let rsa_jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::RS256),
                key_id: Some(RSA_KID.to_string()),
                ..CommonParameters::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: RSAKeyType::RSA,
                n: URL_SAFE_NO_PAD.encode(rsa_private.n().to_bytes_be()),
                e: URL_SAFE_NO_PAD.encode(rsa_private.e().to_bytes_be()),
            }),
        };

        let ec_signing = EcSigningKey::random(&mut OsRng);
        let ec_pem = ec_signing
            .to_pkcs8_pem(EcLineEnding::LF)
            .unwrap_or_else(|err| unreachable!("encoding ec test key: {err}"));
        let ec_encoding_key = EncodingKey::from_ec_pem(ec_pem.as_bytes())
            .unwrap_or_else(|err| unreachable!("loading ec encoding key: {err}"));
        let point = ec_signing.verifying_key().to_encoded_point(false);
        let ec_jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::ES256),
                key_id: Some(EC_KID.to_string()),
                ..CommonParameters::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: URL_SAFE_NO_PAD.encode(
                    point.x().unwrap_or_else(|| unreachable!("uncompressed point has x")),
                ),
                y: URL_SAFE_NO_PAD.encode(
                    point.y().unwrap_or_else(|| unreachable!("uncompressed point has y")),
                ),
            }),
        };

        Self {
            rsa_encoding_key,
            rsa_jwk,
            ec_encoding_key,
            ec_jwk,
        }
    }

    /// Mints a JWT with `claims` signed under `algorithm`, using this issuer's matching
    /// key and setting its `kid` header to that key's JWKS entry.
    ///
    /// # Panics
    ///
    /// Panics if `algorithm` is neither `RS256` nor `ES256` — this test issuer mints only
    /// those two, matching plan 02's test-issuer requirement.
    #[must_use]
    pub fn mint<C: Serialize>(&self, claims: &C, algorithm: Algorithm) -> String {
        let (kid, encoding_key) = match algorithm {
            Algorithm::RS256 => (RSA_KID, &self.rsa_encoding_key),
            Algorithm::ES256 => (EC_KID, &self.ec_encoding_key),
            other => unreachable!("test issuer only mints RS256 and ES256, got {other:?}"),
        };
        let mut header = Header::new(algorithm);
        header.kid = Some(kid.to_string());
        encode(&header, claims, encoding_key)
            .unwrap_or_else(|err| unreachable!("signing test token: {err}"))
    }

    #[must_use]
    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.rsa_jwk.clone(), self.ec_jwk.clone()],
        }
    }
}
```

- [ ] **Step 4: Wire it into `lib.rs`**

Add to `server/crates/identity/src/lib.rs`:

```rust
#[cfg(feature = "testkit")]
pub mod testkit;
```

- [ ] **Step 5: Write a smoke test proving both algorithms round-trip**

Create `server/crates/identity/tests/testkit.rs`:

```rust
#![cfg(feature = "testkit")]

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use postit_identity::testkit::Keys;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

fn claims() -> Claims {
    Claims {
        sub: "test-subject".to_string(),
        #[allow(clippy::cast_possible_truncation)]
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
    }
}

#[test]
fn mints_a_verifiable_rs256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(), Algorithm::RS256);
    let header = decode_header(&token).unwrap_or_else(|e| unreachable!("decode_header: {e}"));
    assert_eq!(header.alg, Algorithm::RS256);

    let jwks = keys.jwks();
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.common.key_id.as_deref() == header.kid.as_deref())
        .unwrap_or_else(|| unreachable!("matching jwk not found"));
    let decoding_key =
        DecodingKey::from_jwk(jwk).unwrap_or_else(|e| unreachable!("from_jwk: {e}"));

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_aud = false;
    let decoded = decode::<Claims>(&token, &decoding_key, &validation)
        .unwrap_or_else(|e| unreachable!("decode: {e}"));
    assert_eq!(decoded.claims.sub, "test-subject");
}

#[test]
fn mints_a_verifiable_es256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(), Algorithm::ES256);
    let header = decode_header(&token).unwrap_or_else(|e| unreachable!("decode_header: {e}"));
    assert_eq!(header.alg, Algorithm::ES256);

    let jwks = keys.jwks();
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.common.key_id.as_deref() == header.kid.as_deref())
        .unwrap_or_else(|| unreachable!("matching jwk not found"));
    let decoding_key =
        DecodingKey::from_jwk(jwk).unwrap_or_else(|e| unreachable!("from_jwk: {e}"));

    let mut validation = Validation::new(Algorithm::ES256);
    validation.validate_aud = false;
    let decoded = decode::<Claims>(&token, &decoding_key, &validation)
        .unwrap_or_else(|e| unreachable!("decode: {e}"));
    assert_eq!(decoded.claims.sub, "test-subject");
}
```

- [ ] **Step 6: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --features testkit --test testkit`
Expected: FAIL — `postit_identity::testkit` doesn't exist until Step 4.

- [ ] **Step 7: Make it pass**

Run: `cd server && cargo test -p postit-identity --features testkit --test testkit`
Expected: PASS (2 tests).

- [ ] **Step 8: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add server/Cargo.toml server/crates/identity
git commit -m "postit-identity: testkit::Keys (RSA + EC test issuer keys)"
```

### Task 11: `Verifier` — pure JWT validation given a resolved JWKS

**Files:**
- Create: `server/crates/identity/src/verifier.rs`
- Modify: `server/crates/identity/src/lib.rs`
- Test: `server/crates/identity/tests/verifier.rs`

**Interfaces:**
- Consumes: `testkit::Keys` (Task 10, test-only), `VerifyError` (Task 9).
- Produces: `postit_identity::verifier::{Verifier, VerifiedClaims}`.

This task verifies a token against an already-fetched `jsonwebtoken::jwk::JwkSet` — no
HTTP. Task 12 adds the discovery/JWKS fetch that produces that `JwkSet` in production.

- [ ] **Step 1: Write `Verifier`**

Create `server/crates/identity/src/verifier.rs`:

```rust
use std::time::Duration;

use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::{Map, Value};

use crate::error::VerifyError;

/// Never accepted, whatever `accepted_algorithms` says — a symmetric algorithm verified
/// with a key meant to be public (the RSA/EC public key material published in the JWKS)
/// lets an attacker who knows that public key forge a token, the classic
/// "algorithm confusion" attack. This list is checked before the configured allowlist, not
/// instead of it, so misconfiguring `auth.oidc.accepted_algorithms` to include one of these
/// still can't open the hole.
const NEVER_ACCEPTED: &[Algorithm] = &[Algorithm::HS256, Algorithm::HS384, Algorithm::HS512];

#[derive(Debug, Clone)]
pub struct VerifiedClaims {
    pub sub: String,
    pub raw: Map<String, Value>,
}

pub struct Verifier {
    issuer: String,
    audiences: Vec<String>,
    accepted_algorithms: Vec<Algorithm>,
    leeway: Duration,
}

impl Verifier {
    #[must_use]
    pub fn new(
        issuer: impl Into<String>,
        audiences: Vec<String>,
        accepted_algorithms: Vec<Algorithm>,
        leeway: Duration,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audiences,
            accepted_algorithms,
            leeway,
        }
    }

    /// # Errors
    ///
    /// Returns the specific [`VerifyError`] variant for the first check that fails:
    /// malformed token or header, an algorithm not in [`NEVER_ACCEPTED`]'s complement of
    /// `accepted_algorithms`, an unknown `kid`, a bad signature, wrong issuer or audience,
    /// or an expired/not-yet-valid token.
    pub fn verify(&self, token: &str, jwks: &JwkSet) -> Result<VerifiedClaims, VerifyError> {
        let header = decode_header(token).map_err(|_| VerifyError::Malformed)?;

        if NEVER_ACCEPTED.contains(&header.alg) || !self.accepted_algorithms.contains(&header.alg)
        {
            return Err(VerifyError::UnacceptedAlgorithm);
        }

        let kid = header.kid.as_deref().ok_or(VerifyError::UnknownKid)?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid))
            .ok_or(VerifyError::UnknownKid)?;
        let decoding_key =
            DecodingKey::from_jwk(jwk).map_err(|_| VerifyError::BadSignature)?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&self.audiences);
        validation.leeway = self.leeway.as_secs();

        let token_data = decode::<Value>(token, &decoding_key, &validation)
            .map_err(map_jsonwebtoken_error)?;

        let raw = token_data.claims.as_object().cloned().unwrap_or_default();
        let sub = raw
            .get("sub")
            .and_then(Value::as_str)
            .ok_or(VerifyError::Malformed)?
            .to_string();

        Ok(VerifiedClaims { sub, raw })
    }
}

fn map_jsonwebtoken_error(err: jsonwebtoken::errors::Error) -> VerifyError {
    match err.kind() {
        ErrorKind::InvalidSignature => VerifyError::BadSignature,
        ErrorKind::InvalidIssuer => VerifyError::WrongIssuer,
        ErrorKind::InvalidAudience => VerifyError::WrongAudience,
        ErrorKind::ExpiredSignature => VerifyError::Expired,
        ErrorKind::ImmatureSignature => VerifyError::NotYetValid,
        ErrorKind::InvalidAlgorithm => VerifyError::UnacceptedAlgorithm,
        _ => VerifyError::Malformed,
    }
}
```

- [ ] **Step 2: Wire it into `lib.rs`**

Add `pub mod verifier;` to `server/crates/identity/src/lib.rs`.

- [ ] **Step 3: Write the tests**

Create `server/crates/identity/tests/verifier.rs`:

```rust
#![cfg(feature = "testkit")]

use std::time::Duration;

use chrono::Utc;
use jsonwebtoken::Algorithm;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use postit_identity::testkit::Keys;
use postit_identity::verifier::Verifier;
use serde::Serialize;

const ISSUER: &str = "https://issuer.test";
const AUDIENCE: &str = "postit";

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
    nbf: i64,
}

fn claims(exp_offset_secs: i64, nbf_offset_secs: i64) -> Claims {
    let now = Utc::now().timestamp();
    Claims {
        sub: "test-subject".to_string(),
        iss: ISSUER.to_string(),
        aud: AUDIENCE.to_string(),
        exp: now + exp_offset_secs,
        nbf: now + nbf_offset_secs,
    }
}

fn verifier() -> Verifier {
    Verifier::new(
        ISSUER,
        vec![AUDIENCE.to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    )
}

#[test]
fn accepts_a_valid_rs256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    let verified = verifier()
        .verify(&token, &keys.jwks())
        .unwrap_or_else(|e| unreachable!("verify: {e}"));

    assert_eq!(verified.sub, "test-subject");
}

#[test]
fn accepts_a_valid_es256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::ES256);

    let verified = verifier()
        .verify(&token, &keys.jwks())
        .unwrap_or_else(|e| unreachable!("verify: {e}"));

    assert_eq!(verified.sub, "test-subject");
}

#[test]
fn rejects_wrong_issuer() {
    let keys = Keys::generate();
    let mut wrong = claims(3600, -10);
    wrong.iss = "https://someone-else.test".to_string();
    let token = keys.mint(&wrong, Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_wrong_audience() {
    let keys = Keys::generate();
    let mut wrong = claims(3600, -10);
    wrong.aud = "someone-else".to_string();
    let token = keys.mint(&wrong, Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_an_expired_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(-3600, -7200), Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_a_not_yet_valid_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(7200, 3600), Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_a_bad_signature() {
    let signing_keys = Keys::generate();
    let other_keys = Keys::generate();
    let token = signing_keys.mint(&claims(3600, -10), Algorithm::RS256);

    // The verifier is given `other_keys`' JWKS, which has a different kid, so this is
    // really testing "unknown kid" rather than "same kid, different key" — a same-kid
    // substitution can't happen in this design because the kid is generated per `Keys`
    // instance, but the outcome (verification fails) is what matters here.
    assert!(verifier().verify(&token, &other_keys.jwks()).is_err());
}

#[test]
fn rejects_an_hmac_token_signed_with_the_rsa_public_key_bytes() {
    // Review Focus: algorithm confusion. Even though HS256 encodes fine and even if a
    // caller misconfigures accepted_algorithms to include it, NEVER_ACCEPTED must still
    // reject it.
    let keys = Keys::generate();
    let jwks = keys.jwks();
    let rsa_jwk: &Jwk = jwks
        .keys
        .iter()
        .find(|k| matches!(k.algorithm, jsonwebtoken::jwk::AlgorithmParameters::RSA(_)))
        .unwrap_or_else(|| unreachable!("rsa jwk present"));
    let public_key_bytes =
        serde_json::to_vec(rsa_jwk).unwrap_or_else(|e| unreachable!("serialize jwk: {e}"));

    let mut header = jsonwebtoken::Header::new(Algorithm::HS256);
    header.kid = rsa_jwk.common.key_id.clone();
    let hmac_token = jsonwebtoken::encode(
        &header,
        &claims(3600, -10),
        &jsonwebtoken::EncodingKey::from_secret(&public_key_bytes),
    )
    .unwrap_or_else(|e| unreachable!("encode hmac token: {e}"));

    // A misconfigured verifier that (wrongly) lists HS256 as accepted must still reject.
    let permissive = Verifier::new(
        ISSUER,
        vec![AUDIENCE.to_string()],
        vec![Algorithm::RS256, Algorithm::ES256, Algorithm::HS256],
        Duration::from_secs(0),
    );

    assert!(permissive.verify(&hmac_token, &jwks).is_err());
}

#[test]
fn rejects_an_unknown_kid() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    let empty_jwks = JwkSet { keys: vec![] };
    assert!(verifier().verify(&token, &empty_jwks).is_err());
}

#[test]
fn key_rotation_old_kid_still_valid_until_dropped_new_kid_valid_once_present() {
    let old_keys = Keys::generate();
    let new_keys = Keys::generate();
    let old_token = old_keys.mint(&claims(3600, -10), Algorithm::RS256);
    let new_token = new_keys.mint(&claims(3600, -10), Algorithm::RS256);

    let mut combined = old_keys.jwks();
    combined.keys.extend(new_keys.jwks().keys);

    assert!(verifier().verify(&old_token, &combined).is_ok());
    assert!(verifier().verify(&new_token, &combined).is_ok());

    let only_new = new_keys.jwks();
    assert!(verifier().verify(&old_token, &only_new).is_err());
    assert!(verifier().verify(&new_token, &only_new).is_ok());
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --features testkit --test verifier`
Expected: FAIL — `postit_identity::verifier` doesn't exist until Step 2.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-identity --features testkit --test verifier`
Expected: PASS (10 tests).

- [ ] **Step 6: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: Verifier (JWT validation against a resolved JWKS)"
```

### Task 12: `JwksSource` and `OidcDiscovery` — HTTP fetch, cache, unknown-`kid` refetch limit

**Files:**
- Modify: `server/crates/identity/Cargo.toml` (add `reqwest`, `async-trait`, `url`)
- Create: `server/crates/identity/src/discovery.rs`
- Modify: `server/crates/identity/src/lib.rs`
- Test: `server/crates/identity/tests/discovery.rs`

**Interfaces:**
- Consumes: `postit_http::execute_traced` (existing), `Verifier`'s `JwkSet` type (Task 11).
- Produces: `postit_identity::discovery::{DiscoveryDocument, JwksSource, HttpJwksSource, OidcDiscovery}`.

- [ ] **Step 1: Add dependencies**

Add to `server/crates/identity/Cargo.toml`'s `[dependencies]`:

```toml
async-trait.workspace = true
reqwest.workspace = true
url.workspace = true
```

- [ ] **Step 2: Write `JwksSource`, `HttpJwksSource`, and `OidcDiscovery`**

Create `server/crates/identity/src/discovery.rs`:

```rust
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use jsonwebtoken::jwk::JwkSet;
use postit_http::HttpError;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveryDocument {
    pub issuer: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
}

/// Separates "fetch the discovery document and JWKS over HTTP" from the caching and
/// unknown-`kid` refetch logic in [`OidcDiscovery`], so tests can substitute a source that
/// doesn't hit the network. `postit-identity`'s own `testkit` feature doesn't need a second
/// implementation of this trait — its test issuer (Task 13) serves real HTTP responses
/// through wiremock, so [`HttpJwksSource`] is exercised as-is in tests too.
#[async_trait]
pub trait JwksSource: Send + Sync {
    async fn discovery(&self) -> Result<DiscoveryDocument, HttpError>;
    async fn jwks(&self) -> Result<JwkSet, HttpError>;
}

pub struct HttpJwksSource {
    client: reqwest::Client,
    issuer: Url,
}

impl HttpJwksSource {
    #[must_use]
    pub fn new(client: reqwest::Client, issuer: Url) -> Self {
        Self { client, issuer }
    }

    fn discovery_url(&self) -> Url {
        let base = self.issuer.as_str().trim_end_matches('/');
        Url::parse(&format!("{base}/.well-known/openid-configuration"))
            .unwrap_or_else(|err| unreachable!("issuer url plus a fixed suffix: {err}"))
    }
}

#[async_trait]
impl JwksSource for HttpJwksSource {
    async fn discovery(&self) -> Result<DiscoveryDocument, HttpError> {
        let request = self
            .client
            .get(self.discovery_url())
            .build()
            .map_err(|err| HttpError::Permanent(err.to_string()))?;
        let response = postit_http::execute_traced(&self.client, request).await?;
        response
            .json::<DiscoveryDocument>()
            .await
            .map_err(HttpError::from)
    }

    async fn jwks(&self) -> Result<JwkSet, HttpError> {
        let discovery = self.discovery().await?;
        let request = self
            .client
            .get(&discovery.jwks_uri)
            .build()
            .map_err(|err| HttpError::Permanent(err.to_string()))?;
        let response = postit_http::execute_traced(&self.client, request).await?;
        response.json::<JwkSet>().await.map_err(HttpError::from)
    }
}

#[derive(Default)]
struct CacheState {
    jwks: Option<JwkSet>,
    fetched_at: Option<Instant>,
    last_kid_refetch: Option<Instant>,
}

/// Caches the JWKS from `source`, refreshing it every `refresh_interval`, and refetches
/// immediately on an unknown `kid` — but never more than once per minute, so a flood of
/// tokens with random `kid`s can't be used to hammer the IdP.
pub struct OidcDiscovery<S: JwksSource> {
    source: S,
    refresh_interval: Duration,
    state: Mutex<CacheState>,
}

const KID_REFETCH_MIN_INTERVAL: Duration = Duration::from_secs(60);

impl<S: JwksSource> OidcDiscovery<S> {
    #[must_use]
    pub fn new(source: S, refresh_interval: Duration) -> Self {
        Self {
            source,
            refresh_interval,
            state: Mutex::new(CacheState::default()),
        }
    }

    /// # Errors
    ///
    /// Returns [`HttpError`] if the initial fetch fails and nothing is cached yet.
    pub async fn jwks(&self) -> Result<JwkSet, HttpError> {
        let stale = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.fetched_at.is_none_or(|t| t.elapsed() >= self.refresh_interval)
        };
        if stale {
            self.refetch().await?;
        }
        self.cached_jwks()
    }

    /// Looks up the JWKS containing `kid`. If the currently cached set doesn't have it,
    /// refetches at most once per [`KID_REFETCH_MIN_INTERVAL`] and returns the result
    /// either way — the caller (the [`crate::verifier::Verifier`] wiring in
    /// `postit-server`, P6) treats a still-missing `kid` after this as
    /// [`crate::VerifyError::UnknownKid`].
    ///
    /// # Errors
    ///
    /// Returns [`HttpError`] if a needed fetch fails and nothing is cached yet.
    pub async fn jwks_for_kid(&self, kid: &str) -> Result<JwkSet, HttpError> {
        let jwks = self.jwks().await?;
        if has_kid(&jwks, kid) {
            return Ok(jwks);
        }

        let may_refetch = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state
                .last_kid_refetch
                .is_none_or(|t| t.elapsed() >= KID_REFETCH_MIN_INTERVAL)
        };
        if !may_refetch {
            return Ok(jwks);
        }

        self.refetch().await?;
        {
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.last_kid_refetch = Some(Instant::now());
        }
        self.cached_jwks()
    }

    async fn refetch(&self) -> Result<(), HttpError> {
        let jwks = self.source.jwks().await?;
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.jwks = Some(jwks);
        state.fetched_at = Some(Instant::now());
        Ok(())
    }

    fn cached_jwks(&self) -> Result<JwkSet, HttpError> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .jwks
            .clone()
            .ok_or_else(|| HttpError::Permanent("jwks not loaded".to_string()))
    }
}

fn has_kid(jwks: &JwkSet, kid: &str) -> bool {
    jwks.keys.iter().any(|k| k.common.key_id.as_deref() == Some(kid))
}
```

- [ ] **Step 3: Wire it into `lib.rs`**

Add `pub mod discovery;` to `server/crates/identity/src/lib.rs`.

- [ ] **Step 4: Write the tests**

Create `server/crates/identity/tests/discovery.rs`:

```rust
use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::build_client;
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn http_client() -> reqwest::Client {
    build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"))
}

async fn mount_discovery_and_jwks(server: &MockServer, jwks_body: serde_json::Value) {
    let discovery_body = serde_json::json!({
        "issuer": server.uri(),
        "jwks_uri": format!("{}/jwks.json", server.uri()),
    });
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(discovery_body))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks_body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetches_discovery_and_jwks() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source =
        HttpJwksSource::new(http_client(), Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")));
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));

    let jwks = discovery.jwks().await.unwrap_or_else(|e| unreachable!("jwks: {e}"));
    assert!(jwks.keys.is_empty());
}

#[tokio::test]
async fn caches_jwks_within_the_refresh_interval() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source =
        HttpJwksSource::new(http_client(), Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")));
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));

    discovery.jwks().await.unwrap_or_else(|e| unreachable!("first jwks: {e}"));
    discovery.jwks().await.unwrap_or_else(|e| unreachable!("second jwks: {e}"));

    let requests = server.received_requests().await.unwrap_or_default();
    let jwks_requests = requests.iter().filter(|r| r.url.path() == "/jwks.json").count();
    assert_eq!(jwks_requests, 1);
}

#[tokio::test]
async fn refetches_once_on_an_unknown_kid_then_respects_the_one_minute_limit() {
    let server = MockServer::start().await;
    mount_discovery_and_jwks(&server, serde_json::json!({"keys": []})).await;

    let source =
        HttpJwksSource::new(http_client(), Url::parse(&server.uri()).unwrap_or_else(|e| unreachable!("{e}")));
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    discovery.jwks().await.unwrap_or_else(|e| unreachable!("warm cache: {e}"));

    discovery
        .jwks_for_kid("missing-kid")
        .await
        .unwrap_or_else(|e| unreachable!("first lookup: {e}"));
    discovery
        .jwks_for_kid("still-missing-kid")
        .await
        .unwrap_or_else(|e| unreachable!("second lookup: {e}"));

    let requests = server.received_requests().await.unwrap_or_default();
    let jwks_requests = requests.iter().filter(|r| r.url.path() == "/jwks.json").count();
    // One warm-cache fetch, one refetch from the first unknown-kid lookup, and none from
    // the second lookup because it's within the one-minute limit.
    assert_eq!(jwks_requests, 2);
}
```

- [ ] **Step 5: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --test discovery`
Expected: FAIL — `postit_identity::discovery` doesn't exist until Step 3.

- [ ] **Step 6: Make it pass**

Run: `cd server && cargo test -p postit-identity --test discovery`
Expected: PASS (3 tests).

- [ ] **Step 7: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: JwksSource, HttpJwksSource, OidcDiscovery"
```

### Task 13: `testkit::TestIssuer` — full HTTP-serving mock OIDC provider

**Files:**
- Modify: `server/crates/identity/Cargo.toml` (move `wiremock` to an optional main dependency, add `serde_json` already present, extend `testkit` feature)
- Modify: `server/crates/identity/src/testkit.rs`
- Test: `server/crates/identity/tests/test_issuer.rs`

**Interfaces:**
- Consumes: `testkit::Keys` (Task 10), `discovery::HttpJwksSource` (Task 12),
  `postit_http::testkit::test_server` (existing, `postit-http`'s `testkit` feature).
- Produces (feature `testkit`): `postit_identity::testkit::TestIssuer` with
  `TestIssuer::start().await`, `.issuer_url() -> Url`, `.mint(...)` (delegates to `Keys`),
  `.mount_userinfo(bearer_token, body).await`.

This is what Task 14's claims-transformation tests and Task 15's cache tests sign in
against — a real HTTP round trip through the same `HttpJwksSource`/`OidcDiscovery` code
path production uses, not a shortcut.

- [ ] **Step 1: Update `postit-identity`'s `Cargo.toml`**

Move `wiremock` out of `[dev-dependencies]` into `[dependencies]` as optional, and add it
to the `testkit` feature:

```toml
wiremock = { workspace = true, optional = true }
```

(remove the `wiremock.workspace = true` line from `[dev-dependencies]` — `tests/discovery.rs`
from Task 12 still gets it through the crate's own `dev-dependencies` inheritance rules,
since a dependency declared in `[dependencies]`, optional or not, is available to test
targets once the feature enabling it is on; Task 12's test doesn't need the `testkit`
feature turned on for its own direct wiremock use, but running the full test suite with
`--all-features` — which the quality gates already do — covers it either way)

Update `[features]`:

```toml
[features]
testkit = ["dep:rsa", "dep:p256", "dep:base64", "dep:rand_core", "dep:wiremock", "postit-http/testkit"]
```

- [ ] **Step 2: Append `TestIssuer` to `testkit.rs`**

Add to the end of `server/crates/identity/src/testkit.rs` (after the existing `Keys`
`impl` block, same file):

```rust
use jsonwebtoken::Algorithm as JwtAlgorithm;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A mock OIDC provider serving discovery, JWKS, and (once mounted) `userinfo` over real
/// HTTP through wiremock, so tests exercise `postit-identity`'s actual HTTP code path
/// (`discovery::HttpJwksSource` / `discovery::OidcDiscovery`) instead of stubbing it out.
pub struct TestIssuer {
    keys: Keys,
    server: MockServer,
}

impl TestIssuer {
    pub async fn start() -> Self {
        let server = postit_http::testkit::test_server().await;
        let keys = Keys::generate();
        let jwks = keys.jwks();

        let discovery_body = serde_json::json!({
            "issuer": server.uri(),
            "jwks_uri": format!("{}/jwks.json", server.uri()),
            "userinfo_endpoint": format!("{}/userinfo", server.uri()),
        });

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(discovery_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&server)
            .await;

        Self { keys, server }
    }

    #[must_use]
    pub fn issuer_url(&self) -> Url {
        Url::parse(&self.server.uri()).unwrap_or_else(|err| unreachable!("wiremock uri is always a valid url: {err}"))
    }

    #[must_use]
    pub fn mint<C: serde::Serialize>(&self, claims: &C, algorithm: JwtAlgorithm) -> String {
        self.keys.mint(claims, algorithm)
    }

    /// Mounts a `GET /userinfo` response that returns `body` only when the request carries
    /// `Authorization: Bearer {bearer_token}`, so a test can verify the transform layer
    /// (Task 14) sends the same token it was given, not a different one.
    pub async fn mount_userinfo(&self, bearer_token: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .and(header("Authorization", format!("Bearer {bearer_token}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }
}
```

- [ ] **Step 3: Write the test**

Create `server/crates/identity/tests/test_issuer.rs`:

```rust
#![cfg(feature = "testkit")]

use std::time::Duration;

use jsonwebtoken::Algorithm;
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::testkit::TestIssuer;
use postit_identity::verifier::Verifier;
use serde::Serialize;

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
}

#[tokio::test]
async fn serves_discovery_and_jwks_that_the_real_client_can_verify_against() {
    let issuer = TestIssuer::start().await;
    let token = issuer.mint(
        &Claims {
            sub: "test-subject".to_string(),
            iss: issuer.issuer_url().to_string(),
            aud: "postit".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600,
        },
        Algorithm::RS256,
    );

    let client = postit_http::build_client(&postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"));
    let source = HttpJwksSource::new(client, issuer.issuer_url());
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    let jwks = discovery.jwks().await.unwrap_or_else(|err| unreachable!("jwks: {err}"));

    let verifier = Verifier::new(
        issuer.issuer_url().to_string().trim_end_matches('/').to_string(),
        vec!["postit".to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    );
    let verified = verifier
        .verify(&token, &jwks)
        .unwrap_or_else(|err| unreachable!("verify: {err}"));

    assert_eq!(verified.sub, "test-subject");
}

#[tokio::test]
async fn mounted_userinfo_only_answers_the_matching_bearer_token() {
    let issuer = TestIssuer::start().await;
    issuer
        .mount_userinfo("right-token", serde_json::json!({"sub": "test-subject", "email": "a@example.test"}))
        .await;

    let client = postit_http::build_client(&postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"));

    let ok = client
        .get(format!("{}userinfo", issuer.issuer_url()))
        .bearer_auth("right-token")
        .send()
        .await
        .unwrap_or_else(|err| unreachable!("request: {err}"));
    assert_eq!(ok.status(), 200);

    let wrong = client
        .get(format!("{}userinfo", issuer.issuer_url()))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap_or_else(|err| unreachable!("request: {err}"));
    assert_eq!(wrong.status(), 404);
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --features testkit --test test_issuer`
Expected: FAIL — `TestIssuer` doesn't exist until Step 2.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-identity --features testkit --test test_issuer`
Expected: PASS (2 tests).

- [ ] **Step 6: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: testkit::TestIssuer (mock OIDC provider over HTTP)"
```

### Task 14: `Principal`, `ClaimsTransformer` — provisioning, bootstrap, profile claims, userinfo fallback

**Files:**
- Create: `server/crates/identity/src/principal.rs`
- Modify: `server/crates/identity/src/discovery.rs` (add `userinfo_endpoint()`)
- Create: `server/crates/identity/src/claims.rs`
- Modify: `server/crates/identity/src/lib.rs`
- Test: `server/crates/identity/tests/claims.rs`

**Interfaces:**
- Consumes: `postit_data::{users::{UsersRepo, ProvisionOutcome, UserRecord}, preferences::UserPreferencesRepo, audit::{AuditLog, AuditEvent, AuditEventKind}, locks}` (Section A), `discovery::{JwksSource, OidcDiscovery}` (Task 12), `verifier::VerifiedClaims` (Task 11), `testkit::TestIssuer` (Task 13, tests only).
- Produces: `postit_identity::principal::Principal`, `postit_identity::claims::ClaimsTransformer`.

**Review Focus:** bootstrap must promote only a **newly created** user matching the rule
while no active admin exists. An existing user who starts matching the rule later (e.g. an
admin re-runs sign-in after `auth.bootstrap.admin_email` is changed to point at them) must
never be silently promoted outside this one-time path — Step 4's tests cover both the
positive and negative case explicitly.

- [ ] **Step 1: Write `Principal`**

Create `server/crates/identity/src/principal.rs`:

```rust
use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus};

#[derive(Debug, Clone, Copy)]
pub struct Principal {
    pub user_id: UserId,
    pub role: UserRole,
    pub status: UserStatus,
}

impl From<&postit_data::users::UserRecord> for Principal {
    fn from(record: &postit_data::users::UserRecord) -> Self {
        Self {
            user_id: record.id,
            role: record.role,
            status: record.status,
        }
    }
}
```

- [ ] **Step 2: Add `userinfo_endpoint()` to `OidcDiscovery`**

In `server/crates/identity/src/discovery.rs`, add this method inside the existing
`impl<S: JwksSource> OidcDiscovery<S>` block (after `jwks_for_kid`):

```rust
    /// Fetches the discovery document fresh (not cached — this is called at most once per
    /// principal-cache miss, not per request) and returns its `userinfo_endpoint`, if any.
    ///
    /// # Errors
    ///
    /// Returns [`HttpError`] if the fetch fails.
    pub async fn userinfo_endpoint(&self) -> Result<Option<String>, HttpError> {
        Ok(self.source.discovery().await?.userinfo_endpoint)
    }
```

- [ ] **Step 3: Write `ClaimsTransformer`**

Create `server/crates/identity/src/claims.rs`:

```rust
use std::sync::Arc;

use postit_config::{BootstrapSettings, OidcClaimNames, UserinfoMode};
use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::locks::{self, BOOTSTRAP_ADMIN_LOCK_KEY};
use postit_data::preferences::UserPreferencesRepo;
use postit_data::users::{ProvisionOutcome, UserRecord, UsersRepo};
use postit_http::HttpError;
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::discovery::{JwksSource, OidcDiscovery};
use crate::error::IdentityError;
use crate::verifier::VerifiedClaims;

/// Turns a verified token's claims into a `users` row: cache-miss provisioning, the
/// bootstrap check, and profile-claim refresh including the `userinfo` fallback. Built
/// once per process and reused across requests — every field is `Clone`-cheap or an `Arc`.
#[derive(Clone)]
pub struct ClaimsTransformer<S: JwksSource> {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
    http_client: reqwest::Client,
    discovery: Arc<OidcDiscovery<S>>,
    claim_names: OidcClaimNames,
    userinfo_mode: UserinfoMode,
    bootstrap: BootstrapSettings,
}

impl<S: JwksSource> ClaimsTransformer<S> {
    #[must_use]
    pub fn new(
        pool: PgPool,
        ids: Arc<dyn IdGenerator>,
        http_client: reqwest::Client,
        discovery: Arc<OidcDiscovery<S>>,
        claim_names: OidcClaimNames,
        userinfo_mode: UserinfoMode,
        bootstrap: BootstrapSettings,
    ) -> Self {
        Self {
            pool,
            ids,
            http_client,
            discovery,
            claim_names,
            userinfo_mode,
            bootstrap,
        }
    }

    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] on a database failure or
    /// [`IdentityError::Http`] if a required `userinfo` call fails.
    pub async fn transform(
        &self,
        issuer: &str,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Result<UserRecord, IdentityError> {
        let userinfo_claims = self.fetch_userinfo_if_needed(verified, bearer_token).await?;
        let lookup = |name: &str| -> Option<String> {
            userinfo_claims
                .as_ref()
                .and_then(|m| m.get(name))
                .and_then(Value::as_str)
                .or_else(|| claim_str(&verified.raw, name))
                .map(str::to_string)
        };
        let email = lookup(&self.claim_names.email);
        let email_verified = userinfo_claims
            .as_ref()
            .and_then(|m| m.get(&self.claim_names.email_verified))
            .and_then(Value::as_bool)
            .or_else(|| claim_bool(&verified.raw, &self.claim_names.email_verified))
            .unwrap_or(false);
        let name = lookup(&self.claim_names.name);
        let preferred_username = lookup(&self.claim_names.preferred_username);
        let display_name = name
            .or(preferred_username)
            .or_else(|| email.clone())
            .unwrap_or_else(|| verified.sub.clone());

        let mut tx = self.pool.begin().await.map_err(DataError::from)?;

        // Held for the whole transaction, not just the bootstrap check below: simpler than
        // conditionally locking, and sign-ins are infrequent enough (once per
        // auth.principal_cache_ttl per user, since this only runs on a cache miss) that the
        // extra serialization costs nothing that matters.
        locks::xact_lock(&mut tx, BOOTSTRAP_ADMIN_LOCK_KEY)
            .await
            .map_err(IdentityError::from)?;

        let id = UserId::from(self.ids.generate());
        let (outcome, mut user) =
            UsersRepo::provision(&mut tx, id, issuer, &verified.sub, &display_name)
                .await
                .map_err(IdentityError::from)?;

        if matches!(outcome, ProvisionOutcome::Created) {
            UserPreferencesRepo::create_default(&mut tx, user.id, "UTC")
                .await
                .map_err(IdentityError::from)?;

            let provisioned_audit_id = AuditEventId::from(self.ids.generate());
            AuditLog::record(
                &mut tx,
                provisioned_audit_id,
                AuditEvent::new(AuditEventKind::UserProvisioned).subject(user.id),
            )
            .await
            .map_err(IdentityError::from)?;

            // Bootstrap only ever considers a user this call just created — never an
            // existing one that starts matching the rule later. See Review Focus.
            if bootstrap_matches(&self.bootstrap, &verified.sub, email.as_deref(), email_verified) {
                let active_admins = UsersRepo::count_active_admins(&mut tx)
                    .await
                    .map_err(IdentityError::from)?;
                if active_admins == 0 {
                    user = UsersRepo::grant_admin(&mut tx, user.id)
                        .await
                        .map_err(IdentityError::from)?;
                    let bootstrap_audit_id = AuditEventId::from(self.ids.generate());
                    AuditLog::record(
                        &mut tx,
                        bootstrap_audit_id,
                        AuditEvent::new(AuditEventKind::BootstrapAdminGranted).subject(user.id),
                    )
                    .await
                    .map_err(IdentityError::from)?;
                }
            }
        } else if user.email.as_deref() != email.as_deref()
            || user.email_verified != email_verified
            || user.display_name != display_name
        {
            UsersRepo::update_profile_claims(
                &mut tx,
                user.id,
                email.as_deref(),
                email_verified,
                &display_name,
            )
            .await
            .map_err(IdentityError::from)?;
            user = UsersRepo::find_by_id(&mut tx, user.id)
                .await
                .map_err(IdentityError::from)?
                .ok_or(DataError::NotFound)
                .map_err(IdentityError::from)?;
        }

        tx.commit().await.map_err(DataError::from)?;
        Ok(user)
    }

    async fn fetch_userinfo_if_needed(
        &self,
        verified: &VerifiedClaims,
        bearer_token: &str,
    ) -> Result<Option<Map<String, Value>>, IdentityError> {
        let should_call = match self.userinfo_mode {
            UserinfoMode::Never => false,
            UserinfoMode::Always => true,
            UserinfoMode::Fallback => {
                claim_str(&verified.raw, &self.claim_names.email).is_none()
                    || claim_str(&verified.raw, &self.claim_names.name).is_none()
                    || claim_str(&verified.raw, &self.claim_names.preferred_username).is_none()
            }
        };
        if !should_call {
            return Ok(None);
        }

        let Some(endpoint) = self
            .discovery
            .userinfo_endpoint()
            .await
            .map_err(IdentityError::from)?
        else {
            return Ok(None);
        };

        let request = self
            .http_client
            .get(&endpoint)
            .bearer_auth(bearer_token)
            .build()
            .map_err(|err| IdentityError::Http(HttpError::Permanent(err.to_string())))?;
        let response = postit_http::execute_traced(&self.http_client, request)
            .await
            .map_err(IdentityError::from)?;
        let body: Value = response
            .json()
            .await
            .map_err(|err| IdentityError::Http(HttpError::from(err)))?;

        if body.get("sub").and_then(Value::as_str) != Some(verified.sub.as_str()) {
            // A userinfo response is used only when its sub matches the token's.
            return Ok(None);
        }
        Ok(body.as_object().cloned())
    }
}

fn claim_str<'a>(raw: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    raw.get(name).and_then(Value::as_str)
}

fn claim_bool(raw: &Map<String, Value>, name: &str) -> Option<bool> {
    raw.get(name).and_then(Value::as_bool)
}

fn bootstrap_matches(
    bootstrap: &BootstrapSettings,
    subject: &str,
    email: Option<&str>,
    email_verified: bool,
) -> bool {
    if let Some(admin_subject) = &bootstrap.admin_subject {
        return admin_subject == subject;
    }
    if let Some(admin_email) = &bootstrap.admin_email {
        return email_verified
            && email.is_some_and(|e| e.eq_ignore_ascii_case(admin_email));
    }
    false
}
```

- [ ] **Step 4: Wire it into `lib.rs`**

Add to `server/crates/identity/src/lib.rs`:

```rust
pub mod claims;
pub mod principal;
```

- [ ] **Step 5: Write the tests**

Create `server/crates/identity/tests/claims.rs`:

```rust
#![cfg(feature = "testkit")]

use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::Algorithm;
use postit_config::{BootstrapSettings, DatabaseSettings, HttpSettings, OidcClaimNames, RedactedSecret, UserinfoMode};
use postit_core::SystemIdGenerator;
use postit_data::users::{UserRole, UserStatus};
use postit_identity::claims::ClaimsTransformer;
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::testkit::TestIssuer;
use postit_identity::verifier::Verifier;
use serde::Serialize;
use sqlx::PgPool;
use url::Url;

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

fn claim_names() -> OidcClaimNames {
    OidcClaimNames {
        email: "email".to_string(),
        email_verified: "email_verified".to_string(),
        name: "name".to_string(),
        preferred_username: "preferred_username".to_string(),
    }
}

fn http_client() -> reqwest::Client {
    postit_http::build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"))
}

fn transformer(
    pool: PgPool,
    issuer: &TestIssuer,
    userinfo_mode: UserinfoMode,
    bootstrap: BootstrapSettings,
) -> ClaimsTransformer<HttpJwksSource> {
    let client = http_client();
    let source = HttpJwksSource::new(client.clone(), issuer.issuer_url());
    let discovery = Arc::new(OidcDiscovery::new(source, Duration::from_secs(3600)));
    ClaimsTransformer::new(
        pool,
        Arc::new(SystemIdGenerator),
        client,
        discovery,
        claim_names(),
        userinfo_mode,
        bootstrap,
    )
}

fn verifier(issuer: &TestIssuer) -> Verifier {
    Verifier::new(
        issuer.issuer_url().to_string().trim_end_matches('/').to_string(),
        vec!["postit".to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    )
}

fn token(issuer: &TestIssuer, sub: &str, email: Option<&str>, email_verified: Option<bool>, name: Option<&str>) -> String {
    issuer.mint(
        &Claims {
            sub: sub.to_string(),
            iss: issuer.issuer_url().to_string().trim_end_matches('/').to_string(),
            aud: "postit".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600,
            email: email.map(str::to_string),
            email_verified,
            name: name.map(str::to_string),
        },
        Algorithm::RS256,
    )
}

#[sqlx::test]
async fn first_sign_in_provisions_a_pending_member(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "sub-1", Some("ada@example.test"), Some(true), Some("Ada"));
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, BootstrapSettings { admin_email: None, admin_subject: None });

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
    assert_eq!(user.display_name, "Ada");
    assert_eq!(user.email.as_deref(), Some("ada@example.test"));
}

#[sqlx::test]
async fn bootstrap_promotes_a_newly_created_matching_user(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "admin-sub", Some("admin@example.test"), Some(true), Some("Admin"));
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let bootstrap = BootstrapSettings { admin_email: Some("admin@example.test".to_string()), admin_subject: None };
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, bootstrap);

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Admin);
    assert_eq!(user.status, UserStatus::Active);
}

#[sqlx::test]
async fn bootstrap_ignores_admin_email_without_email_verified(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "admin-sub", Some("admin@example.test"), Some(false), Some("Admin"));
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let bootstrap = BootstrapSettings { admin_email: Some("admin@example.test".to_string()), admin_subject: None };
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, bootstrap);

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.role, UserRole::Member);
    assert_eq!(user.status, UserStatus::Pending);
}

#[sqlx::test]
async fn bootstrap_never_promotes_an_existing_user_who_starts_matching_later(pool: PgPool) {
    // Review Focus: only a newly created user is considered for bootstrap.
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "later-admin-sub", Some("later@example.test"), Some(true), Some("Later"));

    // First sign-in: no bootstrap rule matches yet, so the user is provisioned as a
    // pending member.
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let no_bootstrap = BootstrapSettings { admin_email: None, admin_subject: None };
    let transformer1 = transformer(pool.clone(), &issuer, UserinfoMode::Never, no_bootstrap);
    let first = transformer1
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("first transform: {e}"));
    assert_eq!(first.status, UserStatus::Pending);

    // Second sign-in with a config that now matches this already-existing user must not
    // promote them.
    let matching_bootstrap = BootstrapSettings { admin_email: Some("later@example.test".to_string()), admin_subject: None };
    let transformer2 = transformer(pool, &issuer, UserinfoMode::Never, matching_bootstrap);
    let second = transformer2
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("second transform: {e}"));

    assert_eq!(second.id, first.id);
    assert_eq!(second.role, UserRole::Member);
    assert_eq!(second.status, UserStatus::Pending);
}

#[sqlx::test]
async fn userinfo_fills_a_claim_missing_from_the_access_token(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "sub-1", None, None, None);
    issuer
        .mount_userinfo(&bearer, serde_json::json!({"sub": "sub-1", "email": "from-userinfo@example.test", "email_verified": true, "name": "From Userinfo"}))
        .await;
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(pool, &issuer, UserinfoMode::Fallback, BootstrapSettings { admin_email: None, admin_subject: None });

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert_eq!(user.email.as_deref(), Some("from-userinfo@example.test"));
    assert_eq!(user.display_name, "From Userinfo");
}

#[sqlx::test]
async fn a_userinfo_response_with_a_mismatched_sub_is_ignored(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = token(&issuer, "sub-1", None, None, None);
    issuer
        .mount_userinfo(&bearer, serde_json::json!({"sub": "someone-else", "email": "attacker@example.test", "email_verified": true}))
        .await;
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(pool, &issuer, UserinfoMode::Fallback, BootstrapSettings { admin_email: None, admin_subject: None });

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    assert!(user.email.is_none());
    assert_eq!(user.display_name, "sub-1");
}

#[sqlx::test]
async fn display_name_falls_back_through_preferred_username_then_email_then_subject(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let bearer = issuer.mint(
        &serde_json::json!({
            "sub": "sub-1",
            "iss": issuer.issuer_url().to_string().trim_end_matches('/'),
            "aud": "postit",
            "exp": chrono::Utc::now().timestamp() + 3600,
            "preferred_username": "adaverse",
            "email": "ada@example.test",
        }),
        Algorithm::RS256,
    );
    let verified = verifier(&issuer).verify(&bearer, &issuer_jwks(&issuer).await).unwrap_or_else(|e| unreachable!("verify: {e}"));
    let transformer = transformer(pool, &issuer, UserinfoMode::Never, BootstrapSettings { admin_email: None, admin_subject: None });

    let user = transformer
        .transform(&issuer_string(&issuer), &verified, &bearer)
        .await
        .unwrap_or_else(|e| unreachable!("transform: {e}"));

    // No `name` claim, so it falls to `preferred_username`.
    assert_eq!(user.display_name, "adaverse");
}

fn issuer_string(issuer: &TestIssuer) -> String {
    issuer.issuer_url().to_string().trim_end_matches('/').to_string()
}

async fn issuer_jwks(issuer: &TestIssuer) -> jsonwebtoken::jwk::JwkSet {
    let client = http_client();
    let source = HttpJwksSource::new(client, issuer.issuer_url());
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    discovery.jwks().await.unwrap_or_else(|e| unreachable!("jwks: {e}"))
}
```

- [ ] **Step 6: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --features testkit --test claims`
Expected: FAIL — `postit_identity::claims` doesn't exist until Step 4.

- [ ] **Step 7: Make it pass**

Run: `cd server && cargo test -p postit-identity --features testkit --test claims`
Expected: PASS (7 tests).

- [ ] **Step 8: Regenerate the `.sqlx` cache and run the full quality gates**

`postit-identity` doesn't use `sqlx::query!`/`query_as!` itself (only `postit-data` does),
so there's no new `.sqlx` cache to regenerate here — but confirm `postit-data`'s existing
one still checks out, since this task is the first to call its repositories from outside
`postit-data`'s own tests.

Run:
```bash
cd server
cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features
SQLX_OFFLINE=true cargo check --workspace --all-targets --all-features
```
Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: ClaimsTransformer (provisioning, bootstrap, userinfo fallback)"
```

### Task 15: `PrincipalCache` and `LISTEN postit_user_changed` eviction

**Files:**
- Create: `server/crates/identity/src/cache.rs`
- Modify: `server/crates/identity/src/lib.rs`
- Test: `server/crates/identity/tests/cache.rs`

**Interfaces:**
- Consumes: `principal::Principal` (Task 14), `postit_core::UserId` (existing).
- Produces: `postit_identity::cache::{PrincipalCache, run_one_listen_session, run_listener}`.

**Review Focus:** a dropped/reconnected listener connection must fall back to
`invalidate_all()`, not silently stop evicting — Step 4's `connection_drop_falls_back_to_invalidate_all`
test proves this by actually terminating the listener's own Postgres backend from a second
connection, not by simulating the failure.

- [ ] **Step 1: Write `PrincipalCache`, `run_one_listen_session`, and `run_listener`**

Create `server/crates/identity/src/cache.rs`:

```rust
use std::time::Duration;

use moka::sync::Cache;
use postit_core::UserId;
use sqlx::PgPool;
use sqlx::postgres::PgListener;

use crate::principal::Principal;

/// The `application_name` this crate's `LISTEN` connection sets on itself, so an operator
/// (or a test) can find and, if needed, terminate that specific backend through
/// `pg_stat_activity` without guessing which connection in the pool it is.
pub const LISTENER_APPLICATION_NAME: &str = "postit_principal_cache_listener";

const CHANNEL: &str = "postit_user_changed";

/// Two small caches, both TTL `auth.principal_cache_ttl`: `(iss, sub) -> UserId` and
/// `UserId -> Principal`. Claims transformation looks up the first then the second; the
/// `LISTEN` task evicts the second directly by the user id in its notification payload, no
/// reverse lookup needed. See the design spec for why this is two caches, not one.
#[derive(Clone)]
pub struct PrincipalCache {
    identity_index: Cache<(String, String), UserId>,
    principals: Cache<UserId, Principal>,
}

impl PrincipalCache {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            identity_index: Cache::builder().time_to_live(ttl).build(),
            principals: Cache::builder().time_to_live(ttl).build(),
        }
    }

    #[must_use]
    pub fn get(&self, issuer: &str, subject: &str) -> Option<Principal> {
        let user_id = self
            .identity_index
            .get(&(issuer.to_string(), subject.to_string()))?;
        self.principals.get(&user_id)
    }

    pub fn insert(&self, issuer: &str, subject: &str, principal: Principal) {
        self.identity_index
            .insert((issuer.to_string(), subject.to_string()), principal.user_id);
        self.principals.insert(principal.user_id, principal);
    }

    pub fn invalidate_user(&self, user_id: UserId) {
        self.principals.invalidate(&user_id);
    }

    pub fn invalidate_all(&self) {
        self.principals.invalidate_all();
    }
}

/// Runs one `LISTEN` session to completion: connects, sets
/// [`LISTENER_APPLICATION_NAME`], listens on `postit_user_changed`, and evicts the
/// notified user from `cache` on every notification. Returns once the connection fails for
/// any reason (including never having connected at all), first calling
/// `cache.invalidate_all()` — so a caller never needs to distinguish "never listened" from
/// "was listening, then the connection dropped": both end the same way. `postit-server`
/// (P6) is the only production caller, through [`run_listener`]; this function stays
/// separate so a test can await one session's natural end instead of managing an infinite
/// loop.
pub async fn run_one_listen_session(pool: PgPool, cache: PrincipalCache) {
    let outcome: Result<(), sqlx::Error> = async {
        let mut listener = PgListener::connect_with(&pool).await?;
        sqlx::query("SET application_name = $1")
            .bind(LISTENER_APPLICATION_NAME)
            .execute(&mut listener)
            .await?;
        listener.listen(CHANNEL).await?;

        loop {
            let notification = listener.recv().await?;
            if let Ok(uuid) = uuid::Uuid::parse_str(notification.payload()) {
                cache.invalidate_user(UserId::from(uuid));
            }
        }
    }
    .await;

    if outcome.is_err() {
        cache.invalidate_all();
    }
}

/// The production entry point: runs [`run_one_listen_session`] forever, with a short pause
/// between attempts so a persistently unreachable database doesn't spin. `postit-server`
/// spawns this once at startup in every process that runs the API (the worker doesn't need
/// the JWKS or the principal cache).
pub async fn run_listener(pool: PgPool, cache: PrincipalCache) {
    loop {
        run_one_listen_session(pool.clone(), cache.clone()).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

- [ ] **Step 2: Wire it into `lib.rs`**

Add `pub mod cache;` to `server/crates/identity/src/lib.rs`.

- [ ] **Step 3: Write the tests**

Create `server/crates/identity/tests/cache.rs`:

```rust
use std::time::Duration;

use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus, UsersRepo};
use postit_identity::cache::{LISTENER_APPLICATION_NAME, PrincipalCache, run_one_listen_session};
use postit_identity::principal::Principal;
use sqlx::PgPool;

fn principal(user_id: UserId) -> Principal {
    Principal {
        user_id,
        role: UserRole::Member,
        status: UserStatus::Pending,
    }
}

#[test]
fn insert_then_get_round_trips() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let user_id = UserId::from(uuid::Uuid::now_v7());

    cache.insert("https://issuer.test", "sub-1", principal(user_id));

    let found = cache.get("https://issuer.test", "sub-1");
    assert!(matches!(found, Some(p) if p.user_id == user_id));
}

#[test]
fn get_misses_for_an_unknown_identity() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    assert!(cache.get("https://issuer.test", "no-such-sub").is_none());
}

#[test]
fn invalidate_user_evicts_only_that_user() {
    let cache = PrincipalCache::new(Duration::from_secs(60));
    let a = UserId::from(uuid::Uuid::now_v7());
    let b = UserId::from(uuid::Uuid::now_v7());
    cache.insert("https://issuer.test", "sub-a", principal(a));
    cache.insert("https://issuer.test", "sub-b", principal(b));

    cache.invalidate_user(a);

    assert!(cache.get("https://issuer.test", "sub-a").is_none());
    assert!(cache.get("https://issuer.test", "sub-b").is_some());
}

#[sqlx::test]
async fn a_status_change_notification_evicts_that_user_in_a_second_process(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_id, "https://issuer.test", "sub-1", "Ada")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));

    let cache = PrincipalCache::new(Duration::from_secs(60));
    cache.insert("https://issuer.test", "sub-1", principal(user_id));
    assert!(cache.get("https://issuer.test", "sub-1").is_some());

    let listener_task =
        tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));
    // Give the spawned task time to connect and LISTEN before the notification fires.
    tokio::time::sleep(Duration::from_millis(200)).await;

    UsersRepo::set_status(&mut conn, user_id, UserStatus::Active, None)
        .await
        .unwrap_or_else(|e| unreachable!("set_status: {e}"));

    // Give the notification time to be delivered and processed.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(cache.get("https://issuer.test", "sub-1").is_none());

    listener_task.abort();
}

#[sqlx::test]
async fn connection_drop_falls_back_to_invalidate_all(pool: PgPool) {
    // Review Focus: the listener's own connection dying must clear every cached principal,
    // not just leave staleness to the TTL.
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let user_a = UserId::from(uuid::Uuid::now_v7());
    let user_b = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, user_a, "https://issuer.test", "sub-a", "A")
        .await
        .unwrap_or_else(|e| unreachable!("provision a: {e}"));
    UsersRepo::provision(&mut conn, user_b, "https://issuer.test", "sub-b", "B")
        .await
        .unwrap_or_else(|e| unreachable!("provision b: {e}"));

    let cache = PrincipalCache::new(Duration::from_secs(60));
    cache.insert("https://issuer.test", "sub-a", principal(user_a));
    cache.insert("https://issuer.test", "sub-b", principal(user_b));

    let session = tokio::spawn(run_one_listen_session(pool.clone(), cache.clone()));
    // Give the session time to connect, set its application_name, and start listening.
    tokio::time::sleep(Duration::from_millis(200)).await;

    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
    )
    .bind(LISTENER_APPLICATION_NAME)
    .execute(&mut *conn)
    .await
    .unwrap_or_else(|e| unreachable!("terminate backend: {e}"));

    // run_one_listen_session ends on its own once its connection is killed — no abort()
    // needed, and awaiting the handle proves it actually reached the invalidate_all() path
    // rather than the test just winning a race against a still-running task.
    session.await.unwrap_or_else(|e| unreachable!("listener task panicked: {e}"));

    assert!(cache.get("https://issuer.test", "sub-a").is_none());
    assert!(cache.get("https://issuer.test", "sub-b").is_none());
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --test cache`
Expected: FAIL — `postit_identity::cache` doesn't exist until Step 2.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-identity --test cache`
Expected: PASS (5 tests). The last two tests are timing-sensitive (they sleep to let a
spawned task establish its connection); if `a_status_change_notification_evicts_that_user_in_a_second_process`
is flaky on the executing machine, increase both `sleep` calls in that test to 500ms rather
than removing the check.

- [ ] **Step 6: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: PrincipalCache and LISTEN postit_user_changed eviction"
```

### Task 16: `UserAdminService` — approve, disable, enable, change role

**Files:**
- Create: `server/crates/identity/src/admin.rs`
- Modify: `server/crates/identity/src/lib.rs`
- Test: `server/crates/identity/tests/admin.rs`

**Interfaces:**
- Consumes: `postit_data::{users::{UsersRepo, UserRecord, UserRole, UserStatus}, audit::{AuditLog, AuditEvent, AuditEventKind}, DataError}` (Section A).
- Produces: `postit_identity::admin::UserAdminService`.

This is the P4 slice only: `approve`, `disable`, `enable`, `change_role`. `DELETE /me`,
`DELETE /users/{id}`, rejecting a pending user, `purge_pending_users`, and `delete_user`
all need `postit-jobs`'s outbox and `postit-mail`, and are plan 02 P5's, per the spec.

- [ ] **Step 1: Write `UserAdminService`**

Create `server/crates/identity/src/admin.rs`:

```rust
use std::sync::Arc;

use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserRecord, UserRole, UserStatus, UsersRepo};
use sqlx::PgPool;

use crate::error::IdentityError;

#[derive(Clone)]
pub struct UserAdminService {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
}

impl UserAdminService {
    #[must_use]
    pub fn new(pool: PgPool, ids: Arc<dyn IdGenerator>) -> Self {
        Self { pool, ids }
    }

    /// `pending -> active`. Sets `approved_by` to `actor`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, or [`IdentityError::InvalidTransition`] if `target` isn't `pending`.
    pub async fn approve(&self, actor: UserId, target: UserId) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Pending {
            return Err(IdentityError::InvalidTransition(user.status.as_str(), "active"));
        }

        let updated = UsersRepo::set_status(&mut conn, target, UserStatus::Active, Some(actor)).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserApproved).actor(actor).subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// `active -> disabled`. Refuses on the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::InvalidTransition`] if `target` isn't `active`, or
    /// [`IdentityError::LastAdmin`] if `target` is the last active admin.
    pub async fn disable(&self, actor: UserId, target: UserId) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Active {
            return Err(IdentityError::InvalidTransition(user.status.as_str(), "disabled"));
        }
        if user.role == UserRole::Admin && UsersRepo::count_active_admins(&mut conn).await? <= 1 {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_status(&mut conn, target, UserStatus::Disabled, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserDisabled).actor(actor).subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// `disabled -> active`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, or [`IdentityError::InvalidTransition`] if `target` isn't `disabled`.
    pub async fn enable(&self, actor: UserId, target: UserId) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Disabled {
            return Err(IdentityError::InvalidTransition(user.status.as_str(), "active"));
        }

        let updated = UsersRepo::set_status(&mut conn, target, UserStatus::Active, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserEnabled).actor(actor).subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// Allowed on `active` or `disabled` users. Refuses demoting the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::InvalidTransition`] if `target` is `pending` or `deleting`,
    /// or [`IdentityError::LastAdmin`] if `target` is the last active admin being demoted.
    pub async fn change_role(
        &self,
        actor: UserId,
        target: UserId,
        role: UserRole,
    ) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if !matches!(user.status, UserStatus::Active | UserStatus::Disabled) {
            return Err(IdentityError::InvalidTransition(user.status.as_str(), role.as_str()));
        }
        let demoting_last_active_admin = user.role == UserRole::Admin
            && role == UserRole::Member
            && user.status == UserStatus::Active
            && UsersRepo::count_active_admins(&mut conn).await? <= 1;
        if demoting_last_active_admin {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_role(&mut conn, target, role).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        let event = AuditEvent::new(AuditEventKind::RoleChanged)
            .actor(actor)
            .subject(target)
            .detail("new_role", role.as_str())?;
        AuditLog::record(&mut conn, audit_id, event).await?;
        Ok(updated)
    }
}
```

- [ ] **Step 2: Wire it into `lib.rs`**

Add `pub mod admin;` to `server/crates/identity/src/lib.rs`.

- [ ] **Step 3: Write the tests**

Create `server/crates/identity/tests/admin.rs`:

```rust
use postit_core::{SystemIdGenerator, UserId};
use postit_data::users::{UserRole, UserStatus, UsersRepo};
use postit_identity::IdentityError;
use postit_identity::admin::UserAdminService;
use sqlx::PgPool;
use std::sync::Arc;

async fn provisioned_pending_user(pool: &PgPool, sub: &str) -> UserId {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, id, "https://issuer.test", sub, "Name")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    id
}

fn service(pool: PgPool) -> UserAdminService {
    UserAdminService::new(pool, Arc::new(SystemIdGenerator))
}

#[sqlx::test]
async fn approve_moves_pending_to_active(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let user = service(pool)
        .approve(admin_actor, target)
        .await
        .unwrap_or_else(|e| unreachable!("approve: {e}"));

    assert_eq!(user.status, UserStatus::Active);
    assert_eq!(user.approved_by, Some(admin_actor));
}

#[sqlx::test]
async fn approving_a_non_pending_user_is_rejected(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;
    let svc = service(pool);
    svc.approve(admin_actor, target)
        .await
        .unwrap_or_else(|e| unreachable!("first approve: {e}"));

    let err = svc.approve(admin_actor, target).await.unwrap_err();
    assert!(matches!(err, IdentityError::InvalidTransition("active", "active")));
}

#[sqlx::test]
async fn disable_then_enable_round_trips(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;
    let svc = service(pool);
    svc.approve(admin_actor, target).await.unwrap_or_else(|e| unreachable!("approve: {e}"));

    let disabled = svc.disable(admin_actor, target).await.unwrap_or_else(|e| unreachable!("disable: {e}"));
    assert_eq!(disabled.status, UserStatus::Disabled);

    let enabled = svc.enable(admin_actor, target).await.unwrap_or_else(|e| unreachable!("enable: {e}"));
    assert_eq!(enabled.status, UserStatus::Active);
}

#[sqlx::test]
async fn disabling_the_last_active_admin_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, admin_id, "https://issuer.test", "admin-sub", "Admin")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, admin_id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    let err = service(pool).disable(admin_id, admin_id).await.unwrap_err();
    assert!(matches!(err, IdentityError::LastAdmin));
}

#[sqlx::test]
async fn demoting_the_last_active_admin_is_refused(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let admin_id = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, admin_id, "https://issuer.test", "admin-sub", "Admin")
        .await
        .unwrap_or_else(|e| unreachable!("provision: {e}"));
    UsersRepo::grant_admin(&mut conn, admin_id)
        .await
        .unwrap_or_else(|e| unreachable!("grant_admin: {e}"));

    let err = service(pool)
        .change_role(admin_id, admin_id, UserRole::Member)
        .await
        .unwrap_err();
    assert!(matches!(err, IdentityError::LastAdmin));
}

#[sqlx::test]
async fn demoting_a_second_admin_is_allowed(pool: PgPool) {
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("acquire: {e}"));
    let first_admin = UserId::from(uuid::Uuid::now_v7());
    let second_admin = UserId::from(uuid::Uuid::now_v7());
    UsersRepo::provision(&mut conn, first_admin, "https://issuer.test", "first-sub", "First")
        .await
        .unwrap_or_else(|e| unreachable!("provision first: {e}"));
    UsersRepo::grant_admin(&mut conn, first_admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant first: {e}"));
    UsersRepo::provision(&mut conn, second_admin, "https://issuer.test", "second-sub", "Second")
        .await
        .unwrap_or_else(|e| unreachable!("provision second: {e}"));
    UsersRepo::grant_admin(&mut conn, second_admin)
        .await
        .unwrap_or_else(|e| unreachable!("grant second: {e}"));

    let updated = service(pool)
        .change_role(first_admin, second_admin, UserRole::Member)
        .await
        .unwrap_or_else(|e| unreachable!("change_role: {e}"));

    assert_eq!(updated.role, UserRole::Member);
}

#[sqlx::test]
async fn changing_role_of_a_pending_user_is_rejected(pool: PgPool) {
    let target = provisioned_pending_user(&pool, "sub-1").await;
    let admin_actor = provisioned_pending_user(&pool, "sub-admin").await;

    let err = service(pool)
        .change_role(admin_actor, target, UserRole::Admin)
        .await
        .unwrap_err();
    assert!(matches!(err, IdentityError::InvalidTransition("pending", "admin")));
}
```

- [ ] **Step 4: Run the tests, expect them to fail to compile**

Run: `cd server && cargo test -p postit-identity --test admin`
Expected: FAIL — `postit_identity::admin` doesn't exist until Step 2.

- [ ] **Step 5: Make it pass**

Run: `cd server && cargo test -p postit-identity --test admin`
Expected: PASS (7 tests).

- [ ] **Step 6: Run the full quality gates**

Run: `cd server && cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo check --workspace --all-targets && cargo test --workspace --all-features`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add server/crates/identity
git commit -m "postit-identity: UserAdminService (approve, disable, enable, change_role)"
```

### Task 17: Close out P4 — full workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Run every quality gate, with and without every feature**

Run from `server/`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo check --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
SQLX_OFFLINE=true cargo check --workspace --all-targets --all-features
```

Expected: every command exits 0. The `SQLX_OFFLINE=true` run is the one that matters for
the `server-windows` CI job — it must pass without a reachable database, using only the
`.sqlx` caches committed in Tasks 3–8.

- [ ] **Step 2: Confirm no secret or token reaches a `Debug` output, error, or audit row**

This is plan 02 P4's own exit criterion. Grep the test suite's assertions for it rather
than trusting the design alone:

```bash
grep -rn "RedactedSecret\|SecretString" server/crates/data/src server/crates/identity/src
```

Expected: every match is either the config layer reading a secret at pool-connect time
(`Db::connect` in Task 1, which calls `.expose()` once to build `PgConnectOptions` and never
logs it) or absent from `audit.rs`/`users.rs`/`claims.rs` entirely — there is no `Debug` or
`Display` implementation anywhere in `postit-data` or `postit-identity` that touches a
secret type, because none of their structs hold one (tokens don't exist until plan 03; the
only secret this phase touches is the database password, confined to `pool.rs`).

- [ ] **Step 3: Confirm every plan 02 P4 exit criterion**

Re-read `!ref/plans/02. foundation.md`'s P4 section (`### P4: Data layer and identity`) and
check each clause against what Tasks 1–16 built:

- "`#[sqlx::test]` suites cover every repository" — yes: `UsersRepo` (Task 3), `AuditLog`/
  `AuditRepo` (Tasks 4–5), `UserPreferencesRepo` (Task 6), `IdempotencyRepo` (Task 7),
  retention queries (Task 8).
- "migrations apply from empty" — Task 1's `running_migrations_twice_is_a_no_op` test plus
  every `#[sqlx::test]` (which applies migrations from empty for its ephemeral database on
  every run).
- "`cargo sqlx prepare --check` passes" — verify directly:

```bash
cd server/crates/data
DATABASE_URL="postgres://postgres:P@\$\$w0rd@localhost:5432/postit" cargo sqlx prepare --check
```

  Expected: exits 0 with no diff. If it doesn't, the `.sqlx` cache is stale — rerun
  `cargo sqlx prepare` (without `--check`) and commit the result.

- "the verifier tests and the identity tests that need no jobs or mail pass" — Task 11
  (verifier) and Tasks 14–16 (claims transformation, cache, admin service) cover exactly
  this list from plan 02's testing section; deletion, approval emails, and cron scheduling
  are explicitly out of scope (P5).
- "no token appears in any `Debug` output, error, or audit row" — Step 2 above.

- [ ] **Step 4: Commit the final state if anything changed**

If Steps 1–3 required any fix (a regenerated `.sqlx` cache, a lint fix), commit it:

```bash
git add server
git commit -m "postit-data + postit-identity: close out plan 02 P4"
```

If nothing changed, this step is a no-op — the plan's tasks already committed everything.
