# A2: Vault, Platforms Table, and Configuration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `postit-vault` (envelope-sealed column values with key rotation), the `platforms` and `vault_state` tables with the startup upsert and platform-config validation, and every plan 03 config section, so later phases store secrets and read settings through one tested path.

**Architecture:**
- `postit-config` gains the new sections and validation, including the QA and production key requirements and the refusal of development keys.
- New `postit-vault` (depends on `postit-config`) seals values with XChaCha20-Poly1305 under per-value data keys wrapped by HKDF-derived key-encryption keys. The associated data binds each value to its table, column, and row.
- `postit-data` adds the tables, `PlatformRepo`, the `vault_state` guard, the sealed-column catalog, and the re-seal routine.
- `postit-server` builds the `AgentRegistry`, upserts platforms, validates the `[platforms.<id>]` tables, runs the vault startup check, and registers the `vault_rotate` job.

**Tech Stack:** Rust 2024, `chacha20poly1305` 0.10, `hkdf` 0.12, `zeroize`, `sha2` 0.10, sqlx 0.9 (Postgres), figment, and the existing `postit-jobs`.

**Spec:** `docs/specs/2026-10-19-a2-vault-platforms-config-design.md`, which argues from `docs/plans/03. publishing roadmap.md` phase A2. Read both.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints: `unsafe_code = "forbid"`, clippy `all` + `pedantic`, and `unwrap_used`, `expect_used`, `panic` set to deny. Tests may `#[allow]` these locally, as existing tests do.
- Dependencies go in `backend/Cargo.toml` `[workspace.dependencies]` and are used with `.workspace = true`. New: `chacha20poly1305 = "0.10"`, `hkdf = "0.12"`, `zeroize = "1"`, `postit-vault = { path = "crates/vault" }`. Add `crates/vault` to `members`.
- Dependency order: `vault` depends on `config` (and nothing internal above it). `data` depends on `vault` with its `sqlx` feature. Nothing in `vault` depends on `data`.
- Single migration: every schema change goes into `backend/crates/data/migrations/0001_init.sql`.
- No secret (key material, plaintext, sealed bytes, platform config values) appears in `Debug`, `Display`, an error, a log, or `redacted_dump()`.
- Config error messages name the full key and its env var (for example `vault.keys.1 (POSTIT__VAULT__KEYS__1)`), in the tone of the existing `ConfigError::Validation` messages.
- Quality gates after every task, from `backend/`:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test -p <touched crates>`

  The last task runs the full gates from CLAUDE.md.
- Commits use Conventional Commits with the crate as scope and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Database tests use `#[sqlx::test]`, like `backend/crates/data/tests/*.rs`.
- Writing: American English in code, comments, and docs.

## Review Focus

1. **A key in a `_FILE` secret with a trailing newline.** It must produce the same key-encryption key as the same key passed as an env var. Otherwise a deploy that moves from env vars to files cannot open any sealed value. Task 1 tests this.
2. **Key versions written `01` and `1`, or `v2`.** These must be rejected as non-canonical or duplicate, never silently become two versions or the wrong newest one. Task 1 tests this.
3. **Rows deleted, or set to NULL, while rotation runs.** Rotation must skip them and count them as skipped, not fail the job. Task 5 tests this.
4. **A boolean platform field set by env var as `TRUE`, `1`, or `" true "`.** Only `true` and `false` (case-insensitive, after trimming) convert. Any other value fails validation and the error names the field, not the value. Task 6 tests this.
5. **A build with no agents compiled in, starting against a database that already has `platforms` rows.** Startup must succeed, keep the rows, and log each one as `not_compiled_in`. Task 6 tests this.

---

### Task 1: Config: `[vault]`, `[media]`, `[email]`, `[push]` and the key rules

**Files:**
- Modify: `backend/crates/config/src/settings.rs` (or split the new sections into `backend/crates/config/src/settings_publishing.rs` if `settings.rs` passes about 900 lines), `backend/crates/config/src/lib.rs`, `backend/crates/config/src/loader.rs` (tests), `backend/config/default.toml`, `backend/config/development.toml`

