# D3 Publishing REST Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put thin HTTP handlers in front of the built publishing engine and content repositories, so an HTTP client can do the whole product flow (connect, import, upload, generate, create, validate, dry-run, schedule, publish, send a campaign, see history and notifications) as the owner and as a `publish` delegate.

**Architecture:** A `PostService` in `postit-publishing` owns post editing and shares one target check with `PublishingService::create_job`. Simple owned CRUD (brand profile, writing styles, target groups, campaigns) and admin settings are thin handlers over their `postit-data` repositories. `postit-api` gains `postit-publishing` as a dependency, maps `PublishingError`, a `ScheduleInput` resolved in `postit-core`, two new rate-limit buckets, and three deployment audit events; the clients get regenerated types and English catalog entries.

**Tech Stack:** Rust 2024 (axum, utoipa, sqlx, chrono-tz, emixcrypto), the existing `postit-api` testkit, `postit-agent-mock`, `postit-agent-email` testkit SMTP sink, i18next (web), gen-l10n ARB (mobile).

**Spec:** `docs/specs/2026-11-07-d3-publishing-rest-surface-design.md` (read it with plan 03's D3 section and REST surface table before starting).

## Global Constraints

- Thin handlers: parse, call one service or repository method, map the result. No ownership filter outside `postit-data`; no business rule in a handler.
- Every new route has the four-caller isolation tests (owner, a delegate at `view`/`edit`/`publish`, a delegate limited to other accounts, an unrelated user) against the Access column in plan 03's REST surface.
- Limit answers (spec "Constraints"): a single-account resource outside the limit (writing style, publish options, a delivery) is `404 not_found`; an account outside the limit named in a request body (post targets, publish targets, group members) is `403 account_not_in_scope`, checked up front with `OwnerScope::allows_account`; a visible object reaching hidden overrides or unfinished deliveries is `403 account_not_in_scope` (the repositories' `ensure_whole_post`).
- Rate limits are charged per actor after the route's capability check and before the work, as `routes/ai.rs` does.
- A field rule the database enforces answers `422 validation_failed` with `field`, never 500: name the `CHECK` and translate it in `data/src/constraint.rs` (Task 5 adds `DataError::Invalid`).
- Every new problem code is in `ErrorCode`, in OpenAPI, and in both clients' English error maps.
- Audit, logs, and problem details carry IDs and counts only; never post text, addresses, tokens, or signed URLs.
- Schema changes edit `backend/crates/data/migrations/0001_init.sql` in place (single-migration policy); rerun `cargo sqlx prepare --workspace` after query changes.
- CI never publishes live: tests use the mock agents, the mock OAuth provider, wiremock, and the in-process SMTP sink.
- Build directory outside the project: `CARGO_TARGET_DIR=K:/cargo-target/postit` (Windows path form). DB-backed tests read `DATABASE_URL` from the extracted development vault (`!ref/vault/development/postgres.env`).
- Quality gates at every task end that touches Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test -p <crates touched>`; the full `cargo test --workspace` and `cargo xtask openapi --check` in Task 14.
- American English in code comments, docs, and catalog strings. Commits end with the attribution trailer of the model doing the work.
- pnpm only for web tooling (PowerShell on Windows); Flutter commands through PowerShell.

## Review Focus

1. **A limited delegate edits a post that also targets a hidden account.** Expected, per the repositories' `ensure_whole_post` (`data/src/posts.rs`): a hidden target alone does not block the edit, and a `PATCH` that replaces `targets` keeps the hidden target; a hidden **override** or an unfinished delivery on a hidden account refuses `PATCH`, `DELETE`, and override writes with 403 `account_not_in_scope`. Tests in Task 7: `hidden_target_alone_allows_patch_and_survives_set_targets` and `hidden_override_blocks_patch_and_delete`.
2. **A stale `If-Match` after an override write.** `PUT …/overrides/{account_id}` bumps the post version; a publish with the `ETag` from before it must answer 412 `version_conflict`, and the override response's `ETag` must work. Test in Task 9: `publish_with_etag_from_before_override_is_412`.
3. **Local schedule time with no zone for an actor who never saved preferences.** Expected: the zone defaults to `UTC` (the column default), not an error. Test in Task 1 (`resolve_local_without_zone_uses_default`) and Task 9 (`publish_local_time_without_preferences_row_uses_utc`).
4. **Calendar boundaries.** The range is half-open `[from, to)`: a job at exactly `to` is excluded, one at `from` included; 62 days is accepted, 62 days plus one second refused. Test in Task 10: `calendar_range_is_half_open_and_capped`.
5. **Another owner's media ID in `POST /posts`.** Expected: 404 `not_found` (`PostRepo::set_media` inserts through an owner-filtered `SELECT` and reports the missing row), never 500, and no hint the asset exists; no post row is left behind. Test in Task 7: `create_post_with_foreign_media_is_404`.

---

### Task 1: Schedule input in core

**Files:**
- Create: `backend/crates/core/src/validation/schedule.rs`
- Modify: `backend/crates/core/src/validation/mod.rs` (export), `backend/crates/core/src/validation/violation.rs` (three codes), `backend/crates/core/Cargo.toml` (`chrono-tz.workspace = true`)

**Interfaces:**
- Produces:
  - `pub enum ScheduleInput { Instant(DateTime<FixedOffset>), Local { local: NaiveDateTime, zone: Option<String> } }`: serde untagged: a string parses as RFC 3339 *with* an offset only; an object is `{ "local": "YYYY-MM-DDTHH:MM[:SS]", "zone"?: "Area/City" }`. `utoipa::ToSchema` behind the `openapi` feature (a `oneOf` of string date-time and the object).
  - `pub fn resolve_schedule(input: &ScheduleInput, default_zone: &str) -> Result<DateTime<Utc>, Violation>`
  - `ViolationCode::ScheduleTimeNonexistent` (`"schedule_time_nonexistent"`, params `local`, `zone`), `ViolationCode::ScheduleTimeAmbiguous` (`"schedule_time_ambiguous"`, params `local`, `zone`, `earliest_offset`, `latest_offset` as `+HH:MM`), `ViolationCode::ScheduleZoneUnknown` (`"schedule_zone_unknown"`, param `zone`). All `.at("schedule")`.

- [ ] **Step 1: Write the failing tests** in `schedule.rs`'s test module:
  - `resolve_instant_keeps_the_offset`: `"2026-11-08T09:00:00+02:00"` resolves to `2026-11-08T07:00:00Z`.
  - `string_without_offset_does_not_parse`: `serde_json::from_str::<ScheduleInput>("\"2026-11-08T09:00:00\"")` is an error.
  - `resolve_local_with_zone`: `{ "local": "2026-11-08T09:00", "zone": "Europe/Berlin" }` resolves to `2026-11-08T08:00:00Z`.
  - `resolve_local_without_zone_uses_default`: zone `None`, default `"UTC"`, `2026-11-08T09:00` resolves to `09:00Z`.
  - `dst_gap_is_nonexistent`: `2026-03-29T02:30` in `Europe/Berlin` gives `ScheduleTimeNonexistent` with params `local` and `zone`.
  - `dst_overlap_is_ambiguous`: `2026-10-25T02:30` in `Europe/Berlin` gives `ScheduleTimeAmbiguous` with `earliest_offset == "+02:00"`, `latest_offset == "+01:00"`.
  - `unknown_zone`: `"Mars/Olympus"` gives `ScheduleZoneUnknown` with `zone`.
  - `violation_codes_are_listed`: the three new codes appear in `ViolationCode::ALL` with the strings above; update the existing count assertion in `violation.rs`'s tests (`assert_eq!(ViolationCode::ALL.len(), 33)`) to 36.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p postit-core schedule`
Expected: FAIL (module and codes not defined).

- [ ] **Step 3: Implement** `ScheduleInput` and `resolve_schedule` with `chrono_tz::Tz::from_str` and `LocalResult::{Single, Ambiguous, None}`; add the three codes to the `violation.rs` macro list with doc comments naming their parameters. Export `pub fn parse_zone(zone: &str) -> Option<chrono_tz::Tz>` too, and switch `api/src/routes/me.rs`'s own `parse::<chrono_tz::Tz>()` timezone check to it, so one function decides what a valid zone is.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/core
git commit -m "feat(core): resolve schedule input with IANA zones and DST violations"
```

---

### Task 2: Publishing errors and the service in the API state

**Files:**
- Modify: `backend/crates/api/Cargo.toml` (`postit-publishing.workspace = true`), `backend/crates/api/src/error.rs`, `backend/crates/api/src/state.rs`, `backend/crates/api/src/testkit.rs`, `backend/crates/server/src/compose.rs` (pass the already-built `publishing` into `AppState`)
- Test: `backend/crates/api/src/error.rs` (unit tests)

**Interfaces:**
- Consumes: `postit_publishing::{PublishingService, PublishingError}`.
- Produces:
  - `ErrorCode` variants: `ConfirmationRequired` (400, `confirmation_required`), `AcknowledgementRequired` (400, `acknowledgement_required`), `NotReschedulable`, `NotRetryable`, `NotResolvable`, `PostLocked` (409; `not_reschedulable`, `not_retryable`, `not_resolvable`, `post_locked`), `RotationInProgress` (409, `rotation_in_progress`).
  - `impl From<PublishingError> for ApiError` with the table in the spec ("Error mapping"); `ValidationFailed(v)` carries `violations` the way `From<AccountError>` already does (`with_extension("violations", …)`); `Data(e)` delegates to the existing `From<DataError> for ApiError`, which gains `DataError::RotationInProgress => RotationInProgress`.
  - `AppState.publishing: postit_publishing::PublishingService`.
  - The testkit's `TestApp` builds a `PublishingService` over its pool, agents, accounts, media, notifier, and queue (the same `compose::publishing` inputs). The testkit runs on `SystemClock` and no worker, so tests never advance a clock; they move due times in SQL and run jobs by hand, as D1's tests do (`publishing/tests/common`: `make_due`, `run_publish`). Two helpers (used by the API tests of Tasks 9 and 10; Task 13 runs a real worker instead):
    - `TestApp::make_job_due(&self, job: PublishJobId)`: sets the job's `pending` deliveries' `next_attempt_at`, and the `run_at` of their `queued` `publishing` ledger rows, to `now() - 1 second`.
    - `TestApp::drain_publishing(&self) -> usize`: loop: select `jobs` ledger rows with `queue = 'publishing'`, `state = 'queued'`, `run_at <= now()`, whose ID it has not run yet; run each by `job_type` (`publish_delivery` → `run_publish`, `direct_send_batch` → `direct::run_batch`, `poll_delivery_status` → `poll::run_poll`, `reconcile_delivery` → `reconcile::run_reconcile`; any other type panics); remember the ID; stop when a pass finds no new row; panic after 50 passes. Returns the number of jobs run.

- [ ] **Step 1: Write the failing tests** `publishing_errors_map_to_their_codes`: for each `PublishingError` variant, `ApiError::from(e)` has the status and `code` from the spec table (`Data(DataError::Sql(..))`, `Notify`, `Corrupt`, `Jobs` give 500 `internal`; `Data(DataError::RotationInProgress)` gives 409 `rotation_in_progress`); and a testkit test `drain_runs_a_due_dry_run_job` (create a dry-run job through the service, `drain_publishing` returns at least 1, the delivery ends `published`).
- [ ] **Step 2: Run it**: `cargo test -p postit-api --lib error` → FAIL.
- [ ] **Step 3: Implement** the codes, the `From`, the state field, the server wiring, and the testkit construction, `make_job_due`, and `drain_publishing`.
- [ ] **Step 4: Run**: `cargo test -p postit-api --lib` and `cargo check -p postit-server --all-targets` → PASS.
- [ ] **Step 5: Commit**: `feat(api): map publishing errors and hold the publishing service`.

---

### Task 3: Rate-limit buckets and deployment audit events

**Files:**
- Modify: `backend/crates/config/src/settings.rs` (`RateLimitSettings` fields and `check_bucket` calls), `backend/crates/config/src/loader.rs` (the default TOML's `[rate_limit.*]` tables), `backend/config/development.toml` and the example config if it lists buckets, `backend/crates/api/src/state.rs` (`Limits::new` and `retain_recent`), `backend/crates/api/src/testkit.rs` (`default_settings`: both new buckets `generous()`), `backend/crates/api/src/routes/media.rs` (charge `uploads` in `upload` and `start_session`), `backend/crates/api/src/routes/accounts.rs` (charge `credential_checks` in `start_oauth`), `backend/crates/data/src/audit.rs`, `backend/crates/api/src/dto.rs` (`AuditEventKindDto`), `docs/configuration.md`
- Test: `backend/crates/config/src/loader.rs` tests, `backend/crates/api/tests/media.rs`, `backend/crates/api/tests/oauth.rs`, `backend/crates/data` audit tests

**Interfaces:**
- Produces: `rate_limit.uploads` (default `rate_per_minute = 30`, `burst = 10`), `rate_limit.publish_actions` (default `60`, `20`); `Limits.uploads: KeyedLimiter<UserId>`, `Limits.publish: KeyedLimiter<UserId>`; `AuditEventKind::{LivePublishingChanged => "live_publishing_changed", PlatformEnabledChanged => "platform_enabled_changed", UserLimitsChanged => "user_limits_changed"}` and their DTO mirrors.

- [ ] **Step 1: Write the failing tests**:
  - config: `uploads_and_publish_buckets_default` (the defaults above) and `zero_rate_is_refused_for_new_buckets` (like the existing `lookup` test).
  - `media.rs`: `upload_session_start_is_rate_limited` (with `uploads` set to burst 1 via `TestApp::start_with`, the second `POST /media/uploads` is 429 `rate_limited` with `Retry-After`) and `upload_chunks_are_not_charged` (burst 1: start one session, then two `PUT /media/uploads/{id}` chunks both succeed).
  - `oauth.rs`: `oauth_start_is_rate_limited` (burst 1 on `credential_checks`).
  - `media.rs`: `refused_upload_is_not_charged` (burst 1: a `view` delegate's `POST /media/uploads` is 403 `capability_required`; the same user's own next session start still succeeds), pinning the charge-after-capability order.
  - audit: the three kinds round-trip through `as_str` and the DTO list.
- [ ] **Step 2: Run them**: `cargo test -p postit-config -p postit-data -p postit-api --test media --test oauth` → FAIL.
- [ ] **Step 3: Implement**; document both buckets in `docs/configuration.md` next to the existing ones.
- [ ] **Step 4: Run the same command** → PASS.
- [ ] **Step 5: Commit**: `feat(api): rate-limit uploads and OAuth starts, add deployment audit kinds`.

---

### Task 4: Platforms and publish options

**Files:**
- Create: `backend/crates/api/src/dto_platforms.rs`, `backend/crates/api/src/routes/platform_catalog.rs` (signed-in routes; `routes/platforms.rs` stays the bearer-free routes a platform calls)
- Modify: `backend/crates/oauth/src/catalog.rs` (`PLATFORM_ORDER`, `catalog_rank`), `backend/crates/oauth/src/manage.rs` (`publish_options`), `backend/crates/api/src/routes/accounts.rs`, `backend/crates/api/src/routes/mod.rs`, `backend/crates/api/src/openapi.rs`
- Test: `backend/crates/api/tests/platforms.rs` (new)

**Interfaces:**
- Consumes: `AccountService::bound_client` (`oauth/src/service.rs`) and the agent client's `publish_options(&AccountRef)`; the catalog's `availability`; `PlatformRepo::list`.
- Produces in `postit-oauth`:
  - `AccountService::publish_options(&self, scope: &OwnerScope, account: AccountId) -> Result<OptionsSchema, AccountError>`: requires `Capability::Edit`; an account outside the limit or not the owner's is `AccountError::NotFound`.
  - `pub const PLATFORM_ORDER: &[&str]` in `catalog.rs`, plan 03's catalog order: `email`, `telegram`, `slack`, `discord`, `bluesky`, `mastodon`, `x`, `youtube`, `linkedin`, `facebook`, `instagram`, `threads`, `tiktok`; and `pub fn catalog_rank(id: &PlatformId) -> (usize, String)`: listed IDs by position, unlisted ones (`mock`, `mock_direct`) after them by ID. Descriptors do not change.
- Produces: `GET /api/v1/platforms` → `Vec<AgentDescriptor>` (availability `Available` only: compiled in, configured, enabled; sorted by `catalog_rank`); `GET /api/v1/platforms/{id}/icon` → `image/png`, `Cache-Control: public, max-age=86400`, strong `ETag` = `"<hex sha256 of bytes>"` via `emixcrypto`, `304` on match, `404` for an unlisted platform; `GET /api/v1/accounts/{id}/publish-options` → `OptionsSchema`, charged to `credential_checks`.
- Produces a shared helper in `backend/crates/api/src/preconditions.rs`: `pub fn if_none_match(headers: &HeaderMap, tag: &HeaderValue) -> bool` (strong comparison, `*` matches, list of tags), used by Tasks 7, 9, 10.

- [ ] **Step 1: Write the failing tests** in `tests/platforms.rs` with `TestApp::start_with_agents` (mock agents):
  - `platforms_lists_enabled_compiled_platforms` (contains `mock`; after `PlatformRepo::set_enabled(false)` it does not).
  - `platform_icon_is_png_with_cache_headers`, `platform_icon_304_on_matching_etag`, `icon_of_disabled_platform_is_404`.
  - `catalog_rank_orders_by_plan_03` (unit test in `catalog.rs`: `email` before `telegram`, both before `mock`, `mock` before `mock_direct`).
  - `publish_options_returns_the_agents_schema` (owner); the isolation matrix `publish_options_access` (view delegate 403 `capability_required`, edit delegate 200, limited delegate on another account 404 `not_found`, stranger 404).
  - `publish_options_is_rate_limited`.
  - Unit tests for `if_none_match`: exact tag, `*`, a list, a weak tag never matching.
- [ ] **Step 2: Run**: `cargo test -p postit-api --test platforms` → FAIL.
- [ ] **Step 3: Implement** the routes (`GET /platforms` and the icon accept act-as; descriptors are not owned data).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**: `feat(api): list platforms, serve icons, read publish options`.

---

### Task 5: Brand profile and writing styles

**Files:**
- Create: `backend/crates/api/src/dto_workspace.rs`, `backend/crates/api/src/routes/workspace.rs`
- Modify: `backend/crates/data/migrations/0001_init.sql` (name the checks `brand_profiles_examples_max` and `campaigns_dates_ordered`), `backend/crates/data/src/error.rs` (`DataError::Invalid`), `backend/crates/data/src/constraint.rs` (translate both), `backend/crates/data/src/brand_profiles.rs` and `campaigns.rs` (map write errors through `translate`), `backend/crates/data/src/writing_styles.rs` (`find`), `backend/crates/api/src/error.rs` (`From<DataError>`), `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/workspace_content.rs` (new), `constraint.rs`'s tests

**Interfaces:**
- Consumes: `BrandProfileRepo::{get, upsert}`, `WritingStyleRepo::{get, upsert}`. `WritingStyleRepo::get` is `NotFound` both for an account outside the limit and for one with no style, so the data layer gains `WritingStyleRepo::find(conn, scope, account) -> Result<Option<WritingStyle>, DataError>`: `NotFound` when the account is outside the limit, not the owner's, or disconnected; `Ok(None)` when it is visible and has no style. The `GET` handler calls `find` and answers the empty DTO for `None`.
- Produces in `postit-data`: `DataError::Invalid { field: &'static str }`; `constraint::translate` maps the check violation `brand_profiles_examples_max` to `Invalid { field: "examples" }` and `campaigns_dates_ordered` to `Invalid { field: "ends_on" }`. In `postit-api`: `DataError::Invalid { field }` → `422 validation_failed` with the `field` extension.
- Produces: `GET/PUT /api/v1/brand-profile` with `BrandProfileDto { id: Option<BrandProfileId>, description, audience, offerings, avoid_words, default_language, examples, updated_at: Option<DateTime<Utc>> }` (empty strings and lists with `id: null` when none was written); `GET/PUT /api/v1/accounts/{id}/writing-style` with `WritingStyleDto` mirroring the repository fields, same empty rule.

- [ ] **Step 1: Write the failing tests**: data `named_checks_translate_to_invalid` (both constraints); `brand_profile_is_empty_until_written`, `brand_profile_put_then_get`, `brand_profile_more_than_five_examples_is_422` (`validation_failed` with `field: "examples"`, from the database check behind `MAX_EXAMPLES`), `writing_style_round_trip`, `writing_style_is_empty_until_written`, and the isolation matrices `brand_profile_access` (view reads, edit writes, stranger sees their own empty profile, not the owner's) and `writing_style_access` (limited delegate on another account 404 `not_found` for both methods, stranger 404).
- [ ] **Step 2: Run**: `cargo test -p postit-api --test workspace_content` → FAIL.
- [ ] **Step 3: Implement** (edit the migration in place; rerun `cargo sqlx prepare --workspace`). A field rule the data layer does not enforce yet goes into the repository with its own data test, not into the handler.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**: `feat(api): brand profile and writing style routes`.

---

### Task 6: Target groups and campaigns

**Files:**
- Modify: `backend/crates/data/src/target_groups.rs` (`reachable` answers `AccountNotInScope`; new `replace`), `backend/crates/data/src/campaigns.rs` (new `patch`), `dto_workspace.rs`, `routes/workspace.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/workspace_content.rs`, the data crate's target-group and campaign tests (update any that assert `NotFound` for an account outside the limit)

**Interfaces:**
- Consumes: `TargetGroupRepo::{create, get, list, delete}`, `CampaignRepo::{create, get, list, archive, delete}`.
- Produces in `postit-data`:
  - `TargetGroupRepo::replace(conn, scope, id, name: &str, accounts: &[AccountId]) -> Result<TargetGroup, DataError>`: rename and `set_accounts` in one transaction (the existing `rename` and `set_accounts` stay for their callers).
  - `CampaignRepo::patch(conn, scope, id, patch: CampaignPatch) -> Result<Campaign, DataError>` with `CampaignPatch { name: Option<String>, starts_on: Option<Option<NaiveDate>>, ends_on: Option<Option<NaiveDate>>, color: Option<Option<String>> }`: one `UPDATE … SET name = COALESCE(…)` with a flag per nullable field, so an explicit `null` clears it; errors through `translate` (the `campaigns_dates_ordered` check from Task 5).
  - `TargetGroupRepo`'s `reachable` returns `DataError::AccountNotInScope` for an account outside the limit (spec "Constraints"); an account that is not the owner's stays `NotFound`.
- Produces: `GET/POST /api/v1/target-groups`, `PUT/DELETE /api/v1/target-groups/{id}` (`TargetGroupDto { id, name, account_ids, updated_at }`; `PUT { name, account_ids }` calls `replace`); `GET/POST /api/v1/campaigns` (`include_archived` query flag, default `false`, passed to `CampaignRepo::list`), `PATCH/DELETE /api/v1/campaigns/{id}` (`CampaignDto { id, name, starts_on, ends_on, color, archived, created_at, updated_at }`; `PATCH` fields all optional and call `patch`; `archived` present calls `archive` with its value, in the same handler after `patch` when both are sent). Lists answer the existing `Page<T>` DTO (`api/src/pagination.rs`) over the repositories' `ResultSet`.

- [ ] **Step 1: Write the failing tests**: `target_group_crud`, `limited_delegate_put_keeps_hidden_accounts` (owner's group has accounts A and B, delegate limited to A puts `[A]`; the owner still sees `[A, B]`, the delegate sees `[A]`), `limited_delegate_naming_hidden_account_is_403` (`account_not_in_scope`), `target_group_with_foreign_account_is_404`, `campaign_crud`, `campaign_patch_changes_only_sent_fields` (name only keeps the dates; `ends_on: null` clears it), `campaign_ends_before_start_is_422` (`field: "ends_on"`), `campaign_delete_unlinks_posts` (a post in the campaign survives with `campaign_id: null`), `campaign_include_archived_filter`, plus the isolation matrices `target_group_access` and `campaign_access` (view lists, edit writes, stranger 404).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** (rerun `cargo sqlx prepare --workspace`).
- [ ] **Step 4: Run** `cargo test -p postit-api --test workspace_content` and `cargo test -p postit-data` → PASS.
- [ ] **Step 5: Commit**: `feat(api): target group and campaign routes`.

---

### Task 7: `PostService` and the post routes

**Files:**
- Create: `backend/crates/publishing/src/posts.rs`, `backend/crates/api/src/dto_posts.rs`, `backend/crates/api/src/routes/posts.rs`
- Modify: `backend/crates/publishing/src/lib.rs`, `backend/crates/publishing/src/service.rs` (extract the target check), `backend/crates/data/src/posts.rs` (add `last_published`; `set_targets` answers `AccountNotInScope` for an account outside the limit), `routes/mod.rs`, `openapi.rs`, the testkit and server composition (build `PostService`)
- Test: `backend/crates/publishing/tests/posts.rs` (new), `backend/crates/api/tests/posts.rs` (new)

**Interfaces:**
- Consumes: `PostRepo::{create, get, list, lock, update_content, set_media, set_targets, upsert_override, delete_override, delete, targets, media, overrides}`, `AiGenerationRepo::link_post`. The whole-post rule is already inside those writes (`ensure_whole_post`: hidden overrides and unfinished deliveries block, a hidden target alone does not); `PostService` adds no limit check of its own and does not call `outside_limit`. `PostRepo::set_targets`' limit check changes from `NotFound` to `AccountNotInScope` (spec "Constraints"); `set_media` already answers `NotFound` for an asset that is not the owner's.
- Produces in `postit-publishing`:
  - `pub struct PostService` (cheap clone, over `Arc<PublishingDeps>`; `PublishingService::posts(&self) -> PostService` or a constructor; one owner for the target check either way).
  - `pub struct PostDraft { kind, reply_to: Option<Value>, campaign_id: Option<CampaignId>, text: String, link: Option<String>, tags: Vec<String>, disclose_ai_generated: bool, media: Vec<PostMediaInput>, targets: Vec<AccountId> }`, `pub struct PostMediaInput { media_id: MediaAssetId, alt_text: Option<String> }`, `pub struct PostPatch` (every `PostDraft` field as `Option`).
  - `pub struct PostView { post: Post, media: Vec<PostMediaItem>, targets: Vec<AccountId>, overrides: Vec<PostOverrideRow>, last_published: Vec<LastPublished> }`; `postit_data::posts::LastPublished { account_id, post_version: i64, published_at: DateTime<Utc> }` from `PostRepo::last_published(conn, scope, post) -> Result<Vec<LastPublished>, DataError>` (newest live `published` delivery per account, filtered by the limit).
  - `PostService::create(&self, scope, draft: PostDraft, generation: Option<AiGenerationId>) -> Result<PostView, PublishingError>` (edit; a target outside the limit is `AccountNotInScope`, a foreign media asset `NotFound`, both before commit so no post row is left; links the generation in the same transaction; a generation that is not the owner's or already linked is `NotFound`).
  - `PostService::get(&self, scope, id) -> Result<PostView, PublishingError>`, `PostService::list(&self, scope, Pagination, PostFilter) -> Result<ResultSet<Post>, PublishingError>` (the route wraps it in `Page<T>`).
  - `PostService::update(&self, scope, id, if_match: PostVersion, patch: PostPatch) -> Result<PostView, PublishingError>` (edit; the repositories' whole-post rule answers `AccountNotInScope`).
  - `PostService::delete(&self, scope, id, if_match: PostVersion) -> Result<(), PublishingError>`: `PostRepo::lock` then `PostRepo::delete`, which already requires `Capability::Publish` when the post has any non-dry-run job and `Edit` otherwise, and applies the whole-post rule; the service does not repeat either check.
  - `PostService::put_override(&self, scope, id, account, if_match, patch: Value, options: Value) -> Result<PostView, PublishingError>` and `delete_override(...)` (edit, account in limit).
  - In `service.rs`: `pub(crate) async fn check_targets_for(&self, scope, base, overrides, targets: &[AccountId], at: Option<DateTime<Utc>>, native_schedule: bool, dry_run: bool, now) -> Result<Vec<Target>, TargetCheck>` where the violations are returned per target instead of flattened, so Task 8 can report them by account; `create_job` flattens them into `ValidationFailed` as today.
- Produces in `postit-api`: `GET/POST /api/v1/posts`, `GET/PATCH/DELETE /api/v1/posts/{id}`, `PUT/DELETE /api/v1/posts/{id}/overrides/{account_id}`, each read and write answering `ETag: "v{version}"`; `GET /posts/{id}` answers `304` through `if_none_match`; writes require `If-Match` through `check_if_match` (428 without, 412 stale).

- [ ] **Step 1: Write the failing service tests** in `publishing/tests/posts.rs`: `create_links_generation`, `create_with_linked_generation_is_not_found`, `update_bumps_version_and_refuses_stale`, `delete_with_live_history_needs_publish` (an `edit` delegate gets `Scope`; a `publish` delegate succeeds), `delete_dry_run_only_history_needs_edit`, `put_override_bumps_version`, `last_published_reports_newest_live_delivery`, and `create_job_still_flattens_violations` (the refactor keeps D1 behavior).
- [ ] **Step 2: Write the failing route tests** in `api/tests/posts.rs`: `post_crud_round_trip`, `get_post_304_on_matching_etag`, `patch_without_if_match_is_428`, `patch_stale_if_match_is_412`, `patch_scheduled_post_is_409_post_locked`, `create_post_with_foreign_media_is_404` (and `GET /posts` lists nothing afterwards), `create_post_with_target_outside_limit_is_403`, `hidden_target_alone_allows_patch_and_survives_set_targets` (owner's post targets A and B, a delegate limited to A patches the text and puts `targets: [A]`: 200, and the owner still sees `[A, B]`), `hidden_override_blocks_patch_and_delete` (same post with an override on B: the delegate's `PATCH` and `DELETE` are 403 `account_not_in_scope`), `limited_delegate_sees_only_their_overrides`, and the isolation matrix `post_access` (view reads, edit writes, `publish` needed to delete a post with live history, stranger 404).
- [ ] **Step 3: Run**: `cargo test -p postit-publishing --test posts` and `cargo test -p postit-api --test posts` → FAIL.
- [ ] **Step 4: Implement** the repository query (rerun `cargo sqlx prepare --workspace`), the service, and the routes.
- [ ] **Step 5: Run both, plus `cargo test -p postit-publishing`** (D1's tests stay green after the refactor) → PASS.
- [ ] **Step 6: Commit**: `feat(publishing): post service and post routes with ETags`.

---

### Task 8: Validate routes

**Files:**
- Create: `backend/crates/api/src/routes/schedule.rs`
- Modify: `backend/crates/publishing/src/posts.rs`, `dto_posts.rs`, `routes/posts.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/posts.rs`, `backend/crates/publishing/tests/posts.rs`

**Interfaces:**
- Consumes: Task 7's `check_targets_for`, Task 1's `resolve_schedule`.
- Produces:
  - `pub struct ValidationReport { pub targets: Vec<TargetReport>, pub violations: Vec<Violation> }`, `pub struct TargetReport { pub account_id: AccountId, pub format: Option<FormatId>, pub violations: Vec<Violation> }`, `ValidationReport::valid(&self) -> bool`.
  - `PostService::validate_draft(&self, scope, draft: PostDraft, overrides: BTreeMap<AccountId, PostOverride>, at: Option<DateTime<Utc>>, native_schedule: bool) -> Result<ValidationReport, PublishingError>` (edit; targets in limit; a media ID that is not the owner's is a `media_missing` violation for every target, not `NotFound`).
  - `PostService::validate_post(&self, scope, id, targets: Option<Vec<AccountId>>, at, native_schedule) -> Result<ValidationReport, PublishingError>`.
  - `POST /api/v1/posts/validate` (body: the `POST /posts` body plus `overrides: { "<account_id>": { patch, options } }`, `at?: ScheduleInput`, `native_schedule?`), `POST /api/v1/posts/{id}/validate` (`{ targets?, at?, native_schedule? }`): both `200` with `{ valid, targets, violations }`; both charged to `limits.publish`. The handler resolves `at` with the actor's `user_preferences.timezone` (`UTC` when there is no row); a schedule violation lands in the top-level `violations`.
  - The one zone helper, defined here and reused by Task 9: `pub(crate) async fn actor_zone(state: &AppState, scope: &OwnerScope) -> Result<String, ApiError>` in `backend/crates/api/src/routes/schedule.rs` (new): `UserPreferencesRepo::get` for `scope.actor` (the delegate's own zone through act-as), `"UTC"` when there is no row.

- [ ] **Step 1: Write the failing tests**: `validate_draft_reports_per_target` (mock agent with a text limit; one target over it), `validate_reports_schedule_violation_at_top_level` (a DST gap), `validate_draft_foreign_media_is_violation`, `validate_post_defaults_to_post_targets`, `validate_matches_publish_verdict` (for three posts (valid, too long, missing media), `valid` equals whether a dry-run publish of the same post succeeds), `validate_is_rate_limited`, and the isolation matrix `validate_access` (view 403 `capability_required`, edit 200, limited delegate targeting another account 403 `account_not_in_scope`).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**: `feat(api): validate drafts and saved posts`.

---

### Task 9: Publish, jobs, and deliveries routes

**Files:**
- Create: `backend/crates/api/src/dto_publishing.rs`, `backend/crates/api/src/routes/publishing.rs`
- Modify: `backend/crates/data/src/publishing/jobs.rs` (`JobFilter` gains `campaign_id`, `dry_run`, `from`, `to`), `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/publishing.rs` (new), `backend/crates/data/tests/` (the filter)

**Interfaces:**
- Consumes: `PublishingService::{create_job, reschedule, cancel_job, cancel_delivery, retry, resolve}`, `JobRepo::{get, list, delivery_counts}`, `DeliveryRepo::{get, list_for_job}`, Task 1's `resolve_schedule`, Task 8's `routes::schedule::actor_zone`, Task 2's `TestApp::{make_job_due, drain_publishing}`. Service return types: `reschedule` takes `at: DateTime<Utc>` and `if_match: i64` (the handler resolves the `ScheduleInput` first; a schedule violation is a 422 `validation_failed` with it); `cancel_delivery` returns `CancelReport`, `retry` a `Dispatch`, `resolve` a `DeliveryState`.
- Produces:
  - `POST /api/v1/posts/{id}/publish` (`If-Match`, body `PublishRequest { targets, at?: ScheduleInput, native_schedule?: bool, dry_run?: bool, confirm?: bool }`) → `201 PublishJobDto` with `ETag: "v{job.version}"`.
  - `GET /api/v1/publish-jobs` (`status`, `post_id`, `campaign_id`, `dry_run`, `from`, `to`; paginated), `GET /api/v1/publish-jobs/{id}` (`PublishJobDto { id, post_id, post_version, scheduled_at, native_schedule, dry_run, status, version, cancelled_at, created_at, created_by, deliveries: Vec<DeliveryDto> }`, deliveries filtered by the limit, `304` on `If-None-Match`), `PATCH /api/v1/publish-jobs/{id}` (`If-Match` on the job, `{ at: ScheduleInput }` → `RescheduleOutcomeDto { moved, failed }`), `POST /api/v1/publish-jobs/{id}/cancel` → `CancelReportDto { cancelled, requested, not_cancellable }`.
  - `GET /api/v1/deliveries/{id}` → `DeliveryDto { id, job_id, post_id, account_id, operation, state, parked_reason, attempt_count, next_attempt_at, remote_id, remote_url, remote_publish_at, last_error: Option<{ code, class }>, direct: Option<{ queued, sent, failed, skipped }>, created_by }`; `POST /api/v1/deliveries/{id}/cancel` → `CancelReportDto` (the service's `CancelReport`, same DTO as the job cancel); `/retry` (`{ confirm, acknowledge_outcome_unknown? }`) and `/resolve` (`{ outcome: "published" | "failed" }`) → the delivery read back with `DeliveryRepo::get` after the service call, as `DeliveryDto`.
  - Every mutating route charges `limits.publish` and passes `RequestAudit` to the service.

- [ ] **Step 1: Write the failing tests** with the mock agents and `TestApp::drain_publishing`:
  - `live_publish_without_confirm_is_400`, `publish_with_etag_from_before_override_is_412`, `publish_violations_are_422_with_every_violation`, `dry_run_publish_simulates_and_leaves_status_draft`, `scheduled_publish_then_drain_publishes` (schedule two minutes ahead: `drain_publishing` runs nothing and the job stays `scheduled`; after `make_job_due` it runs the delivery and the job is `published`), `publish_local_time_without_preferences_row_uses_utc`, `delegate_zone_is_their_own` (owner in `Asia/Tokyo`, delegate in `Europe/Berlin`, a local time through act-as resolves in Berlin).
  - `get_job_304_on_matching_etag`, `job_list_filters_by_campaign_and_dry_run`, `limited_delegate_sees_only_their_deliveries`.
  - `reschedule_pending_job`, `reschedule_started_job_is_409_not_reschedulable`, `cancel_job_reports_each_delivery`.
  - `retry_without_confirm_is_400`, `retry_outcome_unknown_without_ack_is_400_acknowledgement_required`, `retry_published_delivery_is_409_not_retryable`, `resolve_pending_delivery_is_409_not_resolvable`, `resolve_outcome_unknown_as_published` (mock agent scripted to an ambiguous failure, `reconcile` `Unsupported`).
  - `publish_actions_are_rate_limited`.
  - The isolation matrix `publishing_access`: dry-run at edit, live at publish, edit delegate's live publish 403 `capability_required`, reschedule and cancel need the whole job in limit (limited delegate 403 `account_not_in_scope`), a delivery on an account outside the limit is 404 for `GET` and every delivery action, stranger 404.
  - `delivery_cancel_answers_cancel_report` and `retry_answers_the_delivery_read_back`.
- [ ] **Step 2: Run**: `cargo test -p postit-api --test publishing` → FAIL.
- [ ] **Step 3: Implement** the filter (data test `job_filter_by_campaign_dry_run_and_range`; rerun `cargo sqlx prepare --workspace`), DTOs, and routes.
- [ ] **Step 4: Run** `cargo test -p postit-api --test publishing` and `cargo test -p postit-data` → PASS.
- [ ] **Step 5: Commit**: `feat(api): publish, job, and delivery routes`.

---

### Task 10: Calendar

**Files:**
- Modify: `backend/crates/data/src/publishing/jobs.rs` (`JobRepo::calendar`), `dto_publishing.rs`, `routes/publishing.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/publishing.rs`, data test for the query

**Interfaces:**
- Produces:
  - `postit_data::publishing::CalendarJob { job_id, post_id, scheduled_at, status: JobStatus, campaign_id: Option<CampaignId>, campaign_color: Option<String>, text_head: String, account_ids: Vec<AccountId> }` and `JobRepo::calendar(conn, scope, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<CalendarJob>, DataError>`: live jobs only, `scheduled_at >= from AND scheduled_at < to`, ordered by `scheduled_at`, `account_ids` filtered by the limit, jobs with none in the limit left out. `text_head` is the post text's first line; the route cuts it to 80 graphemes (`unicode-segmentation`, already used by core's measure, or core's grapheme helper).
  - `GET /api/v1/calendar?from=…&to=…` → `{ items: [CalendarItemDto { kind: "job", … }] }`; `to <= from` or a span over 62 days is 422 `validation_failed` with the field named; strong `ETag` = `"<hex sha256 of the serialized body>"`, `304` on match.

- [ ] **Step 1: Write the failing tests**: `calendar_range_is_half_open_and_capped`, `calendar_excludes_dry_runs`, `calendar_item_has_campaign_color_and_80_grapheme_head` (a 100-emoji text gives 80 graphemes), `calendar_304_on_matching_etag_and_changes_after_reschedule`, `calendar_hides_jobs_outside_limit`, and the isolation matrix `calendar_access` (view 200, stranger sees an empty calendar).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** (rerun `cargo sqlx prepare --workspace`).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**: `feat(api): calendar of scheduled jobs`.

---

### Task 11: Admin routes

**Files:**
- Create: `backend/crates/api/src/routes/admin_publishing.rs`, `backend/crates/api/src/dto_admin.rs`
- Modify: `backend/crates/data/src/publishing/deliveries.rs` (`DeliveryRepo::native_scheduled_count(conn, now) -> Result<i64, DataError>`, deployment-wide), `backend/crates/jobs/src/maintenance.rs` (receives `VaultRotate`), `backend/crates/jobs/src/queue.rs` (`enqueue_unless_active`), `backend/crates/server/src/vault_rotate.rs` (imports `VaultRotate` from `postit-jobs`; handler and registration stay), `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/api/tests/admin_publishing.rs` (new), `backend/crates/jobs` queue tests

**Interfaces:**
- Consumes: `PlatformRepo::{list, set_enabled}`, `AppSettingsRepo::{live_publishing, set_live_publishing}`, `UserLimitsRepo::{get, update}` (`UserLimits` fields are `u32`/`u64`, `0` means unlimited; say so in the DTO docs), `AuditLog`, `RequireAdmin`.
- Produces in `postit-jobs`:
  - `postit_jobs::maintenance::VaultRotate { requested_by_user_id: Uuid }` with its `impl Job` (`JOB_TYPE = "vault_rotate"`, `Queue::Maintenance`), moved unchanged from `postit-server`.
  - `JobQueue::enqueue_unless_active<J: Job>(&self, job: &J) -> Result<Option<JobId>, JobsError>`: one transaction; `pg_advisory_xact_lock` keyed on `J::JOB_TYPE` (reuse `data/src/advisory_lock.rs` if it fits); a `jobs` ledger row of that type in state `queued`, `running`, or `failed` means `Ok(None)`; otherwise `enqueue_in` and `Ok(Some(id))`. Test `enqueue_unless_active_enqueues_once` (two concurrent calls: one `Some`, one `None`).
- Produces:
  - `GET /api/v1/admin/platforms` → `[AdminPlatformDto { id, name, compiled_in, configured, enabled, redirect_uri: Option<String> }]` (`redirect_uri` = `{server.public_url}/api/v1/platforms/{id}/oauth/callback` for OAuth platforms only); `PUT /api/v1/admin/platforms/{id}` `{ enabled }` → the DTO, audit `platform_enabled_changed` with `platform_id`, `old`, `new`.
  - `GET /api/v1/admin/settings/publishing` → `{ live, native_scheduled_count }`; `PUT { live }`, audit `live_publishing_changed` with `old`, `new`.
  - `GET /api/v1/admin/users/{id}/limits` → `{ email_daily_cap, contacts_cap, storage_quota_bytes, updated_at }`; `PUT` the three fields, audit `user_limits_changed` with `target_user_id` and, per changed field, `{ field: { old, new } }`.
  - `POST /api/v1/admin/vault/rotate` → `202 { job_id }` through `enqueue_unless_active(&VaultRotate { requested_by_user_id: admin })`; `None` answers `409 rotation_in_progress`.
  - Each audit event is written in the same transaction as its change.

- [ ] **Step 1: Write the failing tests**: `admin_lists_platforms_with_redirect_uri` (the mock OAuth platform has one, `mock` does not), `disabling_platform_writes_audit`, `live_switch_round_trip_writes_audit`, `native_scheduled_count_counts_waiting_uploads`, `user_limits_round_trip_writes_audit`, `negative_limit_is_422` (the `u32` field refuses `-1` at deserialization, which `ApiJson` answers as `422 validation_failed`), `limits_of_unknown_user_is_404`, `vault_rotate_enqueues_once` (second call 409 `rotation_in_progress`), `admin_routes_refuse_members` (403) and `admin_routes_refuse_act_as` (403 `owner_only`).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** (rerun `cargo sqlx prepare --workspace`).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit**: `feat(api): admin platform, publishing, limits, and vault rotation routes`.

---

### Task 12: Contract and client catalogs

**Files:**
- Modify: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/` (all generated), `web/src/i18n/en-US/common.json` (error codes), a new `web/src/i18n/en-US/violations.json` registered in the i18n setup, `mobile/lib/l10n/app_en.arb`
- Create: `web/src/i18n/violations.test.ts`
- Extend (do not duplicate): `web/src/i18n/catalogs.test.ts` and `web/src/api/errors.test.ts` (existing catalog and error-code checks), `mobile/test/ui/l10n_test.dart` (its `every ErrorCode has a translation` test; add `every ViolationCode has a translation` beside it), and `mobile/test/ui/errors/messages_test.dart` for the mobile error map (`mobile/lib/ui/errors/messages.dart`)

**Interfaces:**
- Produces: a `violations` namespace keyed by every `ViolationCode` string, English, with i18next `{{param}}` interpolation of the code's parameters (for example `text_too_long`: `"The text is {{actual}} characters; this platform allows {{max}}."`); ARB entries `violation_<code>` with matching placeholders; error-map entries for `confirmation_required`, `acknowledgement_required`, `not_reschedulable`, `not_retryable`, `not_resolvable`, `post_locked`, `rotation_in_progress`, and any other code in the generated `ErrorCode` enum without one. Plain language per plan 03's "Simple first": say what is wrong and what the platform wants.

- [ ] **Step 1: Regenerate**: `cargo xtask openapi` (from `backend/`; needs `pnpm install` in `web/` and `flutter pub get` in `mobile/`).
- [ ] **Step 2: Write the failing completeness tests**: web `every violation code has an English entry` (reads the `ViolationCode` union from `schema.d.ts` or the enum in `openapi.json` and checks the namespace) and `every error code has an English entry`; mobile `every violation code has an ARB entry` (reads the generated Dart enum's values).
- [ ] **Step 3: Run**: from `web/`, `pnpm test -- violations`; from `mobile/`, `flutter test test/ui/l10n_test.dart test/ui/errors` → FAIL.
- [ ] **Step 4: Write the entries.**
- [ ] **Step 5: Run the client gates**: web per `web/CLAUDE.md` (lint, typecheck, test); mobile `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test`; then `cargo xtask openapi --check` → all PASS.
- [ ] **Step 6: Commit**: `feat(clients): D3 contract, error codes, and the violation catalog`.

---

### Task 13: The exit run

**Files:**
- Create: `backend/crates/server/tests/d3_flow.rs`
- Modify: the shared no-secret-leak harness registration if routes are listed there (`backend/crates/api/tests/*_leak.rs` pattern: add `backend/crates/api/tests/publishing_leak.rs`)

**Interfaces:**
- Consumes: everything above; the server's real composition with `mock-agent`, the email agent with the `testkit` SMTP sink, the mock OAuth provider, and a wiremock OpenAI-compatible AI provider (as `api/tests/ai.rs` does).
- Harness (the server has no router-level test helper; use the seam `server/tests/oauth.rs` and `smoke.rs` already use): `postit_server::start_with_agents(StartOptions { role: Role::All, .. }, Some(registry))` with the mock agents plus the test-only `mock_oauth`, an `postit_identity::testkit::TestIssuer` for the bearer tokens, and requests over HTTP with a `reqwest` client (already a server dependency) against the `RunningServer`'s address. The worker runs in the same process, so a scheduled delivery is made due by the same SQL as `TestApp::make_job_due` (a small helper in the test file) and the test waits for its state with `postit_jobs::testkit::wait_until_async` (10-second timeout) instead of draining.

- [ ] **Step 1: Write the test** `owner_then_publish_delegate_run_the_whole_flow`, all through HTTP against the running server: connect a `mock` account by credentials (check, confirm) and one through the mock OAuth provider (start, follow the callback, confirm); connect the email sending method against the sink; import contacts with `attest_consent` and poll the import to `done`; upload an image (resumable session: start, two chunks, complete); add an AI provider and `POST /ai/generate`; `POST /posts` with the `generation_id`; validate (valid); dry-run publish (simulated); schedule a live publish with `confirm` for two minutes ahead, see it in `GET /calendar`, make it due, wait, see the delivery `published` and a `delivery_published` notification; create an email post with an override holding the email document and `lists`, publish live, wait for the delivery to end, and see each subscribed contact's mail in the sink; read `GET /publish-jobs`, `GET /calendar`, and `GET /notifications`. Then grant a `publish` delegation and repeat the post, validate, schedule, publish, and campaign steps with `X-Postit-Act-As`, asserting `created_by` is the delegate.
- [ ] **Step 2: Write** `publishing_leak.rs`: run publish, retry, and resolve with a mock agent scripted to echo its token and the post text into its errors; assert neither appears in captured logs or in any problem `detail`.
- [ ] **Step 3: Run**: `cargo test -p postit-server --features mock-agent --test d3_flow` and `cargo test -p postit-api --test publishing_leak` → PASS (fix whatever the run exposes in the owning task's code, with a regression test there).
- [ ] **Step 4: Commit**: `test(server): D3 exit run as owner and publish delegate`.

---

### Task 14: Docs, deferred bookkeeping, and the closing gates

**Files:**
- Modify: `CLAUDE.md` (Project state: D3 built, routes, buckets), `docs/contracts/03. publishing roadmap.md` (an "As built (D3)" note under Phase D3 recording the calls marked in the spec: validate answers 200, schedule violations, `generation_id` linking, PATCH stays at `edit`, the empty brand profile, calendar cap and hash ETag, the new `rotation_in_progress` code, the bucket defaults, the limit-answer rule (404 for a hidden single-account resource, 403 for a hidden account named in a body), database field rules as 422 through `DataError::Invalid`, `PLATFORM_ORDER`, and `VaultRotate` moved to `postit-jobs` with `enqueue_unless_active`), `docs/configuration.md` (already done in Task 3; check), `docs/plans/2026-10-30-c1-c2-contacts-email-agent-deferred.md` (mark the violation half of the catalog entry resolved in D3; the DNS finding codes stay open for D4/D6)
- Rename: `git mv docs/plans/2026-10-29-d1-publishing-engine-notifications-deferred.md docs/plans/2026-10-29-d1-publishing-engine-notifications-deferred-done.md`, with a "Resolved in D3 (Tasks 2, 9, 12)" note on its entry
- Create: `docs/plans/2026-11-07-d3-publishing-rest-surface-verification.md` with one entry: **Drive the exit flow by hand** (the owner and a `publish` delegate run connect, compose, validate, schedule, publish, and a campaign against `cargo dev` through `https://postit.local` with an HTTP client such as Bruno or curl; why an agent cannot: agents do not sign in on `postit.local` and the run needs a person's Zitadel session; trigger: before D4 starts). Any further human-only item found during execution goes here too; any agent work that cannot be finished goes to `2026-11-07-d3-publishing-rest-surface-deferred.md`.

- [ ] **Step 1: Write the docs and bookkeeping** above.
- [ ] **Step 2: Run the full gates** from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo xtask openapi --check`; and the web and mobile gates → all PASS.
- [ ] **Step 3: Fix every issue the gates or the final review find**, minor ones included (none may be left only in a chat report).
- [ ] **Step 4: Commit**: `docs: D3 publishing REST surface as built`.
- [ ] **Step 5: Remind the maintainer** of the human checks D3 triggers: C3's real Telegram channel, group, limits, and discovery; C1/C2's real Papercut campaign and the TLS/STARTTLS relay check; and D3's own verification entry.
