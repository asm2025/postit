# D3 Publishing REST surface: design

Plan 03 phase D3. Source of truth: `docs/contracts/03. publishing roadmap.md` (Phase D3, the REST surface rows marked D3, "Publishing", "Delivery states", "Rescheduling, cancelling, and retrying", "Formats and validation", "Posts, variants, and kinds", "Brand profile and writing style", "Audit events added by this plan") and `docs/contracts/01. vision and architecture.md` (delegation levels and the whole-object rule, thin handlers, generated contract). This document records how D3 fits the code as it stands after D1, C1–C3, and D2, the deferred items it picks up, and the calls made where the plans leave room. It does not repeat the plans; read those sections first.

The maintainer skipped the design questions for this phase ("ok"), so every decision below that the plans do not state is marked **(call)** and is open to change in review.

## Intent

Everything below the HTTP layer is built: the publishing engine (`PublishingService`), the repositories for posts, overrides, campaigns, target groups, brand profiles, and writing styles (B2a), the drafter, contacts, media, accounts, and delegation. No client can publish anything yet, because no route creates a post or a publish job. D3 puts thin HTTP handlers in front of those services so that, with the mock agent and the email agent, an HTTP client can do the whole product flow as the owner and as a `publish` delegate. D4 (web) and D6 (mobile) are built on these routes next.

D3 is a backend phase plus the client bookkeeping the contract regeneration needs (generated types, error and violation catalogs). It builds no screens.

## Goals

1. Every REST row marked D3 in plan 03, with the access level in its Access column:
    - `GET /platforms`, `GET /platforms/{id}/icon`
    - `GET /accounts/{id}/publish-options`, `GET/PUT /accounts/{id}/writing-style`
    - `GET/POST /target-groups`, `PUT/DELETE /target-groups/{id}`
    - `GET/PUT /brand-profile`
    - `GET/POST /campaigns`, `PATCH/DELETE /campaigns/{id}`
    - `GET /posts`, `GET /posts/{id}`, `POST /posts`, `POST /posts/validate`, `POST /posts/{id}/validate`, `PATCH /posts/{id}`, `DELETE /posts/{id}`, `PUT/DELETE /posts/{id}/overrides/{account_id}`
    - `POST /posts/{id}/publish`, `GET /publish-jobs`, `GET /publish-jobs/{id}`, `PATCH /publish-jobs/{id}`, `POST /publish-jobs/{id}/cancel`, `GET /deliveries/{id}`, `POST /deliveries/{id}/cancel`, `/retry`, `/resolve`, `GET /calendar`
    - `GET /admin/platforms`, `PUT /admin/platforms/{id}`, `GET/PUT /admin/settings/publishing`, `GET/PUT /admin/users/{id}/limits`, `POST /admin/vault/rotate`