**Interfaces:**
- Produces:
  - `VaultSettings { keys: BTreeMap<String, RedactedSecret> }` with `fn versions(&self) -> Result<Vec<(u32, &RedactedSecret)>, ConfigError>`, sorted ascending.
  - `MediaSettings`. Fields and defaults:
    - `storage_root: PathBuf` (`/var/lib/postit/media`; development `../.data/media`)
    - `signing_key: Option<RedactedSecret>`
    - `public_base_url: Option<Url>`
    - `single_request_max: u64` (20_000_000)
    - `chunk_max: u64` (16_000_000)
    - `upload_session_ttl: Duration` (24h)
    - `view_url_ttl: Duration` (10min)
    - `public_url_ttl: Duration` (1h)
    - `user_quota_bytes: u64` (21_474_836_480; 0 means unlimited)
    - `fit_concurrency: u32` (1)
    - `fit_max_duration: Duration` (10min)
    - `ffmpeg_path: PathBuf` (`ffmpeg`)
  - `EmailSettings`. Fields and defaults:
    - `signing_key: Option<RedactedSecret>`
    - `batch_size: u32` (50)
    - `image_url_ttl: Duration` (365days)
    - `tracking_base_url: Option<Url>`
    - `allow_private_hosts: bool` (false)
  - `PushSettings`. Fields and defaults:
    - `enabled: bool` (false)
    - `vapid_private_key: Option<RedactedSecret>`
    - `vapid_subject: Option<String>` (`blank_as_none`)
    - `allow_private_endpoints: bool` (false)
  - New `Settings` fields `vault`, `media`, `email`, `push`.
  - `pub const DEVELOPMENT_KEYS: &[&str]` exported from `postit-config`, holding the literal values shipped in `development.toml`.

- [ ] **Step 1: Write failing tests** in `loader.rs` tests, using the existing `BASELINE` and env-loading helpers:

```rust
#[test] fn vault_keys_load_from_env_and_file_identically() // POSTIT__VAULT__KEYS__1=<k> vs POSTIT__VAULT__KEYS__1_FILE=<file containing k + "\n">: versions() equal, same expose()
#[test] fn vault_version_must_be_canonical_positive_integer() // keys "0", "01", "v2", "-1" each -> Err containing "vault.keys"
#[test] fn vault_key_shorter_than_32_bytes_rejected() // 31-byte key -> Err containing "vault.keys.1" and "32"
#[test] fn qa_requires_vault_and_signing_keys() // qa with each of vault.keys / media.signing_key / email.signing_key missing -> Err naming that key and its POSTIT__ var
#[test] fn development_needs_no_extra_secrets() // BASELINE + development.toml loads in development
#[test] fn development_keys_refused_outside_development() // qa with media.signing_key = DEVELOPMENT_KEYS[1] -> Err containing "development key"
#[test] fn signing_keys_must_differ() // media.signing_key == email.signing_key -> Err; == audit.pseudonym_key -> Err
#[test] fn push_enabled_requires_vapid() // push.enabled=true without vapid_private_key or vapid_subject -> Err in every environment
#[test] fn redacted_dump_hides_new_secrets() // dump JSON contains no key values; vault.keys.1 == "[redacted]"
```

- [ ] **Step 2: Run** `cargo test -p postit-config`. Expected: compile failure, because the sections are missing.

- [ ] **Step 3: Implement.**
  - Keys are `BTreeMap<String, _>`, because figment env keys are strings.
  - `versions()` accepts only `[1-9][0-9]*`, which rejects duplicates by construction.
  - Key length is measured after the loader's existing trim. Confirm the `_FILE` path trims; if it does not, trim in `versions()`.
  - Add the length, presence, distinctness, development-key, and push rules to `Settings::validate`, and add the new durations to `validate_bounds`.
  - Update the doc comment on `validate` listing every rule.
  - In `default.toml`, add `[media]`, `[email]`, and `[push]` with the defaults above (no secrets) and an empty `[vault.keys]`.
  - In `development.toml`, add `vault.keys.1`, `media.signing_key`, and `email.signing_key`, each a distinct literal of at least 32 bytes containing `insecure-dev-only`, under a comment marking them insecure and dev-only. Also add `media.storage_root = "../.data/media"`.
  - Add `.data/` to `.gitignore` if it is not already there.

- [ ] **Step 4: Run** `cargo test -p postit-config` and clippy. Expected: PASS. Then run `cargo test --workspace` once, to catch other crates' test settings that now need the new sections. Fix those fixtures by adding the dev keys.

