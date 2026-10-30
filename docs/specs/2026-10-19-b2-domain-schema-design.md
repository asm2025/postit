# B2: Domain schema and scoped repositories (design)

Date: 2026-10-19. Source: plan 03 (`docs/plans/03. publishing roadmap.md`), phase B2, plus the sections it links to: Publishing, Delivery states, Rescheduling, cancelling, and retrying, Quotas, Disconnecting an account, Acting through a delegation, Sending an email campaign, Recurring posts, Jobs added by this plan, and Media. Plan 01 covers the data model (Owned content, Deployment, Data retention, User deletion), delegation and the capability table, and the engineering standards ("Access checks live in one place", "Enqueue with the change"). This document does not repeat the plan. It records intent, the decisions the plan left open, and what the builder decides.

The user skipped the question round. Every decision below is the designer's call unless the plan states it, and each call is marked.

## Intent

Every later phase reads and writes owned content through `postit-data`, and every concurrency guarantee the publishing engine makes (no double publish, no lost post, no quota overshoot, no edit racing a publish) is a SQL statement written here. B2 builds the whole schema and those statements before any service exists, and proves them under concurrency with tests. An isolation mistake here shows one user's posts or contacts to another. A claim or fence mistake here posts twice on a real account, which plan 03 treats as the worst failure the product can have.

## Goals

- Everything under plan 03 phase B2, meeting its exit criteria: repositories can create posts, comment posts, campaigns, recurrences, and contacts; create publish jobs; claim deliveries and send batches safely under concurrency; and record every outcome, with isolation and account limits proven by tests.
- Close P4 deferred ruling 6: `IdempotencyRepo` takes `&OwnerScope`.
- Fill the A2 sealed-column catalog and keep it honest by test.
- Give D1 (publishing engine), D3 (REST), B3/B7 (connections), B4 (media), B6 (delegation), and C1 (contacts) a repository surface they call instead of writing SQL.

## Non-goals