2. Schedule input as RFC 3339 with an offset, or a local date-time plus an IANA zone, defaulting to the acting user's zone; DST gaps and overlaps rejected as violations.
3. ETags on post, job, and calendar reads, with `If-None-Match` answered by `304`.
4. Per-user rate limits on connect, import, upload, generate, publish, and user lookup (reply is E2's route; it reuses the publish bucket).
5. The deployment audit events: `live_publishing_changed`, `platform_enabled_changed`, `user_limits_changed` (the vault rotation events exist).
6. `backend/openapi.json`, the TypeScript types, and the Dart client regenerated; the contract CI job green.
7. The exit run: an HTTP-level test through the server's router with the mock agent and the email agent's SMTP sink, as the owner and as a `publish` delegate.

## Non-goals

- Screens of any kind (D4, D5, D6). The client work in D3 is generated code, error-code catalog entries, and the violation catalog structure.
- The background account CSV import (D4 builds it, per the B3 deferred entry).
- `PUT /posts/{id}/recurrence` and the recurrence occurrences in `GET /calendar` (E1). The calendar's item shape leaves room for them.
- `POST /inbox/{id}/reply` (E2), `POST /media/{id}/fit` (E4), `GET /admin/doctor` (G4).
- The email HTTP services' webhook route (C6).
- The DNS finding catalog in the clients (D4 and D6 show those findings; see "Deferred items picked up").

## Anti-goals

- **Business rules in handlers.** A handler parses, calls one service or repository method, and maps the result. Validation, the target checks, and the whole-object rule live in service or data code that jobs and tests reuse.
- **A second validation path.** `POST /posts/validate`, `POST /posts/{id}/validate`, and `POST /posts/{id}/publish` run the same target check that `PublishingService::create_job` runs today. A draft the validate route calls valid is one the publish route accepts, given the same version and time.
- **Ownership filters outside `postit-data`.** Handlers never filter by owner or account limit; the repositories do.
- **Content in audit, logs, or problem details.** IDs and counts only, as everywhere else.
- **Live publishing in CI.** Every test uses the mock agents or the in-process SMTP sink.

## Constraints

- Thin handlers over service crates (plan 01). Problem+json with stable `code`s; every new code appears in `ErrorCode`, in OpenAPI, and in both clients' error maps.
- Access: the four-caller isolation matrix (owner, a delegate at each level, a delegate limited to other accounts, an unrelated user) for every new route, against the Access column.
- Limit answers, one rule for every route (call, following the existing `ApiError` mapping of `DataError`): a single-account resource outside the caller's limit (an account's writing style, publish options, a delivery) stays hidden and answers `404 not_found`, as `WritingStyleRepo` and the account routes already do. A write that names an account outside the limit in its body (post targets, publish targets, target-group members) answers `403 account_not_in_scope`, checked up front against `OwnerScope::allows_account` the way `create_job` already does, so it reveals nothing about the account. Two repositories answer `NotFound` for that case today: `TargetGroupRepo`'s `reachable` helper (used by `create` and `set_accounts`) and `PostRepo::set_targets`' limit check. D3 changes both to `DataError::AccountNotInScope`; an account inside the limit that is not the owner's or is disconnected stays `NotFound`. A visible multi-account object that reaches hidden accounts answers `403 account_not_in_scope` (the whole-object rule).
- One migration file: schema changes edit `backend/crates/data/migrations/0001_init.sql` in place (the single-migration policy holds until G1); `cargo sqlx prepare` is rerun after query changes.
- Quality gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo xtask openapi --check`, and the web and mobile gates (their own `CLAUDE.md` files).
- Build directories stay outside the project (`CARGO_TARGET_DIR=K:/cargo-target/postit`).

## Decisions

### Where the code goes

- **`PostService` in `postit-publishing` (call).** Post create, update, delete, overrides, and validation need the agent registry, media, and accounts, as publishing already does, and `create_job` already holds the target check. A new `posts` module in `postit-publishing` gets a `PostService` (or extends `PublishingService`; the builder picks, keeping one owner for the target check). The target check in `service.rs` (`check_targets` and `read_post`) becomes the one function both `create_job` and the validate routes call.
- **Brand profile, writing styles, target groups, and campaigns are thin handlers over their `postit-data` repositories (call).** Those repositories already take the scope, call `require`, apply the account limit, and enforce their own field rules (for example `MAX_EXAMPLES`). There is no rule left for a service to own. If the builder finds one (a field rule that is not in the repository), it goes into the repository, not the handler.
- **Admin routes** call `PlatformRepo`, `AppSettingsRepo`, `UserLimitsRepo`, and the jobs queue directly from handlers, writing the audit event through `AuditLog` in the same transaction as the change.
- **`AppState` gains the `PublishingService`**, composed in `postit-server` for the API process the same way the worker gets it. The API process only creates, reschedules, cancels, retries, and resolves; it never claims a delivery.

### Error mapping

`impl From<PublishingError> for ApiError` in `postit-api`, with the statuses from the D1 deferred entry:

| Variant | Status | Code |
|---|---|---|
| `Scope` | 403 | `owner_only` / `capability_required` (existing `ScopeError` mapping) |
| `NotFound` | 404 | `not_found` |
| `AccountNotInScope` | 403 | `account_not_in_scope` |
| `VersionConflict` | 412 | `version_conflict` |
| `ConfirmationRequired` | 400 | `confirmation_required` |
| `AcknowledgementRequired` | 400 | `acknowledgement_required` |
| `ValidationFailed` | 422 | `validation_failed` with `violations` |
| `NotReschedulable`, `NotRetryable`, `NotResolvable`, `JobActive`, `PostLocked` | 409 | `not_reschedulable`, `not_retryable`, `not_resolvable`, `job_active`, `post_locked` |
| `PlatformDisabled` | 409 | `platform_disabled` |
| `PlatformUnavailable` | 503 | `platform_unavailable` |
| `Data(e)` | as `ApiError::from(e)` | the `DataError` mapping; `internal` for the rest |
| `Notify`, `Corrupt`, `Jobs` | 500 | `internal` |

`PublishingError::Data` maps through the existing `From<DataError> for ApiError` rather than straight to 500, so a data error that already has a code keeps it. That `From` gains `DataError::Invalid { field }` (`422 validation_failed` with `field`, see "Brand profile, writing styles, target groups, campaigns") and `DataError::RotationInProgress` (`409 rotation_in_progress`).

`platform_disabled` and `platform_unavailable` use the statuses the accounts routes already use, so one code has one status. API tests reach `not_retryable` and `not_resolvable` (the D1 deferred entry asks for both).

### Schedule input

One `ScheduleInput` type, used by `POST /posts/{id}/publish` (`at`), `PATCH /publish-jobs/{id}` (`at`), and the validate routes:

- a string: RFC 3339 with an offset (`2026-11-08T09:00:00+02:00`); a string without an offset is refused as malformed input (`validation_failed` at `at`, no guessing);
- or an object `{ "local": "2026-11-08T09:00", "zone": "Europe/Berlin" }`. `zone` is optional and defaults to the **acting** user's `user_preferences.timezone` (the delegate's own zone when acting as someone else, because the dialog shows times in the viewer's zone). Seconds are optional in `local`.

Resolved with `chrono-tz` (already a workspace dependency) and stored in UTC. New core `ViolationCode`s, at path `schedule`, returned with every other violation in the 422 (call: violations, not a separate problem code, so the composer shows them inline):

- `schedule_time_nonexistent` (a DST gap; parameters `local`, `zone`),
- `schedule_time_ambiguous` (a DST overlap; parameters `local`, `zone`, and the two candidate offsets, so the client can ask which one),
- `schedule_zone_unknown` (parameter `zone`).

Resolution happens in core next to the other schedule rules (core gains the `chrono-tz` workspace dependency), so recurrences (E1) reuse it with their own gap and overlap policy. The existing "up to a minute in the past is now" rule is unchanged.

### Posts

- **`POST /posts`** (edit): `kind`, `reply_to`, `campaign_id`, `text`, `link`, `tags`, `disclose_ai_generated`, `media` (`[{ media_id, alt_text }]`, order is position), `targets` (account IDs, each in the caller's limit: one outside it is `403 account_not_in_scope` from `PostRepo::set_targets`, checked before any row is written, like `create_job`; an account in the limit that is not the owner's, or is disconnected, is `404 not_found`), and an optional `generation_id`. A media ID that is not the owner's asset is `404 not_found` (`PostRepo::set_media` inserts through an owner-filtered `SELECT` and reports the missing row). Answers `201` with the post and its `ETag`. Overrides are written separately (`PUT …/overrides/{account_id}`), so the create body stays one shape for both modes (call).
- **`generation_id`** links the `ai_generations` row to the new post by setting its `post_id` (call: the row must be the owner's and not yet linked to a post, else `404 not_found`; one generation links to one post). This is how the spend view shows which post a paid draft became.
- **`GET /posts/{id}`** (view) returns the post, its media, its targets and overrides filtered by the caller's limit, and `last_published` per visible target (the newest live `Published` delivery's `post_version` and time), so the publish dialog can warn "this version was already published to X" (plan 03 D4). `ETag: "v{version}"`; `If-None-Match` with the current tag answers `304`.
- **`GET /posts`** (view): paginated, filters `status`, `campaign_id`, `kind` (the existing `PostFilter`). Each item is the post without overrides.
- **`PATCH /posts/{id}`** (edit, whole post in limit, `If-Match`): any of the content fields, `media` (replaces the list), `targets` (replaces the list; targets outside a limited delegate's limit are kept, as target groups do). Answers the post with its new `ETag`.
- **`DELETE /posts/{id}`** (`If-Match`): `edit` and whole post in limit; `publish` when the post has live history. Reading of plan 01 (call): plan 03's table lists "`publish` when the post has live history" against PATCH and DELETE together, but plan 01 says only deleting needs `publish`, because deleting deletes the history. An edit does not touch history, so PATCH stays at `edit`. `PostRepo::delete` already makes this decision (any non-dry-run job of the post means `publish`); `PostService` does not repeat it. `204`.
- **`PUT /posts/{id}/overrides/{account_id}`** (edit, account in limit, `If-Match` on the post): `{ patch, options }`, where `patch` holds the fields that differ (text, link, tags, a media subset, the email document for an email target, the `lists` for a direct target). The repository bumps the post version, so the answer carries the post's new `ETag`. `DELETE` removes it, same rules.
- **Whole post in limit** is the repositories' `ensure_whole_post` (`backend/crates/data/src/posts.rs`), which every post write already calls: a limited delegate gets `403 account_not_in_scope` on PATCH, DELETE, and override writes when an **override** on the post, or a **delivery that has not ended**, belongs to an account outside the limit. A hidden **target** alone does not block an edit: the post is visible whole to the delegate, and `set_targets` replaces only the targets inside the limit and keeps the hidden ones. `PostRepo::outside_limit` (which also counts targets) is not the edit check, and D3 does not call it.
- **`post_locked`** comes from `PostRepo::lock` when a scheduled or running job holds the post; the handler only maps it.

### Validation routes

Both answer `200` with the result, never `422`, because the request itself succeeded (call):

```json
{ "valid": false,
  "targets": [ { "account_id": "…", "format": "text", "violations": [ … ] } ],
  "violations": [ … ] }
