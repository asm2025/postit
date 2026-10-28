# D1 Publishing engine, notifications, and push — design

Plan 03 phase D1. Source of truth: `!ref/plans/03. publishing roadmap.md` (Phase D1,
"Publishing", "Delivery states", "Rescheduling, cancelling, and retrying", "Quotas",
"Disconnecting an account", "Sending an email campaign" steps 3–7, "Credential refresh",
"Notifications and push", "Jobs added by this plan", "Audit events added by this plan", the REST
rows marked D1) and `!ref/plans/01. vision and architecture.md` (crate levels 6–10, delegation
notification rules, `notifications`, data retention). This document records how D1 fits the
code as it stands after B2–B7 and D0c, the deferred items it picks up, the one scope addition
the maintainer asked for (browser push), and the calls made where the plans leave room. It does
not repeat the plans; read those sections first.

## Intent

Everything before D1 stores posts, accounts, and media, and B2b built the delivery data layer,
but nothing publishes yet. D1 makes queued work real: durable, safely claimed, observable,
retryable, idempotent, and impossible to duplicate after a remote success. It also makes postit
able to tell people things: in-app notifications, critical emails, and push to Android
(UnifiedPush) and, added by the maintainer, to browsers.

D1 is proven entirely with the `mock` and `mock_direct` agents. It adds service APIs, not the
publishing routes; D3 puts HTTP in front of them. The only HTTP D1 adds is the notification,
push, and preference routes.

**One plan.** The maintainer chose a single D1 plan (engine and notifications together) over a
D1a/D1b split. The plan should still order its tasks so the engine lands first and
`postit-notify` second, with the notification seam defined before either.

**The notification seam.** The `NotificationKind` enum and its payload types in `postit-core`,
plus `postit-notify`'s one entry point, `record(tx, event)`, which resolves recipients and
writes rows and jobs inside the caller's transaction. The plan creates the crate with this
function first (rows only), so the engine, `oauth`, and `identity` call it from the start; the
email, push, and digest fan-out are added behind it later without touching the callers.

## Goals

### Engine (`postit-publishing`, new)

- **Jobs** on a new `publishing` queue, ID-only `JobSummary`s, no apalis retry, console retry and
  cancel disabled: `publish_delivery`, `poll_delivery_status`, `reconcile_delivery`,
  `direct_send_batch` (enqueued), `delivery_due_sweep` (every minute), `account_preflight`
  (every 10 minutes).
- **Worker flow** exactly as plan 03 "Publishing" steps 3–7: claim with `dispatch_gen` and
  lease, lease renewal every third of `publishing.lease`, fenced writes, the step-4 checks in
  order with parking, quota reservation and release, revalidation, the agent call (`publish`,
  `Commenter::comment`, or the direct flow), retry with full-jitter backoff and the
  `publishing.max_attempts` budget, `DeadLetter`, `Failed`, `OutcomeUnknown` plus reconcile,
  `outcome_unresolvable` parking, `container_ref` persistence, processing polls with backoff
  15 s → 15 min, the processing timeout (from `remote_publish_at` for native-scheduled uploads),
  late receipts through the reconcile path, and job status refresh after every write.
- **Due sweep** with all five duties from plan 03, and the parking rule with
  `publishing.park_grace`. It is the only place that unparks.
- **Service API** for D3: create a job from a post (snapshot, deliveries, enqueue, in one
  transaction, `publish_confirmed` audit for live), reschedule, cancel (job and delivery),
  retry (confirm, acknowledgement, revalidation, `job_active`), resolve. Native-scheduled
  reschedule and delete go through `NativeScheduler`.
- **Direct deliveries:** audience materialization, `direct_send_batch` with batch claim, lease
  renewal, the per-user daily cap with continuation at the next day, per-recipient results,
  5xx → `bounced` plus suppression, 4xx retry within the batch budget, terminal counts,
  `Processing` → `Published`, crash mid-batch resuming without re-sending terminal rows.
- **Dry-run** recording in `delivery_attempts`, `Simulated` outcomes, no change to
  `posts.status`; a direct dry-run reports the audience count and materializes nothing.
- **Graceful shutdown:** an `InFlight` delivery stops at its next safe checkpoint and returns to
  `Pending` with `dispatch_gen` + 1; one past its final create or publish call finishes within
  `server.shutdown_timeout`.