- [ ] **Step 5: Commit** `feat(config): add vault, media, email, and push settings with key rules`.

---

### Task 2: Config: reserved sections and `[platforms.<id>]`

**Files:**
- Modify: same as Task 1. Create `backend/crates/config/src/platforms.rs`.

**Interfaces:**
- Produces:
  - `PublishingSettings { live: bool (true), max_attempts: u32 (5), lease (5min), sweep_grace (2min), park_grace (15min), processing_timeout (24h) }`
  - `OAuthSettings { return_to_allowlist: Vec<String> (empty) }`. Empty means only `app.public_url` and the app's custom URL scheme. Document this on the field. Enforcement is B7.
  - `AiSettings { request_timeout (120s), allow_private_endpoints: bool (false) }`
  - `LimitsSettings { email_daily_cap: u32 (500), contacts_cap: u32 (10000), storage_quota_bytes: Option<u64> }`. 0 means unlimited for all three, and `None` means `media.user_quota_bytes`.
  - `RecurrenceSettings { lead (10min) }`
  - `InboxSettings { active_interval (5min), idle_interval (1h) }`
  - `RetentionSettings` gains `inbox_items`, `direct_sends`, `email_events` (180days each).
  - `LegalSettings { privacy_path, terms_path, data_deletion_path: Option<PathBuf> }`
  - `PlatformTable(serde_json::Map<String, Value>)` with `fn values(&self) -> &Map<String, Value>`. Its `Serialize` and `Debug` emit each key with `"[redacted]"`.
  - `Settings.platforms: BTreeMap<String, PlatformTable>` (`#[serde(default)]`).
  - Each section's doc comment starts "Reserved: validated at load, not yet read", like `OpsSettings`.

- [ ] **Step 1: Write failing tests:**

```rust
#[test] fn reserved_sections_load_defaults() // assert publishing.max_attempts == 5, publishing.sweep_grace == 2min, limits.email_daily_cap == 500, limits.contacts_cap == 10000, ai.request_timeout == 120s, inbox.idle_interval == 1h, retention.inbox_items == 180 days
#[test] fn limits_zero_means_unlimited_is_accepted() // POSTIT__LIMITS__EMAIL_DAILY_CAP=0 loads
#[test] fn platform_table_from_toml_and_env_file() // [platforms.tiktok] client_id = "abc" in a temp config + POSTIT__PLATFORMS__TIKTOK__CLIENT_SECRET_FILE -> values() has both keys; secret value equals file contents (trimmed)
#[test] fn platform_tables_are_redacted() // format!("{:?}") and redacted_dump() contain "client_id" and "[redacted]" but not "abc" or the secret
#[test] fn zero_durations_rejected_in_new_sections() // publishing.lease = "0s" -> Err containing "publishing.lease"
```

- [ ] **Step 2: Run** `cargo test -p postit-config`. Expected: FAIL.

- [ ] **Step 3: Implement** the sections, `default.toml` entries, bounds checks, and `PlatformTable`. If the `_FILE` layer does not reach nested map keys, extend `FileSecrets` so that any `POSTIT__A__B__C_FILE` sets `a.b.c`, and keep the existing tests passing.