```

The top-level `violations` holds the ones not bound to a target (the schedule ones).

- **`POST /posts/validate`** (edit, targets in limit): an unsaved post (the `POST /posts` body), optional per-target `overrides`, `at`, and `native_schedule`. Media IDs must be the owner's assets; a missing one is a `media_missing` violation, not a `404`.
- **`POST /posts/{id}/validate`** (edit, targets in limit): validates the stored post and overrides for `{ targets?, at?, native_schedule? }`; `targets` defaults to the post's targets.

Both call the shared target check, which reads each target's options schema through the agent, so validating can reach a platform. They are charged to the publish bucket (below).

### Publishing, jobs, and deliveries

- **`POST /posts/{id}/publish`** (`If-Match`; edit for dry-run, publish for live; targets in limit): `{ targets[], at?, native_schedule?, dry_run?, confirm }`, calls `create_job`, answers `201` with the job (deliveries included). Targets are account IDs; a client expands target groups itself (call: the contract says `targets[]`, and expanding on the server would hide from the dialog which accounts a confirmation covers).
- **`GET /publish-jobs`** (view): paginated, filters `status`, `post_id`, `campaign_id`, `dry_run`, `from`/`to` on `scheduled_at`. A limited delegate sees a job when at least one of its deliveries is in their limit, and only those deliveries ("deliveries in limit").
- **`GET /publish-jobs/{id}`** (view): the job, its deliveries in the caller's limit, and direct-send progress counts for direct deliveries. `ETag: "v{version}"` (`publish_jobs.version`), `304` on `If-None-Match`.
- **`PATCH /publish-jobs/{id}`** (publish, whole job in limit, `If-Match` on the job): `{ at }` as `ScheduleInput`; answers `RescheduleOutcome` (`moved`, `failed`).
- **`POST /publish-jobs/{id}/cancel`** (publish, whole job in limit): answers `CancelReport`.
- **`GET /deliveries/{id}`** (view, delivery in limit): state, parking reason, remote ID and URL, the last error's code and class (never a platform message), attempt count, `created_by`, the job and post IDs, `remote_publish_at`, and direct-send counts.
- **`POST /deliveries/{id}/cancel`**, **`/retry`** (`{ confirm, acknowledge_outcome_unknown? }`), **`/resolve`** (`{ outcome: published | failed }`) (publish, account in limit): call the matching service methods. Answers (call): cancel answers the `CancelReport` that `cancel_delivery` returns (the same shape as cancelling a job); retry and resolve answer the delivery as `GET /deliveries/{id}` shows it, read back with `DeliveryRepo::get` after the service call (`retry` returns a `Dispatch` and `resolve` a `DeliveryState`, neither of which is the client's view).
- Every response field that names a user is the user ID; clients resolve names through the existing routes.

### Calendar

- **`GET /calendar?from=…&to=…`** (view): RFC 3339 bounds, `to` after `from`, at most 62 days apart (call: a month view plus its spill-over weeks), else `validation_failed`. Returns live jobs (dry runs excluded: they run at once and are not plans) with `scheduled_at` in the range: job ID, post ID, `scheduled_at`, status, the campaign's ID and color, the post's first line of text cut to 80 graphemes (call: enough for a calendar cell), and the target account IDs in the caller's limit. Jobs with no delivery in the caller's limit are left out.
- Each item has `"kind": "job"`, so E1 adds `"kind": "occurrence"` without breaking clients.
- **ETag:** a strong tag over a SHA-256 (through `emixcrypto`) of the serialized body (call: the calendar has no single version column). `If-None-Match` answers `304`. Job and post reads use their version columns instead.

### Platforms and publish options

- **`GET /platforms`** (any signed-in user): the `AgentDescriptor`s, serialized as today (the icon is left out), for platforms that are compiled in, configured, and enabled. Sorted by catalog rank (call: the order the connect screen shows them). No rank exists in code yet: `postit-oauth`'s catalog gains `pub const PLATFORM_ORDER: &[&str]`, the platform IDs in plan 03's catalog order (`email`, `telegram`, `slack`, `discord`, `bluesky`, `mastodon`, `x`, `youtube`, `linkedin`, `facebook`, `instagram`, `threads`, `tiktok`; an agent crate added later uses its own platform ID here), and a platform not in the list (`mock`, `mock_direct`) sorts after it by ID. The agents' descriptors do not change.
- **`GET /platforms/{id}/icon`** (any): the descriptor's PNG, `Content-Type: image/png`, `Cache-Control: public, max-age=86400`, a strong ETag over its bytes, `304` on `If-None-Match`. A platform that is not listed by `GET /platforms` answers `404`.
- **`GET /accounts/{id}/publish-options`** (edit, account in limit; an account outside the limit is `404`): the account's `OptionsSchema` from the agent's `publish_options`, including any `consent_text`. It may call the platform, so it is charged to the connect bucket. Agent errors map as the account check route maps them today.
- These routes are signed-in routes and live in a new `routes/platform_catalog.rs`; `routes/platforms.rs` stays the bearer-free routes a platform calls (the OAuth callback and events).

### Brand profile, writing styles, target groups, campaigns

- **`GET/PUT /brand-profile`**: `GET` answers `200` with the profile or, when the owner wrote none, an empty profile with `id: null` (call: clients render one form either way, rather than handling a 404). `PUT` upserts.
- **`GET/PUT /accounts/{id}/writing-style`**: view; edit with the account in limit (an account outside the limit is `404`, as `WritingStyleRepo` answers). Same empty-on-missing rule for an account in the limit that has no style yet.
- **Target groups**: `GET` (paginated), `POST { name, account_ids }`, `PUT /{id} { name, account_ids }` (replaces; accounts outside a limited delegate's limit are kept and hidden, which the repository does), `DELETE`.
- **Campaigns**: `GET` (paginated; `include_archived` query flag, default `false`, matching `CampaignRepo::list`'s `include_archived`), `POST { name, starts_on?, ends_on?, color? }`, `PATCH /{id}` (any field, including `archived`), `DELETE` (unlinks posts, never deletes them; the repository does). `CampaignRepo::update` replaces every field, so D3 adds `CampaignRepo::patch(conn, scope, id, CampaignPatch)`, one `UPDATE … SET name = COALESCE($3, name), …` (a nullable date or color is cleared with an explicit `null`, so `CampaignPatch` uses `Option<Option<_>>` for those), and `PATCH` calls it; `archived: true` still goes through `archive`.
- **Field rules that the database enforces** come back as 422, not 500. Two rules are unnamed `CHECK`s in `0001_init.sql` today, so they surface as `DataError::Sql`: the brand profile's five-example limit and a campaign's `ends_on >= starts_on`. D3 names them (`brand_profiles_examples_max`, `campaigns_dates_ordered`), and `constraint::translate` turns them into a new `DataError::Invalid { field: &'static str }` (`examples`, `ends_on`), which `ApiError` maps to `422 validation_failed` with `field`. The repositories that write those columns call `translate`. The rule stays in the data layer; no handler checks it.

### Admin routes

All `admin`, reject act-as with `owner_only`, and write their audit event in the same transaction as the change:

- **`GET /admin/platforms`**: every platform row: ID, name, compiled in, configured, enabled, and for OAuth platforms the redirect URI to copy into the developer portal (`{public_url}/api/v1/platforms/{id}/oauth/callback`). **`PUT /admin/platforms/{id}`** `{ enabled }` writes `platform_enabled_changed` (platform ID, old and new value).
- **`GET /admin/settings/publishing`**: `{ live, native_scheduled_count }` (deliveries waiting on a platform's own schedule, which the kill switch cannot stop). **`PUT`** `{ live }` writes `live_publishing_changed`.
- **`GET /admin/users/{id}/limits`**, **`PUT`** with the three `user_limits` fields; `PUT` writes `user_limits_changed` (user ID and the changed field names with old and new numbers, which are counts, not content).
- **`POST /admin/vault/rotate`**: enqueues the existing `vault_rotate` job with the admin's ID and answers `202 { job_id }`. A run already queued or running answers `409 rotation_in_progress` (new code; the job's advisory lock remains the real guard). The payload struct `VaultRotate` and its `impl Job` (type name and queue only) move from `postit-server`'s `vault_rotate.rs` to `postit-jobs::maintenance`, so the API can name it; the handler and its registration stay in the server. The "already queued or running" check is one new `postit-jobs` method, `JobQueue::enqueue_unless_active<J: Job>(&self, job: &J) -> Result<Option<JobId>, JobsError>`: in one transaction it takes a transaction advisory lock keyed on `J::JOB_TYPE`, looks for a `jobs` ledger row of that type in state `queued`, `running`, or `failed` (a `failed` row waits for its retry), and enqueues only when there is none (`None` means one is active). `DataError::RotationInProgress`, which the re-seal lock already returns, maps to the same `409 rotation_in_progress` wherever it reaches the API. The job already writes `vault_rotation_started` and `vault_rotation_completed` (`backend/crates/server/src/vault_rotate.rs`); the handler writes no audit event of its own.

### Rate limits

Per user (the actor), charged before the work but after the route's capability check, as `routes/ai.rs` does, so a refused caller is not charged; `429 rate_limited` with `Retry-After` (the existing `KeyedLimiter::charge`):

| Bucket | Config key | Routes |
|---|---|---|
| connect (existing `credential_checks`) | `rate_limit.credential_checks` | credentials check and import (already), `POST /accounts/connect/{platform}` (new), `GET /accounts/{id}/publish-options` (new) |
| upload (new) | `rate_limit.uploads` | `POST /media`, `POST /media/uploads` (the session start; chunk `PUT`s are not charged, so a resumable upload is one charge) |
| publish (new) | `rate_limit.publish_actions` | publish, both validate routes, reschedule, both cancels, retry, resolve; E2's reply later |
| generate (existing `ai_requests`) | `rate_limit.ai_requests` | unchanged |
| lookup (existing) | `rate_limit.lookup` | unchanged |

Defaults (call): `uploads` 30 per minute, burst 10; `publish_actions` 60 per minute, burst 20. The buckets are fields of `RateLimitSettings` in `backend/crates/config/src/settings.rs`, checked by its `check_bucket`, with defaults in the loader's TOML; the API testkit's `default_settings` sets them generous like the others. Documented in `docs/configuration.md` and in the development and example config.

### Audit events

`AuditEventKind` gains `LivePublishingChanged`, `PlatformEnabledChanged`, and `UserLimitsChanged`, and the API's `AuditEventKindDto` mirrors them so the audit screen's filter lists them. The publishing events are written by the service already.

### Clients

- `cargo xtask openapi` regenerates `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`.
- Both error maps (`web/src/i18n/en-US/*.json`, `mobile/lib/l10n/app_en.arb`) gain English entries for every new problem code: `not_retryable`, `not_resolvable`, `rotation_in_progress`, and any publishing code not mapped yet.
- **Violation catalog structure (call):** a `violations` namespace in the web catalog keyed by `ViolationCode` with i18next interpolation of the parameters, and `violation_<code>` ARB entries with placeholders in the app. Every `ViolationCode` in `openapi.json` gets an English entry, the three C2 email codes and the new schedule codes included. A completeness test in each client fails when a code in the generated enum has no entry, so a new code fails CI. D3 builds no screen that shows them; the catalogs exist so D4 and D6 only render.

### Testing

- **HTTP tests per route** in `postit-api` (or the server's test crate where the full composition is needed) with the mock agent: success, each error code the route documents, `ETag`/`If-Match`/`If-None-Match`, and the four-caller isolation matrix against the Access column.
- **Confirmation rules:** a live publish without `confirm`, a retry without `confirm`, a retry of an `OutcomeUnknown` delivery without the acknowledgement.
- **Schedule input:** offset string, local plus zone, zone defaulting to the actor's preference (and the delegate's own zone through act-as), a DST gap, an overlap, an unknown zone, a string without an offset.
- **Validation parity:** for a set of posts, the validate route's verdict equals what publish accepts or refuses.
- **Rate limits:** each new bucket refuses past its burst, and a chunk `PUT` is not charged.
- **Admin:** each change writes its audit event; act-as refused; a member refused.
- **Running publishing jobs in tests.** The API testkit runs on the system clock, so tests never "advance the clock". As D1's tests do, a test makes a scheduled delivery due by moving its time in SQL, then runs the queued jobs by hand. `TestApp` gains `make_job_due(job: PublishJobId)` (moves the job's pending deliveries' `next_attempt_at` and their `publishing` ledger rows' `run_at` to one second ago) and `drain_publishing() -> usize`: it repeatedly reads the `jobs` ledger rows of the `publishing` queue in state `queued` whose `run_at` has passed, runs each through its runner by `job_type` (`publish_delivery` with `run_publish`, `direct_send_batch` with `direct::run_batch`, `poll_delivery_status` with `poll::run_poll`, `reconcile_delivery` with `reconcile::run_reconcile`), remembers the ledger IDs it ran so it never runs one twice (no worker moves the ledger rows in the API testkit, and the runners enqueue follow-up jobs under new IDs), and stops when a pass finds no new row (panicking after 50 passes). It returns how many jobs it ran.
- **Exit run** (server test crate, through the real server: `postit_server::start_with_agents` with `Role::All`, the mock agents plus `mock_oauth`, and an `identity` `TestIssuer` for tokens, as `server/tests/smoke.rs` and `server/tests/oauth.rs` start it; requests go over HTTP with a `reqwest` client (already a server dependency); the worker runs in the same process, so after `make_job_due`-style SQL the test waits with `postit_jobs::testkit::wait_until_async` for the delivery state; mock agent plus the email agent against the in-process SMTP sink): connect a mock account by credentials and one through the mock OAuth provider, import contacts, upload media, generate a post with a wiremock AI provider, create the post with the `generation_id`, validate, dry-run, schedule, wait for the worker, see the delivery published and the notification; send an email campaign to a list and see it in the sink; then the same flow as a `publish` delegate through `X-Postit-Act-As`.
- The shared no-secret-leak harness runs over the new routes' logs (no token, signed URL, or address).