- **Account preflight** using `connected_accounts.last_checked_at`.
- **Audit events:** `publish_confirmed`, `publish_job_rescheduled`, `publish_job_cancelled`,
  `delivery_cancelled`, `delivery_retried`, `delivery_resolved`, `delivery_published`,
  `delivery_dead_lettered`, `delivery_outcome_unknown` (IDs and counts only), with their web
  audit-kind labels in English and Arabic.

### Cross-crate hooks

- **Deletion steps** registered in `postit-server` in this order: publishing (cancel what can be
  cancelled, delete native-scheduled uploads through the agent), oauth (exists), media (remove
  the user's stored files, picking up the B4 deferred entry).
- **Disconnect** (`postit-oauth::AccountService::disconnect`): native-scheduled uploads are
  deleted on the platform through the agent where `NativeScheduler` allows; the rest stay listed
  in `native_scheduled_remaining`. A direct delivery in `Processing` for the disconnected email
  account has its queued rows cancelled (this settles the open question in the B2b follow-up
  plan, "Notes for D1"). `oauth` sits below `publishing`, so the hook is a trait `oauth` defines
  and `publishing` implements, injected by `postit-server`. *(My call, same pattern as
  `UserDeletionStep`.)*
- **Media in requests (B5 deferred):** request assembly resolves the snapshot's media IDs through
  `MediaService::assets`, so agents get real `MediaAsset`s; the conformance fixture gains real
  assets so media-rule validation is exercised. A snapshot asset deleted since the job was
  created fails revalidation with a violation (retry path) or `Failed` (worker path).

### Housekeeping items assigned to D1

- **Unknown job types** (plan 02 P5 open item 2): dispatch re-queues a job of an unregistered
  type as pending with `run_at` + 60 s and no attempt consumed. The maintenance sweep's
  `kill_unregistered` stays only as a backstop for types unknown far longer than any rolling
  deploy: a new setting `jobs.unknown_type_max_age` (default 7 days). *(My call: without a
  backstop a permanently removed type re-queues every minute forever.)*
- **Console reason codes** (D0 open item): `actions_disabled_reason` becomes a stable code
  (`already_retried`, `type_not_registered`, `must_finish`, `publishing_managed`, …) in the
  OpenAPI enum; the web maps codes, not sentences (`web/src/features/jobs/JobActions.tsx`
  `REASON_KEYS`); mobile models regenerate.
- **Preferences** (D0 open items): `PUT /me/preferences` creates a missing row instead of 404;
  `UpdatePreferences.locale` is the `en-US`/`ar` enum in OpenAPI.

### Notifications (`postit-notify`, new)

- **Crate level** per plan 01: `notify` (config, core, http, data, jobs, mail, i18n, vault),
  below `identity`, `oauth`, and `publishing`, which all record through it. `identity` gains its
  dependency on `notify` here, as plan 01 says.
- **Recording** inside the caller's transaction: one `notifications` row per recipient, plus the
  email and push jobs it implies, enqueued in the same transaction.
- **Recipients** (plan 01): the owner; plus the delegate who triggered the event while their
  grant is live and, for account-bound kinds, the account is within their limit. *(The limit
  check is my call: a delegate who can no longer see the account should not be told about it.)*
  Deployment notifications (`owner_id` null) go to every active admin.
- **Kinds** (closed enum in core, each with a severity and a payload of IDs and counts):
  - Critical (in-app + email + push by default): `delivery_dead_lettered`, `delivery_failed`,
    `delivery_outcome_unknown`, `delivery_parked` (live deliveries only), `delivery_stale`
    (the sweep found the cause cleared but left the delivery parked past `park_grace`),
    `job_partial`, `account_needs_reconnect`, `account_revoked_by_platform`,
    `reconnect_by_passed`, `recurrence_skipped`.
  - Non-critical (in-app; push only where the user enables it): `delivery_published`,
    `reconnect_reminder` (7 days and 1 day), `delegation_changed` (granted, changed, revoked,
    left, expired, limit emptied; the event in the payload), `new_comments` (coalesced count per
    account per poll), `user_pending` (admins).
  - `recurrence_skipped` (E1) and `new_comments` (E2) have no producer yet; D1 defines them,
    their switches, their texts, and the digest so E1 and E2 only call `notify`. Tests drive
    them directly.
- **Producers wired in D1:** the engine (delivery and job kinds); `postit-oauth`
  (`needs_reconnect` from refresh, preflight, and pasted-credential auth failures;
  `revoked_by_platform` from platform events); `refresh_credentials` gains the 7-day and 1-day
  `reconnect_by` stages (`reconnect_warned` `7d`, `1d`) and notifies on `expired` (B7 deferred);
  the delegation service and `delegation_expiry` (B6 deferred: the other party on every grant,
  change, revocation, leave, and expiry, and both parties when a limited grant loses its last
  account); the sign-in path for `user_pending` (plan 01: in-app for admins from D1, alongside
  the existing approval emails).
- **Payload: IDs, counts, and display labels** (decided by the maintainer). Besides IDs and
  counts, a payload carries labels copied in when the notification is recorded: the account's
  display name and platform ID, and for delegation kinds the other party's display name. No
  post text, comment text, addresses, or secrets. Clients build the sentence from the kind code
  and the labels in the user's language, so "codes, not sentences" holds, and an old
  notification still reads correctly after the account is renamed or a delegate's grant ends.
  This extends plan 01's "IDs and counts" wording; the recipient rule already keeps a label from
  reaching anyone who could not see the account. Plan 01's `notifications` description is
  updated to match. `GET /notifications` returns `{ id, kind, severity, owner_id, payload,
  read_at, created_at }` with the payload typed per kind in OpenAPI.
- **Two parking notifications, two markers.** `delivery_parked` is recorded in the same
  transaction as the transition into `Parked`, so it fires once per parking and needs no marker.
  `delivery_stale` keeps B2b's existing use of `deliveries.parked_notified_at` in
  `Sweep::unpark` (set only in the "cause cleared but too old" case). The worker never sets
  `parked_notified_at` at parking time, because that would suppress `delivery_stale`.
- **Preferences:** per-kind `push` and `email` switches (email only meaningful for critical
  kinds), stored on `user_preferences`, returned by `GET` and accepted by `PUT /me/preferences`.
  The existing `email_notifications` stays as the master email switch. Defaults per plan 03:
  push on for new comments, failed or parked deliveries, a skipped recurrence, and
  `needs_reconnect`; email on for every critical kind; digest off.
- **`PUT /me/preferences` becomes a partial update:** every field optional, an absent field
  unchanged (and `null` treated as absent, because the generated Dart client sends absent
  optional fields as `null`). It accepts `locale`, `timezone` (IANA name, validated with
  `chrono-tz`; invalid → 422), `email_notifications`, the per-kind switches, and the digest
  switch. Today nothing can set `timezone` (rows are created with `UTC`), so the profile page
  gains a time-zone field prefilled from the browser's zone when the stored value is still the
  default, and the digest and D3's schedule defaults read it. *(My call: settling the zone in
  D1 rather than shipping a digest that runs at 08:00 UTC for everyone.)*