- No `delegations` or `delegation_accounts` tables (B6). B2 adds `Access::Delegated` and tests it with constructed scopes, as the plan says.
- No services, routes, jobs, or job handlers. B2 provides queries that D1's handlers and D3's services call. It enqueues nothing itself (it sits below `postit-jobs`); functions that create work return the IDs and run times the caller passes to `JobQueue::enqueue_in` in the same transaction.
- No time-zone arithmetic. Quota `period_start` and recurrence occurrences are computed by the caller (D1, E1) and passed in as values. B2 stores and compares them.
- No `ops_alerts` (G4).
- Repositories only for the tables the exit criteria and the next phases need now (listed below). Tables owned by later phases get their schema now and their repository in their phase (designer's call, listed under "Schema only").

## Anti-goals

- A repository method on owned rows that does not take `&OwnerScope`, or that filters by owner in some methods and not others.
- A delegate limited to some accounts seeing, through any repository method, a delivery, override, target, writing style, quota row, or account outside the limit. Rows outside the limit behave as not found.
- A worker write that lands after the delivery was re-dispatched, cancelled, or swept.
- Two transactions that together reserve more quota units, or more email-cap units, than the limit.
- A post edit that commits while a publish job for that post is being created, or a job whose snapshot changes after creation.
- Any plaintext secret in any column.
- A cross-owner reference that the database accepts because the repository forgot to check.

## Constraints

- Single migration policy: everything goes into `backend/crates/data/migrations/0001_init.sql`; the dev database is reset.
- Compile-checked `sqlx::query!` macros and the committed offline cache, as in the existing repositories. Tests use `#[sqlx::test]`.
- `postit-data` depends on `config`, `core`, and `vault` only.
- Lists return `emixdb::ResultSet<T>` from `emixdb::Pagination`.
- Hashes use `sha2`, which `postit-data` already uses (designer's call: follow the nearest code rather than adding `emixcrypto` to this crate).
- PostgreSQL 18: `ON DELETE SET NULL (column)` on composite foreign keys is available and used.

## Decisions

### Core additions

- New ID newtypes in `postit-core::ids` (same macro): `GrantId`, `TargetGroupId`, `RecurrenceId`, `ContactId`, `ContactListId`, `ContactImportId`, `DeliveryAttemptId`, `InboxItemId`, `AiProviderId`, `AiGenerationId`, `NotificationId`, `PushEndpointId`, `DelegationId`, `WritingStyleId`, `OAuthStateId`, `PendingConnectionId`. (`DelegationId` now so `Access::Delegated` can carry it for audit.)
- Text forms for the enums already in core that become columns (`DeliveryState`, `ParkReason`, `JobStatus`, `PostKind`): `as_str` and `FromStr`, owned by core so the strings live once. The `CHECK` constraints in SQL list the same strings, and a data test asserts every core variant round-trips through its column.

### `OwnerScope` and access

- `Access` becomes `Owner | Delegated { delegation: DelegationId, level: Level, accounts: AccountLimit }`, with `AccountLimit::All | Only(Arc<[AccountId]>)` (sorted, deduplicated). `OwnerScope` stops being `Copy` and stays `Clone` (cheap: the account list is behind an `Arc`). Callers that copied it now borrow or clone.
- `Capability` (designer's call on the split): `View`, `Edit`, `Publish`, and `OwnerOnly`. `Level` is the delegation level (`View`, `Edit`, `Publish`). `require(OwnerOnly)` fails for every delegated scope with `ScopeError::OwnerOnly`; the others compare the level. The capability table in plan 01 maps each action to one of these four; services choose which to require.
- **Account filter, one implementation.** `OwnerScope::account_filter() -> Option<&[Uuid]>` returns `None` for the owner and for `AccountLimit::All`, and the allowed IDs otherwise. Every account-bound query binds it as `$n::uuid[]` and adds `AND ($n::uuid[] IS NULL OR <account column> = ANY($n))`. A unit test asserts the empty limit (`Only([])`) yields `Some(&[])`, which matches nothing, so an empty limit sees no accounts (plan 01 "Empty account limit"). A test helper in `testkit` runs a supplied query closure under the four callers (owner, delegate at each level, delegate limited to other accounts, unrelated user), so each repository's isolation matrix is a few lines.
- **Whole-object check.** `PostRepo::outside_limit(conn, scope, post_id) -> bool` answers whether any override, draft target, or non-terminal delivery of the post belongs to an account outside the scope's limit. Services use it to return 403 `account_not_in_scope` (plan 01, "Whole-object rule"). B2 owns the query; B6 tests the rule at the service and HTTP layers.
- **Admin repositories** (`UserLimitsRepo`, `AppSettingsRepo`, `PlatformAppQuotaRepo`, and the existing `PlatformRepo`) take no scope, are separate types, and have no method that joins owned tables (designer's call: a test greps nothing; the separation is by type and review).

### Schema conventions

- Every owned table: `id UUID PRIMARY KEY`, `owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE`, `created_at`, `updated_at`, `created_by UUID REFERENCES users(id) ON DELETE SET NULL`, `updated_by` likewise, and `UNIQUE (owner_id, id)`.
- Every reference between owned rows is a composite FK `(owner_id, x_id) REFERENCES t(owner_id, id)`. Nullable references use `ON DELETE SET NULL (x_id)` so the owner column is never nulled (for example `connected_accounts.grant_id`, `posts.campaign_id`, `direct_sends.contact_id`, `inbox_items.delivery_id`). Child rows that live and die with their parent use `ON DELETE CASCADE` (`post_media`, `post_targets`, `post_overrides`, `deliveries` from `publish_jobs`, `delivery_attempts`). References to `connected_accounts` from history (`deliveries.account_id`) are `RESTRICT`; accounts are never deleted except with their owner, and then the cascade from `users` removes the history first.
- Enumerations are `TEXT` with `CHECK (x IN (...))`, like the existing tables.
- **Sealed columns** are `BYTEA` with `COMMENT ON COLUMN … IS 'postit:sealed'` (designer's call). A test reads `pg_description` and asserts the set of commented columns equals `SEALED_COLUMNS`, so a sealed column added without a catalog entry (or the reverse) fails CI. Catalog entries in B2: `account_grants.credential`, `account_tokens.access_token`, `oauth_states.pkce_verifier`, `oauth_states.grant_tokens`, `pending_connections.credential`, `ai_providers.api_key`, `push_endpoints.endpoint`, `push_endpoints.p256dh`, `push_endpoints.auth`.
- Every table the plan lists under Owned content gets created now, except `delegations` and `delegation_accounts`. Plus the deployment tables `app_settings`, `user_limits`, `platform_app_quota_usage`, and the connection tables `oauth_states` and `pending_connections` (owned by `user_id`/`owner_id`; needed for the purge queries).
- **Designer's additions to plan 01's columns**, each needed by a query the plan asks for:
  - `direct_sends.lease_expires_at`: a batch claim leases its rows, so a crashed batch's rows return to the queue when the lease expires, and the sweep's "missing `direct_send_batch`" check is a plain query.
  - `deliveries.post_id` (denormalized, composite FK): lets the one-live-job and `post_locked` checks and the whole-object query avoid a join through `publish_jobs`. The builder may drop it if the queries stay simple without it.
  - `publish_jobs.status` is stored, and recomputed from deliveries by one function (below).
  - `contacts.email_hash`: the same HMAC-SHA256 of the normalized address (keyed by `email.signing_key`) that `suppressions` stores, written by the contacts repository on insert and on email change. One Rust function owns the normalization and the HMAC; the repository takes the key as an argument.
  - `deliveries.parked_notified_at`: the sweep tells the owner once about a parked delivery that stays parked past `park_grace`.
  - `recurrences.paused_reason` gains `ended` (a rule that ran out of occurrences).
  - `[retention] contact_imports` (default 30 days) in `postit-config`, which plan 01's retention table lists but A2 did not add.

### Indexes and constraints the plan names

- `deliveries.idempotency_key BYTEA UNIQUE`, computed in Rust as `sha256(job_id bytes || account_id bytes)` by one function in `postit-data`.
- One non-terminal live job per post: `CREATE UNIQUE INDEX … ON publish_jobs (post_id) WHERE NOT dry_run AND status IN ('scheduled', 'publishing')`.
- `connected_accounts (platform_id, remote_account_id) WHERE status <> 'disconnected'` unique (deployment-wide).
- `connected_accounts (owner_id) WHERE platform_id = 'email' AND status <> 'disconnected'` unique (one sending method).
- Unique `(owner_id, email)` on contacts, `(owner_id, email_hash)` on suppressions, `(delivery_id, contact_id)` on direct sends, `(account_id, remote_comment_id)` on inbox items, `(account_id, day)` on account metrics, the quota-usage uniques from plan 01.
- Indexes for each hot query: due pending deliveries (`state, next_attempt_at`), expired leases (`state, lease_expires_at`), parked (`state, parked_reason`), queued direct sends per delivery, due recurrences (`next_occurrence_at WHERE NOT paused`). The builder adds others the tests show are needed.

### Repositories built in B2

Module per area in `postit-data`, each method taking `&mut PgConnection` (a transaction where the caller needs one) and `&OwnerScope`:

- `accounts`: grants (`insert`, `get`, `set_status`, `list_accounts`), connected accounts (`insert_or_reactivate`, `get`, `list` with the account filter, `set_status`, `disconnect` (sets `disconnected`, clears `grant_id`, deletes its `account_tokens`, its overrides, its inbox items, and removes it from target groups and recurrence targets, pausing recurrences left with none), `grant_account_count`), `account_tokens` (`upsert`, `get`). Reconnect by a different owner of a remote account that is not disconnected returns `DataError::AccountConnectedElsewhere` without naming the owner.
- `posts`: posts, `post_media`, `post_targets`, `post_overrides`, the post lock, version, and comment posts (`kind = 'comment'`, `reply_to`).
- `campaigns`, `target_groups` (with `target_group_accounts`), `writing_styles`, `brand_profiles`.
- `media`: `media_assets` insert, get, list, delete, and the owner's stored-bytes sum (B4 builds intake on top and may extend it).
- `recurrences` (with `recurrence_targets`).
- `publishing`: publish jobs, deliveries, delivery attempts, quota usage, direct sends.
- `contacts`: contacts, lists, list members, suppressions, contact imports (create, get, list, upsert by email, status change; C1 builds import and export on top).
- Admin: `user_limits`, `app_settings`, `platform_app_quota_usage`.
- `idempotency`: changed to take `&OwnerScope` (actor and owner from the scope); `postit-api`'s middleware passes the request scope.

**Schema only** (repository in the owning phase, which must use the same scope and account-filter helpers): `inbox_items` (E2), `post_metric_snapshots` and `account_metrics_daily` (E3), `ai_providers` and `ai_generations` (D2), `notifications` and `push_endpoints` (D1), `email_events` (C2), `oauth_states` and `pending_connections` beyond their purge query (B3, B7).

### The post lock and versions

- `PostRepo::lock(conn, scope, post_id, expected_version: Option<i64>) -> Result<LockedPost, DataError>` runs `SELECT … FOR UPDATE` on the post (owner-filtered), then checks:
  - `expected_version` against `posts.version`: mismatch is `DataError::VersionConflict` (412); `None` is allowed only for internal callers, and the API layer's `If-Match` rule (428 when missing) stays in `postit-api`.
  - `post_locked`: any non-terminal publish job for the post (dry-run included, designer's call: a dry run runs immediately and ends in seconds, and locking it keeps "the snapshot is what you previewed" simple) returns `DataError::PostLocked` (409).
- `LockedPost` is a token that only `lock` creates. Every write that changes what the post would publish takes `&LockedPost`: text and fields, media and alt text, draft targets, override upsert and delete, recurrence set and delete, and job creation. Each such write calls one private `bump_version(conn, &LockedPost)` that increments `posts.version`, sets `updated_by`, and pauses the post's recurrence with `paused_reason = 'post_edited'` if it is not already paused (plan 03, "Editing pauses"). Job creation does not bump the version.
- The type makes "write without the lock" a compile error rather than a review item (designer's call).

### Publish jobs and deliveries

- `create_job(conn, scope, &LockedPost, NewJob { snapshot: PublishSnapshot, scheduled_at, native_schedule, dry_run, recurrence_id }) -> Result<CreatedJob, DataError>` inserts the job with `post_version` and `content_snapshot` (the core `PublishSnapshot` serialized as JSON), and one delivery per variant's account (`state = 'pending'`, `dispatch_gen = 0`, `next_attempt_at = scheduled_at`, `operation` from the post kind and agent kind, which the caller passes per target), all with `created_by = actor`. It returns each delivery's `(id, dispatch_gen, next_attempt_at)` for the caller's `enqueue_in`. The one-live-job index turns a race into `DataError::JobActive` (409 `job_active`).
- The snapshot column is never updated after insert (no repository method writes it; a test edits the post after job creation and asserts the snapshot is byte-identical).
- **Claim.** `claim(conn, delivery_id, dispatch_gen, now, lease) -> Option<ClaimedDelivery>` is the plan's single `UPDATE … WHERE id AND dispatch_gen AND state = 'pending' AND next_attempt_at <= now AND the job is not cancelled RETURNING …`, setting `state = 'claimed'` and `lease_expires_at`. Claims are system writes: they take no scope (the worker is not a user) and the returned row carries `owner_id` for the checks that follow.
- **Lease renewal.** `renew_lease(conn, id, gen, now, lease) -> bool`, conditioned on `dispatch_gen` and a state in (`claimed`, `in_flight`).
- **Fenced transitions.** One function, `apply(conn, Fence { id, dispatch_gen, expected: DeliveryState }, event: DeliveryEvent, now) -> Result<Applied, DataError>`, computes the new state with core's `transition()` (so the transition table lives once, in core), writes it with `WHERE id = $1 AND dispatch_gen = $2 AND state = $3`, bumps `publish_jobs.version`, and returns `Applied::Stale` when no row matched. Events that re-enqueue (retry backoff, reschedule, unpark, user retry, sweep, graceful shutdown) increment `dispatch_gen` in the same statement and return the new generation and run time for the caller to enqueue. Payload columns (`remote_id`, `remote_url`, `container_ref`, `remote_publish_at`, `last_error`, `parked_reason`, `attempt_count`, `cancel_requested_at`) are written by the same statement from the event's data. `container_ref` has its own fenced write so multi-step agents persist it before each next step.
- **Late success.** `record_late_receipt(conn, id, receipt) -> bool` moves an `outcome_unknown` delivery with no `remote_id` to `published` with the receipt, regardless of `dispatch_gen` (plan: "a late success is never lost and never duplicated"). It is the only unfenced write besides user actions, and it is conditioned on the state.
- **User actions** (retry, resolve, cancel, reschedule) are scoped methods: they take `&OwnerScope` and the delivery's account must pass the account filter. Cancel of an `in_flight` delivery sets `cancel_requested_at`; cancel of `pending` and `parked` sets `cancelled` and bumps `dispatch_gen`.
- **Sweep queries** (system, return rows for the caller to enqueue or reconcile): overdue pending (`next_attempt_at < now - sweep_grace`, bump `dispatch_gen`), expired `claimed` back to `pending` (bump), expired `in_flight` to `outcome_unknown`, direct deliveries with queued rows and no live batch lease, and **unpark**: a parked delivery whose cause has cleared, decided in one query per reason against current state (owner `active`; account `active`; platform `enabled` and in a caller-supplied list of compiled-in platform IDs; `app_settings` `publishing.live` true; `quota_exceeded` once `next_attempt_at` has passed), returning to `pending` only when its due time is within `park_grace`, else left parked and returned in a "needs owner" list so D1 notifies once (designer's call: a `parked_notified_at` column on deliveries records the notification so it is sent once). `outcome_unresolvable` never unparks.
- All sweep updates use `FOR UPDATE SKIP LOCKED` sub-selects with a batch limit, so two sweeps in two workers never fight over rows.
- **Job status.** `refresh_job_status(conn, job_id, now)` loads the job's deliveries, runs core's `derive_job_status`, writes `publish_jobs.status`, and mirrors `posts.status` when the job is live and its `post_version` equals `posts.version` (else the post shows `draft`). Every fenced transition that can change status calls it in the same transaction (designer's call: correctness over one extra query).
- **Attempts.** `delivery_attempts` insert and finish, carrying the classified error and a redacted request summary the caller supplies.

### Quotas

- Account buckets: `reserve(conn, QuotaKey { owner, account, platform, bucket, period_start }, cost, limit) -> bool` as one `INSERT … ON CONFLICT DO UPDATE SET units_used = units_used + $cost WHERE units_used + $cost <= $limit RETURNING`, so the check and the increment are one statement and concurrent reservations cannot overshoot. `release(…, cost)` decrements with `GREATEST(units_used - $cost, 0)`.
- App buckets: the same two functions on `platform_app_quota_usage`, in the admin repository (no owner).
- **Email daily cap.** `reserve_up_to(conn, key, want, limit) -> u32` grants as many units as fit (`LEAST($want, $limit - units_used)`) in one statement; `limit = 0` means unlimited and skips the row update. Keyed on the owner's sending account with `platform_id = 'email'` and bucket `daily_cap`, as plan 01 says. A batch whose worker crashed keeps the units it reserved and its rows reserve again when reclaimed, so after a crash the day's cap can look used up with recipients left for tomorrow; this over-count is deliberate, because some of that mail may have been sent but not recorded.

### Direct sends

- **Materialize.** `materialize_audience(conn, delivery, owner, list_ids, tag_filter) -> u64`: one `INSERT INTO direct_sends … SELECT DISTINCT ON (contact) … FROM contact_list_members JOIN contacts … WHERE status = 'subscribed' AND NOT EXISTS (suppression for email_hash) AND tag filter ON CONFLICT DO NOTHING`, storing the count in `deliveries.direct_counts`. Postgres has no HMAC without `pgcrypto`, so the suppression check cannot hash addresses in SQL; `contacts.email_hash` is stored instead (see the additions above).
- **Batch claim.** `claim_batch(conn, fence, size, now, lease, cap: Option<CapKey>) -> BatchClaim` in one transaction: first checks the fence (the delivery is still `processing` under the caller's dispatch generation) with a share lock, returning `Stale` without reserving anything when it is not; reserves up to `size` email-cap units (`reserve_up_to`), then leases that many queued rows with `FOR UPDATE SKIP LOCKED` ordered by `id`, sets `batch` (a UUID the data layer allocates once per claim and returns with the rows) and `lease_expires_at`, and releases any reserved units it could not use. Zero granted units means "cap reached, continue tomorrow" and is returned as such for D1 to reschedule.
- **Record results.** Per-row terminal writes (`sent`, `failed`, `suppressed`, `bounced`) conditioned on the row being leased to that batch; a bounce adds the suppression in the same transaction. A late result wins over a cancel: a row the cancel marked `failed`/`cancelled` while its batch was sending keeps its batch and lease, so that batch's real result replaces the cancel and the delivery's stored counts are refreshed under a lock on the delivery. A row never leased stays cancelled, and a recorded result is final.
- **Terminal check.** `direct_progress(conn, delivery_id) -> DirectCounts` and `finish_if_done` move the delivery to `published` through the fenced transition when no row is `queued`, writing the counts.

### Recurrences

- `due(conn, until, limit)`: unpaused recurrences with `next_occurrence_at <= until`, `FOR UPDATE SKIP LOCKED`.
- `advance(conn, id, expected_next, new_next: Option<DateTime>, version) -> bool`: one `UPDATE … SET next_occurrence_at = $new, occurrences_created = occurrences_created + 1 WHERE id AND next_occurrence_at = $expected AND NOT paused`. The tick runs it in the same transaction as `create_job`; the loser of two concurrent ticks matches nothing and rolls back (plan's "recurrence advance racing two tick runs (one wins)"). `None` ends the recurrence (sets it paused with reason `ended`, designer's call: add `ended` to `paused_reason`).
- `record_skip(conn, id, at, reason)` appends to `skipped` (keeping the most recent 20, designer's call).
- Set, confirm, pause, resume, delete are scoped writes through `LockedPost`.

### Users, limits, and settings

- `UsersRepo::provision` and the bootstrap path insert the `user_limits` row in the same transaction, from a `UserLimitDefaults` value built from `[limits]` (and `media.user_quota_bytes` when `storage_quota_bytes` is unset). `postit-identity` passes it; the signature change ripples to its callers.
- `app_settings (key TEXT PRIMARY KEY, value JSONB, updated_at, updated_by)`. `AppSettingsRepo::seed_defaults(&Settings-derived values)` inserts missing keys at startup (`publishing.live` from config) without overwriting; `postit-server` calls it next to the platform upsert, API process only, like A2. Typed getters (`live_publishing()`), no generic string API (designer's call).

### Deletion, retention, and purges

- `delete_user`'s owned-row step: `delete_owned_batch(conn, owner, table, limit) -> u64` over an ordered list of the largest tables (`direct_sends`, `email_events`, `delivery_attempts`, `inbox_items`, `post_metric_snapshots`, `contacts`), called until each returns zero, so no single transaction holds millions of rows; the final `DELETE FROM users` cascades the rest. D1 wires it into the deletion steps; B2 provides and tests the query and the cascade (a seeded owner with rows in every table is deleted, and another owner's rows and the delegate-authored `created_by` references survive with `NULL`).
- `data_retention` purges for: read notifications, `delivery_attempts`, `ai_generations` prompt clearing and row deletion, `inbox_items`, `direct_sends`, `email_events`, `contact_imports`. Batched, cutoffs computed in Rust (the existing `retention.rs` style). `postit-server`'s existing `data_retention` job calls them; B2 adds the calls and the config key.
- `oauth_state_purge` query: delete expired `oauth_states` and `pending_connections`. The job registration is B3/B7's; the query and its test are here.

### Errors

`DataError` gains: `VersionConflict`, `PostLocked`, `JobActive`, `AccountConnectedElsewhere`, `SendingMethodExists`, `AccountNotInScope`, and `Scope(ScopeError)`. Each maps one-to-one to the API codes in plan 03 (D3 does the mapping). Unique-violation errors are translated by constraint name in one helper, so a constraint rename breaks a test instead of silently returning a generic conflict.

## Testing

All `#[sqlx::test]` against the real schema.

- Schema from empty; every core enum round-trips its column; the sealed-column comment set equals the catalog; the plaintext check (seed every sealed column through the vault with known fixture secrets, then scan every `text`, `jsonb`, and `bytea` column of every table for each fixture string, which must not appear).
- Composite FKs reject cross-owner references for each owned reference (table-driven).
- The isolation matrix (four callers) for every repository built in B2, using the testkit helper.
- Concurrency (two connections, explicit interleaving with transactions held open, as `reseal` tests already do): two claims of one delivery (one wins); a stale `dispatch_gen` claim finds nothing; lease expiry and the sweep taking over; a stalled worker's fenced write rejected after the sweep; an edit and a job creation racing on one post (one waits on the row lock and then sees `post_locked` or the new version); concurrent quota reservations never exceeding the limit (N tasks, limit < N); two workers claiming disjoint direct-send batches; a cap reached mid-campaign leaving rows queued; two recurrence ticks advancing one recurrence (one wins).
- Constraints: the idempotency key rejecting a duplicate delivery; a second job republishing the same post version after the first ended; the one-live-job index ignoring dry runs; a disconnected account freeing its remote key for another owner; the one-email-account constraint; `post_locked`; the snapshot unchanged by a later edit.
- Audience materialization skips suppressed, unsubscribed, bounced, and complained contacts and duplicates across lists.
- Retention purges and the oauth purge delete exactly the expired rows.
- User deletion cascade as described above.

## Delivery as two plans (designer's call)

B2 is too large for one plan of the A2 size. It ships as two plans on one branch, in order:

- **B2a — schema, scope, and owned repositories:** core IDs and enum strings, `Access::Delegated` and the account filter, the full schema, the sealed catalog test and plaintext check, user limits and app settings, the idempotency change, and the repositories for accounts, posts (with the lock and versions), campaigns, target groups, writing styles, brand profile, media, recurrences (CRUD), and contacts, each with its isolation matrix.
- **B2b — publishing concurrency:** job creation, claim, lease, fenced transitions, late receipt, user actions, sweep and unpark, job status, attempts, quotas, direct sends, recurrence tick queries, deletion batches, retention and oauth purges, and the concurrency tests.

## Left to the builder

- Module and type names, as long as they read like the existing repositories.
- Exact column order and index names.
- Whether `deliveries.post_id` is needed.
- Batch sizes for sweeps and purges (around 500).
- How the testkit isolation helper is shaped, as long as each repository's matrix is short and covers the four callers.
