# A2: Vault, platforms table, and configuration (design)

Date: 2026-10-19. Source: plan 03 (`!ref/plans/03. publishing roadmap.md`), phase A2, plus the sections it links to: Media, Publishing, Sending an email campaign, Notifications and push, Jobs added by this plan, and Audit events added by this plan. Plan 01 covers the data model (`platforms`, sealed columns), the crate dependency order (`vault` depends on `config`, and `data` depends on `vault`), backups, and the no-secret-leak rule. This document does not repeat the plan. It records intent, the decisions the plan left open, and what the builder decides.

The user skipped the question round. Every decision below is the designer's call unless the plan states it, and each call is marked.

## Intent

Every phase after A2 that stores a secret, including OAuth tokens (B7), pasted credentials (B3), PKCE verifiers, AI keys (D2), and push endpoints (D1), stores it through one sealing primitive. Every agent reads its deployment config through one path. Every later phase reads its settings from sections that already exist and are already validated. A2 builds those three foundations before any table needs them. A sealing mistake here exposes every user's tokens. A config mistake here shows up as a confusing startup failure on a self-hoster's VPS.

## Goals

- Everything under plan 03 phase A2, meeting its exit criteria:
  - Sealed values round-trip.
  - Rotation re-seals every row.
  - A row's ciphertext cannot be opened under another row's ID or column.
  - Secrets never appear in `Debug`, errors, or logs.
  - Config validation reports errors the operator can act on.
- The composition root builds an `AgentRegistry` (empty in this phase; B5 adds the mock agent), upserts `platforms` from it at startup, and validates every `[platforms.<id>]` table against the owning agent's `config_schema()`.
- QA and production refuse to start without the required keys. `stack doctor`, CI smoke, the deploy examples, and `docs/configuration.md` all know about the new keys.

## Non-goals

- No owned tables with sealed columns. Those arrive in B2 (`account_grants` and the rest) and later phases. A2 ships the column type, the re-seal routine, and an empty sealed-column catalog that B2 fills.
- No `POST /admin/vault/rotate` route (D3) and no `GET /platforms` route (D3). A2 registers the `vault_rotate` job only.
- No `app_settings`, `user_limits`, or `platform_app_quota_usage` tables (B2). The `[publishing]`, `[limits]`, and similar sections are validated config that nothing reads yet. They follow the existing "Reserved: validated at load, not yet read" pattern of `RetentionSettings` and `OpsSettings`.
- No `PgCredentialStore` (B3 and B7). No real agents.

## Anti-goals

- A sealed value that decrypts after being copied to another row, another column, or another table.
- A plaintext secret in a `Debug` print, a `Display` output, an error message, a log line, or `Settings::redacted_dump()`. This includes the opaque `[platforms.<id>]` tables, which can hold client secrets.
- Rotation that loses or corrupts a value when it races an application write.
- Hand-rolled cryptography. Only maintained RustCrypto crates are used.
- A dev-only key accepted in QA or production.

## Constraints

- Rust 2024, workspace lints, workspace-level dependencies, `#![warn(missing_docs)]` where the crate already uses it.
- Dependency order (plan 01): `postit-vault` depends on `postit-config` only (and `postit-core` if it needs ID types). `postit-data` gains `postit-vault`. Nothing in `vault` depends on `data`.
- Single migration policy: the `platforms` table goes into `server/crates/data/migrations/0001_init.sql`, not a new file.
- Prefer maintained libraries. Never use a dependency with an Arabic or Arabic-derived name.
- Existing config keys, env var names, and public `Settings` fields keep working.

## Decisions

### `postit-vault` crate (`server/crates/vault/`)

**Algorithms** (designer's call within the plan's XChaCha20-Poly1305):