- [ ] **Step 4: Run** `cargo test -p postit-config`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(config): add reserved plan 03 sections and opaque platform tables`.

---

### Task 3: `postit-vault` crate

**Files:**
- Create: `backend/crates/vault/Cargo.toml`, `src/lib.rs`, `src/key.rs` (key-encryption key derivation and the key ring), `src/envelope.rs` (format, seal, open, rewrap), `src/sealed.rs` (`Sealed<T>`, `SealContext`, the sqlx impls behind the `sqlx` feature), `src/error.rs`
- Modify: `backend/Cargo.toml`

**Interfaces:**
- Consumes: `postit_config::VaultSettings::versions()` (Task 1).
- Produces:
  - `pub struct Vault` (`Clone`, holds `Arc` of a zeroizing key ring):
    - `Vault::from_settings(&VaultSettings) -> Result<Vault, VaultError>`
    - `Vault::from_keys(impl IntoIterator<Item = (u32, Vec<u8>)>) -> Result<Vault, VaultError>`
    - `newest_version(&self) -> u32`
    - `versions(&self) -> Vec<u32>`
    - `seal<T: Serialize>(&self, value: &T, ctx: &SealContext) -> Result<Sealed<T>, VaultError>`
    - `open<T: DeserializeOwned>(&self, sealed: &Sealed<T>, ctx: &SealContext) -> Result<T, VaultError>`
    - `rewrap<T>(&self, sealed: &Sealed<T>, ctx: &SealContext) -> Result<Option<Sealed<T>>, VaultError>` returns `None` when the value is already on the newest version.
  - `pub struct SealContext { pub table: &'static str, pub column: &'static str, pub row: Uuid }`
  - `pub struct Sealed<T>`:
    - `from_bytes(Vec<u8>) -> Result<Sealed<T>, VaultError>` validates the header.
    - `as_bytes(&self) -> &[u8]`
    - `key_version(&self) -> u32`
    - `Debug` prints `Sealed(kek v{n}, {len} bytes)`.
    - No `Serialize` or `Display`.
    - With the `sqlx` feature: `sqlx::Type<Postgres>`, `Encode`, `Decode` as `bytea`.
  - `pub enum VaultError { NoKeys, KeyTooShort { version }, InvalidVersion, DuplicateVersion { version }, UnknownKeyVersion { version }, Malformed, AuthenticationFailed, Serialization }`. `Display` carries no bytes.
  - `pub const FORMAT_VERSION: u8 = 1`.
  - Layout:

    ```text
    [1][kek_version u32 BE][wrap_nonce 24][wrapped_dek 48][data_nonce 24][ciphertext + 16]
    ```

    The header is 101 bytes.

- [ ] **Step 1: Write failing tests** in `src/envelope.rs` and `src/sealed.rs`:

```rust
#[test] fn round_trip_string_and_struct()
#[test] fn empty_and_one_mebibyte_plaintexts_round_trip()
#[test] fn other_row_column_or_table_fails() // each variation -> Err(AuthenticationFailed)
#[test] fn every_single_byte_flip_fails() // flip each byte of a small envelope -> Err(Malformed | AuthenticationFailed | UnknownKeyVersion)
#[test] fn truncated_envelope_is_malformed()
#[test] fn newest_seals_and_old_opens() // vault{1} seals; vault{1,2}.open works; vault{1,2}.seal -> key_version()==2
#[test] fn rewrap_moves_to_newest_and_keeps_plaintext() // and returns None when already newest; data_nonce and ciphertext bytes unchanged after rewrap
#[test] fn unknown_version_fails() // sealed by {2}, opened by {1} -> UnknownKeyVersion{version:2}
#[test] fn format_test_vector() // test-only seal_with(dek, wrap_nonce, data_nonce) with fixed bytes -> hex equals pinned constant
#[test] fn key_rules() // empty, 31-byte, version 0, duplicate -> matching VaultError
#[test] fn nothing_secret_in_debug_or_errors() // Debug of Vault, Sealed, every VaultError contains no key bytes, plaintext, or ciphertext hex
```

- [ ] **Step 2: Run** `cargo test -p postit-vault`. Expected: compile failure.

- [ ] **Step 3: Implement.**
  - Derive each key-encryption key with `Hkdf::<Sha256>::new(None, key_material).expand(format!("postit vault kek v{version}"), &mut [0u8; 32])`.
  - The associated data is the length-prefixed (`u32` big-endian) concatenation of `postit.vault.v1`, `table`, `column`, and the 16 raw UUID bytes.
  - Nonces and data keys come from `OsRng` (`chacha20poly1305::aead::OsRng`).
  - Plaintext is `serde_json::to_vec`, held in `Zeroizing<Vec<u8>>`.
  - Data keys and derived keys are `Zeroizing`.
  - Document the layout in the crate docs.

- [ ] **Step 4: Run** `cargo test -p postit-vault --all-features` and clippy. Expected: PASS.

- [ ] **Step 5: Commit** `feat(vault): add the postit-vault crate with envelope-sealed values`.

---

### Task 4: Data: `platforms`, `vault_state`, the catalog, and the vault guard

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/Cargo.toml` (`postit-vault = { workspace = true, features = ["sqlx"] }`), `backend/crates/data/src/lib.rs`
- Create: `backend/crates/data/src/platforms.rs`, `backend/crates/data/src/vault.rs`
- Test: `backend/crates/data/tests/platforms.rs`, `backend/crates/data/tests/vault_state.rs`