- **Critical email:** a `MailKind::Notification` with a `MailContextLoader` in `postit-notify`;
  email jobs carry only notification IDs; `emailed_at` is the sent marker. Templates use askama
  on the shared `postit-mail` layout, strings from `postit-i18n`, recipient's locale.
  **Coalescing:** the send runs 2 minutes after the first unemailed critical notification, and
  the loader gathers every unemailed critical notification of that recipient into one email
  ("3 things need your attention"), marking them all; later jobs find nothing and skip.
  *(My call: a ten-target job that fails would otherwise send ten emails.)*
- **Daily comment digest:** an hourly `notification_digest` cron emails, at 08:00 in the
  recipient's `user_preferences.timezone`, the `new_comments` notifications since the last
  digest, for users who enabled it. *(My call: the hour and the cron shape; the plan only says
  "daily".)*
- **Routes:** `GET /notifications` (paginated, newest first), `POST /notifications/read`
  (`{ ids }` or `{ all: true }`), `GET /notifications/unread-count` (cheap, ETag,
  `If-None-Match` → 304). Self only; act-as answers 403 `owner_only` (plan 01: notifications are
  never delegable). A delegate's own list includes notifications from the workspaces they act
  in; `owner_id` tells the client which.

### Push (Android UnifiedPush and browsers)