- `chacha20poly1305` (`XChaCha20Poly1305`) and `hkdf` (HKDF-SHA256), both RustCrypto. Their versions must line up with the workspace's `sha2 0.10`, `hmac 0.12`, and `rand_core 0.6`. `zeroize` covers plaintext and key buffers.
- **Key-encryption keys (KEKs).** Each configured key is opaque material of at least 32 bytes. The 32-byte KEK is derived with HKDF-SHA256 (salt: none; info: `postit vault kek v{version}`). Operators can then paste any strong random string, such as `openssl rand -base64 48`, without a base64 rule to get wrong.
- **Envelope.** `seal` generates a random 32-byte data key (DEK) for each value:
  - Plaintext is encrypted under the DEK with a random 24-byte nonce.
  - The DEK is wrapped under the newest KEK with its own random nonce.
  - Both encryptions use the same associated data (AAD).
  - Rotation only re-wraps the DEK, and the payload ciphertext stays as it is.
- **Associated data.** A length-prefixed encoding of a domain tag (`postit.vault.v1`), the table, the column, and the row ID, so no two different contexts encode to the same bytes. The AAD is passed in as `SealContext { table: &'static str, column: &'static str, row: Uuid }` and is never stored.
- **Storage format.** One `bytea` value, so B2 adds one column per secret. Layout:
  - format byte `1`
  - KEK version (`u32`, big-endian)
  - wrap nonce (24 bytes)
  - wrapped DEK (32 bytes plus a 16-byte tag)
  - data nonce (24 bytes)
  - ciphertext plus tag

  The builder may reorder the fields but must document the layout in the crate and pin it with a test vector, using fixed key and nonce inputs through a test-only constructor.
- **Typed values.** `Sealed<T>` wraps the bytes with `PhantomData<T>`. `Vault::seal<T: Serialize>(&T, &SealContext) -> Result<Sealed<T>>` and `Vault::open<T: DeserializeOwned>(&Sealed<T>, &SealContext) -> Result<T>` use JSON (`serde_json`) as the plaintext encoding, and the intermediate buffer is zeroized. Callers decide whether `T` is a `SecretString` or a struct of secrets.
- **`Sealed<T>` must not leak.** Its `Debug` prints `Sealed(kek v{n}, {len} bytes)` and never the bytes. It has no `Display` and no `Serialize`, so it can never reach an API DTO by accident.
- **`sqlx` support.** The `sqlx` impls (`Type`, `Encode`, `Decode` as `bytea`) sit behind a `sqlx` feature of `postit-vault`, which `postit-data` enables. The orphan rule keeps them out of `data`.
- **Errors** (`VaultError`, using `thiserror`):
  - unknown key version
  - malformed envelope
  - authentication failed (wrong row, column, or key)
  - serialization

  The messages carry versions and lengths only, never bytes, keys, or plaintext.
- **`Vault` construction.** `Vault::from_settings(&VaultSettings)` builds the vault and rejects:
  - an empty key list
  - a key shorter than 32 bytes
  - a duplicate version
  - version `0`

  The newest version is the highest number.
- **`Vault` must not leak.** Its `Debug` lists the key versions only.

### Rotation

- `postit-data` owns a sealed-column catalog: `SealedColumn { table, column, id_column }` and `pub const SEALED_COLUMNS: &[SealedColumn]`, empty in A2. B2 and later phases add one entry for each sealed column they create. A test in each later phase asserts that every `bytea` column whose name the catalog lists exists. The builder chooses how B2 keeps the catalog honest, and it is noted here only so B2 inherits it.
- `reseal(pool, vault, columns, batch_size) -> ResealReport` processes each column in batches of `batch_size`. Each batch selects rows in `id` order after a cursor and re-wraps every value whose KEK version is older than the newest. Each row is updated with a compare-and-swap: `UPDATE … SET col = $new WHERE id = $id AND col = $old`.
  - If the row changed in the meantime, the compare-and-swap finds nothing. The concurrent writer has already sealed the value under the newest key, so the row is counted as skipped.
  - Null values are skipped.
  - The routine is resumable because it is idempotent. Running it again after a crash only touches values that still use an old version.
  - The report lists, for each column, how many rows were scanned, re-sealed, and skipped. It never contains values.