**Interfaces:**
- Produces:
  - SQL:
    - `platforms`: `id TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_]{0,31}$')`, `display_name TEXT NOT NULL`, `enabled BOOLEAN NOT NULL DEFAULT TRUE`, `first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()`, `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`.
    - `vault_state`: `id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id)`, `newest_key_version INTEGER NOT NULL CHECK (newest_key_version > 0)`, `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`.
  - `pub struct PlatformRow { id: PlatformId, display_name: String, enabled: bool, first_seen_at: DateTime<Utc> }`
  - `pub struct PlatformRepo` (admin-level, takes `&PgPool`):
    - `upsert_registered(pool, &[(PlatformId, String)]) -> Result<(), DataError>`
    - `list(pool) -> Result<Vec<PlatformRow>, DataError>`
    - `set_enabled(pool, &PlatformId, bool) -> Result<bool, DataError>`, which returns whether a row matched.
  - `pub struct SealedColumn { pub table: &'static str, pub column: &'static str, pub id_column: &'static str }` and `pub const SEALED_COLUMNS: &[SealedColumn] = &[];`
  - `pub async fn vault_check_startup(pool: &PgPool, vault: &Vault, columns: &[SealedColumn]) -> Result<(), DataError>`. It raises `vault_state` when the vault's newest version is higher, fails with `DataError::VaultKeyBehind { configured, stored }` when it is lower, and fails with `DataError::VaultKeyMissing { version }` when `key_versions_in_use` returns a version the vault lacks.
  - `pub async fn key_versions_in_use(pool: &PgPool, columns: &[SealedColumn]) -> Result<BTreeMap<String, BTreeSet<u32>>, DataError>`, keyed `table.column`. It reads the version with `get_byte` on bytes 1..=4 of non-null values.
  - Table and column names are `&'static str` from the catalog only. Assert they match `^[a-z_]+$` before formatting them into SQL.
  - New `DataError` variants: `VaultKeyBehind { configured: u32, stored: u32 }`, `VaultKeyMissing { version: u32 }`, and `Vault(VaultError)` for sealing errors raised in data.

- [ ] **Step 1: Write failing tests:**

```rust
#[sqlx::test] async fn upsert_inserts_and_refreshes_without_touching_enabled() // insert tiktok, set_enabled false, upsert with new display_name -> enabled still false, display_name updated, first_seen_at unchanged
#[sqlx::test] async fn upsert_keeps_rows_not_in_registry()
#[sqlx::test] async fn check_rejects_invalid_ids() // raw INSERT 'Tik-Tok' and 'tik-tok' fail
#[sqlx::test] async fn vault_state_raised_then_lower_refused() // check with {1,2} -> stored 2; check with {1} -> VaultKeyBehind{configured:1, stored:2}
#[sqlx::test] async fn missing_version_in_use_refused() // scratch table with a v1-sealed value, catalog = [scratch], vault {2} -> VaultKeyMissing{version:1}
#[sqlx::test] async fn versions_in_use_reports_per_column()
```

Create the scratch tables inside each test with `CREATE TABLE scratch_secrets (id UUID PRIMARY KEY, secret BYTEA)`.

- [ ] **Step 2: Run** `cargo test -p postit-data --test platforms --test vault_state`. Expected: FAIL.

- [ ] **Step 3: Implement** the SQL, the repository, and the guard. `vault_check_startup` runs in one transaction under `xact_lock` with a new `VAULT_STATE_LOCK_KEY: i64 = 1_000_004` in `advisory_lock.rs`.

- [ ] **Step 4: Run** the tests. Expected: PASS.

- [ ] **Step 5: Commit** `feat(data): add platforms and vault_state tables with the vault startup guard`.

---

### Task 5: Data: the re-seal routine

**Files:**
- Create: `backend/crates/data/src/reseal.rs`
- Test: `backend/crates/data/tests/reseal.rs`