## Deferred items picked up

- **D1, "API and client mapping of the publishing service's errors"** (`2026-10-29-d1-publishing-engine-notifications-deferred.md`): resolved by the error mapping, its tests, and the client entries above. It is the file's only entry, so the file is renamed to `-deferred-done.md` with a note.
- **C1/C2, "Client catalog entries for the new violation codes and the DNS finding codes"** (`2026-10-30-c1-c2-contacts-email-agent-deferred.md`): D3 does the violation half (catalog structure, every code, completeness tests). The DNS finding codes stay open for D4 and D6, the screens that show them; the entry is marked partly resolved and the file keeps its name.

Not picked up, triggers elsewhere: the B3 background import (D4), the Dart CSV and file upload clients (D6), the Dart delegation `expires_at` PATCH (D6), the C2 rate-limit and NUL entries (G1), click link index (E3), and the C3 Telegram entries (D4, E2, E3).

## Human verification waiting on the maintainer

D3 is the trigger for these existing entries; the plan reminds the maintainer and does not do them:

- C3: a real Telegram channel and group, limits in practice, discovery on a real bot (`2026-11-01-c3-telegram-agent-verification.md`, "now, or phase D3 at the latest").
- C1/C2: a campaign through a real Papercut viewed in its UI, now possible through `POST /posts/{id}/publish`; real TLS and STARTTLS against a relay (`2026-10-30-c1-c2-contacts-email-agent-verification.md`).

D3 adds one entry to its own `-verification.md`: drive the exit flow by hand against `cargo dev` with an HTTP client (for example Bruno or curl) through `https://postit.local`, owner and delegate, because agents do not sign in on `postit.local`.

## Left to the builder

- Whether `PostService` is a new type or methods on `PublishingService`, as long as the target check has one owner.
- DTO names and field order, following the existing `dto_*.rs` files (a new `dto_posts.rs` and `dto_publishing.rs` are expected).
- Whether a route's tests live in `postit-api` with fakes or in the server test crate with the real composition; the exit run must be the real composition.
- Using the existing `Idempotency-Key` support on `POST /posts` and `POST /posts/{id}/publish` if it fits the routes' shape; a live publish is already protected by the one-active-job rule.
- The exact OpenAPI response lists, following the existing handlers' style.
- Updating `CLAUDE.md` "Project state", the roadmap's D3 section with an "As built (D3)" note, and `docs/configuration.md`.