- A Postgres advisory lock (from the existing `advisory_lock.rs`) allows only one rotation at a time.
- **`vault_rotate` job.** `postit-server` registers it through `postit-jobs`, with an ID-only `JobSummary` that holds the admin user ID. There is no cron schedule; it is enqueued by an admin, through the D3 route.
  - The job writes the `vault_rotation_started` and `vault_rotation_completed` audit events, with counts only.
  - It runs on the `maintenance` queue.
  - Retrying from the console is allowed because the routine is idempotent. The builder confirms this against the job console's `ConsoleSpec`.
- **Key-list consistency is enforced (designer's call).** Otherwise, a process still running with an older key list (for example, an API replica not yet restarted) could seal new rows under an old version after rotation, and those rows would be lost once that version is removed. `postit-data` keeps a one-row `vault_state` table (`newest_key_version INT NOT NULL`, in `0001_init.sql`), and there are two checks:
  - At startup, every process (API and worker) compares its newest configured version with the stored one:
    - If its version is higher, the stored value is raised to it.
    - If its version is lower, the process refuses to start with an error that names both versions.
  - `vault_rotate` re-reads `vault_state` before it starts and refuses to run if its own newest version is lower.
- **Removing an old key.** `vault_key_versions_in_use(pool, columns)` returns the KEK versions still present in each sealed column. The rotation report includes it, and an operator may remove a version from config only when no column uses it. At startup, a process that finds a version in use that it has no key for fails with an error naming that version, so a key is never removed silently. **Designer's call:** this check scans the sealed columns. While the catalog is empty (A2), the scan costs nothing. If the scan becomes slow once B2 fills the tables, the builder of that phase may replace it with a per-version count kept up to date.
- **Restart order** (documented in `docs/deployment.md`): to rotate, add the new key version to every process, restart all of them, then enqueue `vault_rotate`, then remove the old version and restart again.

### `platforms` table and startup upsert

- The `platforms` table, in `0001_init.sql`:
  - `id TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_]{0,31}$')`
  - `display_name TEXT NOT NULL`
  - `enabled BOOLEAN NOT NULL DEFAULT TRUE`
  - `first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()`
  - `updated_at TIMESTAMPTZ NOT NULL DEFAULT now()`

  **Designer's call:** plan 01 allows `-` in the ID, but `PlatformId` from A1 rejects it. The CHECK follows `PlatformId`, the stricter rule, so no row can exist that core cannot parse. Plan 01 should be corrected to match.
- `PlatformRepo` (admin-level, not owned content) in `postit-data`:
  - `upsert_registered(&[(PlatformId, display_name)])` inserts new rows and refreshes `display_name` and `updated_at` on existing ones. It never changes `enabled`.
  - `list()` and `set_enabled()` exist for D3.
- **Startup.** After migrations, the API process upserts from the registry. **Designer's call:** only the API process does this, not the worker, so two processes never race at boot (the upsert is idempotent anyway). Rows whose agent is not compiled in stay.
- **Availability.** A pure function in `postit-server` computes it: `available`, `not_compiled_in`, `disabled`, or `not_configured` (an OAuth agent with no `[platforms.<id>]` table). At startup it is logged once per non-available platform. D3 exposes the same function to the API. **Designer's call:** where that function lives is the builder's choice, as long as D3 can reach it without a dependency cycle.

### `[platforms.<id>]` configuration

- In `postit-config`, `Settings.platforms` is a `BTreeMap<String, PlatformTable>` (default empty). `PlatformTable` holds the raw table as an opaque `serde_json::Value`. Core does not know its shape.
- **Designer's call (redaction):** `PlatformTable`'s `Serialize` and `Debug` print each key with the value `"[redacted]"`. The config crate cannot know which fields are secret, so it redacts them all. This keeps `redacted_dump()` safe for client secrets.
- Secrets use the existing `_FILE` mechanism, for example `POSTIT__PLATFORMS__TIKTOK__CLIENT_SECRET_FILE`. The builder checks that the loader's `_FILE` handling reaches nested map keys and adds a test.
- **Env values get their types from the agent's schema (designer's call).** The loader never guesses types, so a value set through an env var or a `_FILE` arrives as a string. Before validating a platform table, the composition root converts each string value to the type that `config_schema()` declares for its field: integer, boolean, enum, or date-time. A string that cannot be converted is a validation error that names the field, never the value. The conversion applies only to platform tables, and the loader's behavior for the other sections does not change.
- **Startup check, in the composition root:**
  - A table whose key is not a valid `PlatformId` is an error.
  - A table for a platform that is not compiled in is a warning.
  - A table for a compiled-in agent is validated against `agent.config_schema()` using core's schema validation. A violation is an error that names the platform and field path, never the value.
  - An agent with no table is `not_configured` only if it connects through OAuth or its `config_schema` has required fields (designer's call). A credential agent with an optional schema, or with no schema at all, is available with no table, as the plan says.
- The validated tables are kept as a `BTreeMap<PlatformId, serde_json::Value>` for building agent contexts later, in B5 and B7.
- `postit-server` derives OAuth redirect URIs from `server.public_url` (`{public_url}/api/v1/platforms/{id}/oauth/callback`) through one helper. They are logged at startup for each OAuth agent. Showing them to admins is D3.

### Configuration sections

All sections are added to `Settings`, have defaults in `default.toml` (except secrets), use `deny_unknown_fields`, and are checked by `validate`, including duration bounds. Defaults come from the plan where it gives them. The others are designer's calls, marked †.

| Section | Keys (default) |
|---|---|
| `[vault]` | `keys`: map of version → secret (`RedactedSecret`), for example `[vault.keys] 1 = "…"`; env `POSTIT__VAULT__KEYS__1` or `POSTIT__VAULT__KEYS__1_FILE` † |
| `[publishing]` | `live` (true), `max_attempts` (5), `lease` (5min), `sweep_grace` (2min), `park_grace` (15min), `processing_timeout` (24h) |
| `[media]` | `storage_root` † (`/var/lib/postit/media`; development `../.data/media` †), `signing_key` (secret), `public_base_url` (unset means `server.public_url`), `single_request_max` (20 MB), `chunk_max` (16 MB), `upload_session_ttl` (24h), `view_url_ttl` (10min), `public_url_ttl` (1h), `user_quota_bytes` (20 GiB; 0 means unlimited), `fit_concurrency` (1), `fit_max_duration` (10min †), `ffmpeg_path` (`ffmpeg`) |
| `[oauth]` | `return_to_allowlist` (empty † means only `app.public_url` and the app's custom scheme are allowed; the builder decides the exact default rule and documents it) |
| `[ai]` | `request_timeout` (120s), `allow_private_endpoints` (false) |
| `[email]` | `signing_key` (secret), `batch_size` (50), `image_url_ttl` (365days), `tracking_base_url` (unset means `server.public_url`), `allow_private_hosts` (false) |
| `[push]` | `enabled` (false †), `vapid_private_key` (secret), `vapid_subject` (unset †), `allow_private_endpoints` (false) |
| `[limits]` | `email_daily_cap` (500 †; 0 means unlimited), `contacts_cap` (10000 †; 0 means unlimited), `storage_quota_bytes` (unset means `media.user_quota_bytes`; 0 means unlimited). These are defaults copied into each new user's `user_limits` row (B2), and admins edit them per user (D3). |
| `[recurrence]` | `lead` (10min) |
| `[inbox]` | `active_interval` (5min), `idle_interval` (1h) |
| `[retention]` | existing keys, plus `inbox_items` (180days), `direct_sends` (180days), `email_events` (180days) |
| `[legal]` | `privacy_path`, `terms_path`, `data_deletion_path` (all optional paths, unset by default) |

**Validation rules:**

- The vault rules from `Vault::from_settings`, so a bad key fails at config load with the key's name, not later.
- In QA and production, `vault.keys` must be non-empty, and `media.signing_key` and `email.signing_key` must be present, at least 32 bytes long, and not blank. When `push.enabled` is set, `push.vapid_private_key` and `push.vapid_subject` are also required.
- In every environment, a signing key that is present must be at least 32 bytes. Media and email signing keys must differ from each other and from `audit.pseudonym_key` (designer's call).
- **Designer's call:** outside development, startup refuses any value equal to one of the shipped development keys, which are published in the repository. The check compares against constants exported by `postit-config`.
- Each error names the full key and the env var that sets it, such as `POSTIT__VAULT__KEYS__1`, matching the style of the existing messages.

**Development:** `development.toml` ships fixed, clearly marked insecure keys for `vault.keys.1`, `media.signing_key`, and `email.signing_key`, so sealed values survive restarts and the dev stack and CI smoke (`POSTIT_ENV=development`) need no vault change. **Designer's call:** the plan names only the vault key. Doing the same for the two signing keys keeps development to a single rule.

**Operations:**

- `stack.sh` and `stack.ps1` `doctor` require `POSTIT__VAULT__KEYS__1` (or any `POSTIT__VAULT__KEYS__*`), `POSTIT__MEDIA__SIGNING_KEY`, and `POSTIT__EMAIL__SIGNING_KEY` in the QA and production `postit.env`, each at least 32 characters. Add cases to `docker/tests/stack-doctor.sh`.
- Update `deploy/env/*.env.example`, the qa and production toml comments, and `docs/configuration.md` (one row per new key, in the existing table format).
- Update `docs/security.md` and `docs/deployment.md`: back up every key version; the operator can decrypt (plan 01, "Backups" and the trust statement); how to rotate.
- **Claude adds the generated keys to the extracted `!ref/vault/qa/postit.env` and `!ref/vault/production/postit.env`, never commits them, and tells the maintainer to re-pack `vault.7z`.**

### Testing

- **Vault unit tests:**
  - round trip
  - wrong row, wrong column, or wrong table fails
  - truncated or tampered envelope fails
  - unknown version fails
  - an old version opens and the newest seals
  - the format test vector
  - `Debug` and errors contain no plaintext or key bytes (using the existing no-secret-leak harness from P10 where it fits)
  - zero-length and large plaintexts
- **Re-seal tests (database):** they use a scratch table created by the test, not the migration.
  - Every row ends on the newest version, and the values still open.
  - Null values are skipped.
  - A concurrent update during rotation is not overwritten.
  - A second run re-seals nothing.
  - The advisory lock blocks a second concurrent run.
  - A process whose newest key version is lower than `vault_state` refuses to start, and so does `vault_rotate`.
  - A version still used by a column but missing from config fails startup with that version named.
- **Config tests:**
  - every new section loads from defaults
  - `_FILE` secrets reach `vault.keys.N` and `platforms.<id>.*`
  - QA and production refusals for each missing key
  - the dev-key refusal outside development
  - key-length and key-distinctness rules
  - `redacted_dump` hides `vault.keys`, signing keys, and every `platforms` value
- **Platform tests:**
  - the upsert inserts, refreshes `display_name`, keeps `enabled`, and keeps rows that are not compiled in
  - the CHECK rejects invalid IDs
  - the availability function covers each state, including a credential agent with an optional schema and no table, which must be available
  - a platform integer or boolean field set by env var is converted and passes validation, and a field that cannot be converted fails without echoing the value
  - a `[platforms.x]` table with an invalid ID, an unknown platform, or a schema violation produces the right error or warning, using a test agent from core's existing test fixtures or a minimal fake agent

## Left to the builder

- Module layout inside `postit-vault`, the exact envelope field order, and the API naming beyond the shapes above.
- Whether `SealContext` takes `Uuid` or a typed ID trait.
- The batch size default for `vault_rotate` (around 500).
- The exact `[oauth] return_to_allowlist` default rule, documented.
- Where the availability function lives, as long as D3 can reuse it.
- Doc wording, as long as it follows the existing docs' tone and format.