**Interfaces:**
- Consumes: `Vault::rewrap`, `Sealed::from_bytes`, `SealedColumn`, `vault_state`, and `VAULT_ROTATE_LOCK_KEY: i64 = 1_000_005` (add it in this task).
- Produces:
  - `pub struct ResealReport { pub columns: Vec<ColumnReport> }`
  - `pub struct ColumnReport { pub column: String, pub scanned: u64, pub resealed: u64, pub skipped: u64 }`. It derives `Serialize`, holds counts only, and `column` is `table.column`.
  - `pub async fn reseal(pool: &PgPool, vault: &Vault, columns: &[SealedColumn], batch_size: u32) -> Result<ResealReport, DataError>`.
  - `DataError::RotationInProgress` when the lock is held, and `DataError::VaultKeyBehind` when `vault_state` is newer than the vault.

- [ ] **Step 1: Write failing tests** using the scratch table from Task 4:

```rust
#[sqlx::test] async fn reseals_every_row_to_newest() // 1_200 rows sealed v1, batch 500, vault {1,2} -> all key_version 2, all open to original values, report resealed == 1200
#[sqlx::test] async fn nulls_and_newest_rows_are_skipped()
#[sqlx::test] async fn second_run_reseals_nothing()
#[sqlx::test] async fn concurrent_update_is_not_overwritten() // after the batch is read, an app write replaces one row with a fresh v2 value; final value opens to the app's value; counted skipped
#[sqlx::test] async fn rows_deleted_mid_run_are_skipped()
#[sqlx::test] async fn second_concurrent_run_refused() // hold the lock on another connection -> RotationInProgress
#[sqlx::test] async fn vault_behind_state_refused() // vault_state = 3, vault {1,2} -> VaultKeyBehind
```

To make the race tests deterministic, add `#[cfg(test)]`-free hooks only if needed. Otherwise use `reseal_batch` (a `pub(crate)` per-batch function, exposed to tests through a `testkit` feature) and interleave the steps by hand.

- [ ] **Step 2: Run** `cargo test -p postit-data --test reseal`. Expected: FAIL.

- [ ] **Step 3: Implement.**
  - Take the session advisory lock with `pg_try_advisory_lock`, not the blocking form.
  - For each column:
    - Select a batch with `SELECT {id}, {col} FROM {table} WHERE {id} > $cursor AND {col} IS NOT NULL ORDER BY {id} LIMIT $n`.
    - Call `rewrap` on each value.
    - Write with `UPDATE {table} SET {col} = $new WHERE {id} = $id AND {col} = $old`, where zero rows affected means skipped.
    - Advance the cursor.
  - Build the `SealContext` from the catalog entry and the row ID.

- [ ] **Step 4: Run** the tests. Expected: PASS.

- [ ] **Step 5: Commit** `feat(data): add the resumable vault re-seal routine`.

---

### Task 6: Server: agent registry, platform upsert, platform config, vault startup

**Files:**
- Create: `backend/crates/server/src/platforms.rs`
- Modify: `backend/crates/server/src/lib.rs` (startup), `backend/crates/server/src/compose.rs`, `backend/crates/server/Cargo.toml` (`postit-vault`, `postit-core`)
- Test: `backend/crates/server/src/platforms.rs` (unit), plus a startup test beside the existing server tests

**Interfaces:**
- Consumes: `AgentRegistry` and `ConnectMethod` from core, `OptionsSchema::validate`, `PlatformRepo`, `vault_check_startup`, `SEALED_COLUMNS`, and `Settings.platforms`.
- Produces:
  - `pub fn agent_registry() -> anyhow::Result<AgentRegistry>` in `compose.rs`. It returns an empty registry for now, with a comment saying B5 registers the mock agent here behind a feature.
  - `pub enum Availability { Available, NotCompiledIn, Disabled, NotConfigured }` (`snake_case` serde), and `pub fn availability(row: &PlatformRow, registry: &AgentRegistry, configs: &PlatformConfigs) -> Availability`. `NotConfigured` applies only when the agent uses `ConnectMethod::OAuth`, or its `config_schema()` has a required field, and it has no table. The function is public, so D3 can call it from `postit-server`'s API wiring, or move it into `postit-api` if D3 needs it there.
  - `pub struct PlatformConfigs(BTreeMap<PlatformId, serde_json::Value>)` with `get(&PlatformId) -> Option<&Value>`.
  - `pub fn resolve_platform_configs(settings: &Settings, registry: &AgentRegistry) -> anyhow::Result<PlatformConfigs>`:
    - an invalid ID is an error
    - a platform not compiled in gets `tracing::warn!`
    - each string value is converted using the field's `FieldKind`: integer by `trim().parse::<i64>()`, boolean by case-insensitive `true` or `false` after trimming, enum and date-time left as strings
    - the result is validated with `OptionsSchema::validate`
    - violations are an error listing `platforms.{id}.{field}: {code}` without values
  - `pub fn oauth_redirect_uri(public_url: &Url, id: &PlatformId) -> Url` returns `{public_url}/api/v1/platforms/{id}/oauth/callback`, keeping any base path.
  - Startup order in `open_database` (or a new `prepare_vault_and_platforms` called after it):
    - build `Vault::from_settings`
    - run `vault_check_startup(pool, &vault, SEALED_COLUMNS)` in every role
    - in API roles only, run `PlatformRepo::upsert_registered`, then log the availability of each non-available row and the redirect URI of each OAuth agent

    `Vault` is added to `Services` for later phases.