- **Server:** `push_endpoints` (sealed endpoint, `p256dh`, `auth`), `platform` gains `web`.
  `GET /push/vapid-public-key`, `POST /me/push-endpoints`, `DELETE /me/push-endpoints/{id}`
  (self, `owner_only` under act-as). Re-registering the same endpoint replaces the row
  (matched by an endpoint hash column, since the endpoint itself is sealed). *(My call.)*
- **Sending:** `push_send { notification, endpoint }` encrypts per RFC 8291 (aes128gcm), signs
  VAPID per RFC 8292 with `push.vapid_private_key` and `push.vapid_subject`, and sends through
  the `postit-http` public client (address policy; `push.allow_private_endpoints` builds a
  permissive one for LAN distributors). `404`/`410` deletes the endpoint; `last_success_at` is
  set on success; `pushed_at` on the notification after the first success. Endpoint URLs are
  logged only as `Sensitivity::Opaque`.
- **Payload:** notification ID, kind, and a short title and body rendered server-side in the
  recipient's locale. Never post text, comment text, addresses, or tokens.
- **Crate:** a maintained Web Push crate for the encryption and VAPID JWT, chosen at plan start
  against the dependency rules (license, maintenance, no Arabic-derived name); it must let
  postit send the request through `postit-http` rather than its own client (for example, one that
  builds an `http::Request`, or `ece` for encryption plus the existing JWT dependency for
  VAPID). Hand-rolling RFC 8291 is the fallback only if no crate fits, and needs the RFC test
  vectors either way.
- **Config:** `push.enabled` (exists, default false) gates registration and sending; with it off,
  `GET /push/vapid-public-key` answers 404 `push_disabled` (new code) and nothing is enqueued.
  The development vault gets a generated VAPID key pair in `postit.env` and `push.enabled =
  true` in `development.toml`; Claude edits the extracted vault file only, the maintainer
  re-packs `vault.7z`. `stack doctor` checks the key is present when push is enabled.

### Browser push (web client, added at the maintainer's request)

- **Service worker** at the site root (`/sw.js`, built by Vite, served by the server's web
  handler with `Service-Worker-Allowed` not needed at root scope). It handles `push` (shows the
  title and body from the payload, tag = notification ID) and `notificationclick` (focuses an
  open postit tab or opens one, then navigates to `/` with the notification ID; D4 maps kinds
  to screens once those screens exist). *(My call: no screen in D1 shows a single notification.)*
- **CSP:** the server's web CSP gains `worker-src 'self'`; the existing CSP test is extended.
- **Profile page** (`web/src/features/me`): a "Browser notifications" switch (asks permission,
  subscribes with the VAPID key, posts the endpoint as `platform: web` with a device label from
  the user agent, unsubscribes and deletes on off; shows a blocked or unsupported state
  plainly), the master email switch, the per-kind push and email switches, the digest switch,
  and the time-zone field. Built from the D0b
  kit, in English and Arabic, right-to-left checked.
- No bell, list, or unread badge on the web in D1; that is D4.

## Non-goals

- Publishing, post, job, delivery, and calendar **routes**: D3.
- **Recurrences** and `recurrence_tick`: E1. **Inbox** polling: E2. **Metrics** jobs: E3.
- **Contacts** service and the public unsubscribe, click, and open routes: C1 and C2. D1's direct
  flow reads the existing contacts tables; tests insert rows directly. The `LinkSigner` stays
  the existing placeholder (`server/src/unavailable.rs`, "C2 (link signing) replaces them"):
  the unsubscribe token needs the owner, which the current `LinkSigner` trait does not receive,
  so the real signer and its trait change belong with the routes that verify it, in C2.
  *(Revised while writing the plan; the design first had D1 build the signer.)*
- **Mobile:** UnifiedPush registration, the WorkManager polling fallback, and notification
  screens are D6. D1 only regenerates the Dart client.
- **Real agents:** C3–C5 and F.
- **App-bucket admin visibility** (`GET /admin/doctor` usage): G4.

## Anti-goals

- A duplicate post on any path: stale or duplicate apalis jobs, console retry, a stalled worker's
  late success, a crash after a remote success, retry after an edit, shutdown, concurrent
  workers.