- [ ] **Step 1: Write failing tests.** Use a minimal fake `Agent` built from core's existing test fixtures (`core/src/agent/mod.rs` tests), or a local one with a credentials schema and an optional integer field `api_version` and boolean `sandbox`:

```rust
#[test] fn invalid_platform_table_id_is_an_error() // [platforms."Tik-Tok"]
#[test] fn unknown_platform_table_warns_and_is_dropped()
#[test] fn env_strings_coerced_by_schema() // {"api_version":"3","sandbox":" TRUE "} -> 3, true
#[test] fn uncoercible_value_errors_without_echo() // {"sandbox":"1"} and {"api_version":"x9"} -> Err mentions "platforms.fake.sandbox", not "1"/"x9"
#[test] fn availability_states() // available, disabled row, not compiled in, OAuth agent with no table -> NotConfigured, credential agent with optional schema and no table -> Available
#[test] fn redirect_uri_keeps_base_path() // https://h/x/ + "tiktok" -> https://h/x/api/v1/platforms/tiktok/oauth/callback
#[sqlx::test] async fn startup_with_no_agents_keeps_existing_rows() // pre-insert row "tiktok"; run the startup step with empty registry -> Ok, row present
#[sqlx::test] async fn startup_refuses_vault_behind_state()
```

- [ ] **Step 2: Run** `cargo test -p postit-server`. Expected: FAIL.

- [ ] **Step 3: Implement** the code above, following the error style of `compose.rs` (`anyhow::Context`).

- [ ] **Step 4: Run** `cargo test -p postit-server`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(server): build the agent registry, upsert platforms, and check the vault at startup`.

---

### Task 7: Server: the `vault_rotate` job

**Files:**
- Create: `backend/crates/server/src/vault_rotate.rs`
- Modify: `backend/crates/server/src/compose.rs` (`registry()` and `console_catalog()`), `backend/crates/data/src/audit.rs` (event kinds)

**Interfaces:**
- Consumes: `reseal`, `ResealReport`, `key_versions_in_use`, `AuditLog::record`, `AuditEvent`, `JobRegistry::register`, and `ConsoleSpec`.
- Produces:
  - `pub struct VaultRotate { pub requested_by_user_id: Uuid }` (`Serialize`, `Deserialize`), implementing `Job` with `JOB_TYPE = "vault_rotate"` and `QUEUE = Queue::Maintenance`.
  - `pub fn register(registry: &mut JobRegistry, pool: PgPool, vault: Vault, ids: Arc<dyn IdGenerator>) -> anyhow::Result<()>` with a default batch size of 500.
  - New `AuditEventKind` variants `VaultRotationStarted` and `VaultRotationCompleted`.
  - The completed event's details hold the per-column `resealed` and `skipped` counts and the `versions_in_use` after the run. They contain counts and version numbers only.
  - Console: `catalog.add::<VaultRotate>(ConsoleSpec::new().id_field("requested_by_user_id", "user_id"))` with retry allowed and no schedule. The builder checks the `id_field` signature against `console_spec.rs`.

- [ ] **Step 1: Write failing tests:**

```rust
#[sqlx::test] async fn rotate_job_reseals_and_audits() // scratch column passed in place of SEALED_COLUMNS through a test constructor; after handle(): rows on newest; audit_events has started + completed with counts
#[sqlx::test] async fn rotate_job_retry_is_idempotent() // handle twice -> second completed event resealed == 0
#[test] fn console_spec_allows_retry_and_has_no_schedule()
```

- [ ] **Step 2: Run** `cargo test -p postit-server vault_rotate`. Expected: FAIL.

- [ ] **Step 3: Implement.** The handler takes the column list from a field, which is `SEALED_COLUMNS` in production and the scratch column in tests. Errors map to the job's failure type with no values in the messages.

- [ ] **Step 4: Run** the tests. Expected: PASS.

- [ ] **Step 5: Commit** `feat(server): register the vault_rotate job`.

---

### Task 8: Operations, docs, vault files, and full gates

**Files:**
- Modify:
  - `stack.sh` and `stack.ps1` (the `doctor` key checks)
  - `docker/tests/stack-doctor.sh`
  - `deploy/env/qa.env.example` and `deploy/env/production.env.example`
  - the comments in `backend/config/qa.toml` and `backend/config/production.toml`
  - `docs/configuration.md`, `docs/security.md`, `docs/deployment.md`
  - `docs/plans/01. vision and architecture.md` (the `platforms.id` pattern drops `-`)
- Modify, never commit: `!ref/vault/qa/postit.env` and `!ref/vault/production/postit.env`

- [ ] **Step 1: Write failing doctor tests** in `docker/tests/stack-doctor.sh`, in the existing `expect` style:
  - each of `POSTIT__VAULT__KEYS__1`, `POSTIT__MEDIA__SIGNING_KEY`, and `POSTIT__EMAIL__SIGNING_KEY` missing for `qa`, so the doctor exits 1
  - each set to a 31-character value, so it exits 1
  - all present at 32 characters or more, so it exits 0

  Update the test's generated `postit.env` fixtures (lines 17 and 105) to include the three keys.

- [ ] **Step 2: Run** `bash docker/tests/stack-doctor.sh`. Expected: the new cases FAIL.

- [ ] **Step 3: Implement** the checks in `stack.sh` (beside the `POSTIT__AUDIT__PSEUDONYM_KEY` check) for non-development environments. Any `POSTIT__VAULT__KEYS__<n>` key satisfies the vault requirement. Mirror the checks in `stack.ps1`.

- [ ] **Step 4: Run** `bash docker/tests/stack-doctor.sh`. Expected: PASS.

- [ ] **Step 5: Docs.**
  - `docs/configuration.md`: one row per new key in the existing table format, including the secrets, the "0 means unlimited" limits, and `POSTIT__PLATFORMS__<ID>__<FIELD>` with `_FILE`.
  - `docs/security.md`: sealed storage, the associated-data binding, and the operator-trust statement from plan 01.
  - `docs/deployment.md`:
    - backing up every key version together with the signing keys, apart from the database backup
    - the rotation procedure: add the new version everywhere, restart all processes, enqueue `vault_rotate`, check `versions_in_use` in the completed audit event, remove the old version, restart
    - why a lower key version refuses to start
  - The env examples get commented `POSTIT__VAULT__KEYS__1_FILE`, `POSTIT__MEDIA__SIGNING_KEY_FILE`, and `POSTIT__EMAIL__SIGNING_KEY_FILE` lines.

- [ ] **Step 6: Vault files.**
  - Generate three distinct random keys per environment with `openssl rand -base64 48`.
  - Append `POSTIT__VAULT__KEYS__1`, `POSTIT__MEDIA__SIGNING_KEY`, and `POSTIT__EMAIL__SIGNING_KEY` to the extracted `!ref/vault/qa/postit.env` and `!ref/vault/production/postit.env`.
  - Do not stage them. `git status` must show no `!ref/vault/` path.
  - Run `./stack.sh doctor qa` to confirm the files pass. Use the doctor's own invocation; check `./stack.sh help`.
  - Tell the maintainer to re-pack `vault.7z`.

- [ ] **Step 7: Full gates** from `backend/`:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
cargo xtask openapi --check
```

Expected: all pass. The OpenAPI output is unchanged because A2 adds no routes.

- [ ] **Step 8: Commit** `docs: document vault keys, rotation, and plan 03 settings`, plus `chore(stack): require vault and signing keys outside development` if the scripts are committed separately.