- A parked live delivery silently degrading to dry-run, or a dry-run changing `posts.status`.
- A retry or reschedule publishing content other than the job's `content_snapshot`.
- An unparked delivery that is stale beyond `park_grace` going out without the owner acting.
- Notifications, emails, push payloads, audit events, or logs carrying post or comment text,
  contact addresses, tokens, endpoint URLs, or keys.
- A delegate learning about accounts outside their limit, or anything after their grant ended.
- A notification email storm: one email per failed delivery.
- A push request to a private address without `push.allow_private_endpoints`.

## Constraints

- One migration file: schema changes edit `backend/crates/data/migrations/0001_init.sql` in
  place.
- `postit-http` is the only place that builds `reqwest` clients.
- Crate levels from plan 01 (`notify` level 6; `publishing` level 10 may depend on `oauth`,
  `media`, `agent-kit`, `identity`, `notify`; `publishing` does not need `contacts` or `drafter`
  in D1). Lower crates reach higher ones only through traits injected by `postit-server`.
- `postit-jobs` is the only crate that depends on apalis.
- Every sealed column is BYTEA with `COMMENT … 'postit:sealed'` and listed in `SEALED_COLUMNS`.
- Reuse the B2b data layer (`data/src/publishing/`): claim, fence, apply, sweep, direct,
  quota, jobs repositories already exist and are tested. Add queries there; do not duplicate.
- Four-caller isolation tests for every new owned-data route.
- The quality gates in `CLAUDE.md`, the web gates in `web/CLAUDE.md`,
  `cargo xtask openapi --check`, and the mobile gates (the Dart client regenerates).
- The shared no-secret-leak harness covers push sending, notification email, and the engine's
  logs.

## Tests

Everything in the plan 03 D1 test list, plus:

- Mock: a scripted reconcile `Unsupported` (the mock's reconcile returns only Found or Absent
  today), so `outcome_unresolvable` is exercised.
- Disconnect hook: native-scheduled delete through the agent, and a running direct delivery's
  queued rows cancelled.
- Deletion steps: publishing step with pending, in-flight, and native-scheduled deliveries
  (idempotent across retries); media step removing files; order publishing → oauth → media.
- Notifications: fan-out to owner and live delegate; none to a revoked or expired delegate or
  one whose limit excludes the account; admin `user_pending`; per-kind switches respected;
  critical email coalescing (several notifications, one email, all marked); digest at the local
  hour, once per day, only for enabled users; B6 and B7 producers each create the right kind.
- Push: RFC 8291 test vectors, VAPID header shape, `404`/`410` cleanup, address policy, push
  disabled, endpoint re-registration replaces.
- Routes: notifications list, read, unread-count with ETag and 304; push endpoint routes;
  preferences PUT on a missing row; locale enum; `owner_only` under act-as.
- Web: the profile switches (component tests with a mocked `PushManager`), the service worker's
  push and click handlers, the CSP containing `worker-src 'self'`, Arabic and right-to-left.
- Unknown job type re-queued at dispatch with no attempt consumed; killed only after
  `jobs.unknown_type_max_age`.

## Left to the builder

- Module, type, and job-struct names inside `postit-publishing` and `postit-notify`.
- The shutdown signalling mechanism between the worker runtime and in-flight deliveries.
- How the per-kind switches are stored (columns or a JSONB map), within "defaults per kind".
- The exact console reason-code names.
- The Web Push crate, within the rules above.
- Whether the service worker is a separate Vite entry or a static file.
- The precise backoff constants inside the plan's ranges.

## Deferred items picked up

- `2026-10-24-b5-agent-kit-mock-deferred.md`: request assembly with real assets and the
  conformance fixture. Its only entry, so the file is renamed `-deferred-done.md` at the end of
  D1.
- `2026-10-26-b4-media-deferred.md`, second entry: media files removed in the deletion step
  (the first entry stays for D6).
- `2026-10-26-b6-delegation-deferred.md`, first entry: delegation notifications (the second
  stays for D6).
- `2026-10-28-b7-oauth-connection-deferred.md`: reconnect notifications and warning stages. Its
  only entry, so the file is renamed `-deferred-done.md` at the end of D1.

Files with entries left open keep their names, with the resolved entries marked.
