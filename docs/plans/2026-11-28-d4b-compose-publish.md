# D4b Compose and publish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The second D4 slice: a web user uploads media, writes a brand profile, composes a post in controlled mode with per-target overrides and publish options, validates it, dry-runs it, schedules or publishes it with confirmation, follows it in the queue and history, acts on its deliveries, and groups posts in campaigns, as owner and as a `publish` delegate.

**Architecture:** One small backend change (the publish-job list carries what a queue row shows and filters open versus finished jobs), then web screens over the D3 REST surface. One composer module owns the draft state (a reducer, the post's `ETag`, dirty tracking) so the D4c AI panel and the D4d email editor plug into it without reaching into its internals. One upload module owns resumable uploads. New kit components (`ViolationList`, `ScheduleInput`, `UploadProgress`, `MediaThumb`, `TextCounter`, `ColorSwatchPicker`) carry everything a page would otherwise draw itself. No platform-specific code in the web app.

**Tech Stack:** Rust (axum, sqlx), React 19, React Router 8, TanStack Query 5, `openapi-fetch`, i18next, Radix/shadcn, Tailwind 4, Vitest + MSW, Playwright.

**Spec:** `docs/specs/2026-11-27-d4-web-publishing-ui-design.md` (sections "Shared decisions" and "Slice details: D4b Compose and publish"). Also read plan 03 Phase D4, "Publishing", "Delivery states", "Rescheduling, canceling, and retrying", "Media", and the REST surface table in `docs/contracts/03. publishing roadmap.md`, and the as-built D4a plan `docs/plans/2026-11-27-d4a-workspace-accounts.md` (workspace, `useAccess`, `wsKey`, kit components this plan reuses).

## Global Constraints

- US English everywhere (code, catalogs, docs, commits).
- Web: pnpm only, run from PowerShell (the Git Bash pnpm shim is broken). Gate: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build` in `web/`.
- Pages use `web/src/components/kit/` and `web/src/components/ui/`; `kitGuard.test.ts` must stay green. A missing widget becomes a kit component, tested in `dir="ltr"` and `dir="rtl"`.
- Every user-visible string is in an `en-US` catalog under `web/src/i18n/en-US/`; layouts use logical properties. A task that adds a namespace registers it in both `web/src/i18n/index.ts` and `web/src/i18n/i18next.d.ts`.
- No platform ID literal in `web/src/features/**` or `web/src/components/**` outside tests (`noPlatformIds.test.ts`); formats, limits, options, native scheduling, and names come from `GET /platforms`.
- Every workspace-scoped query key goes through `wsKey` / `useWsKey()` (`web/src/workspace/keys.ts`); `queryKeyGuard.test.ts` stays green. Self keys (`SELF_KEYS`) are only for data that is the same in every workspace.
- Actions are gated with `useAccess()` only (`atLeast('edit')`, `atLeast('publish')`, `inLimit(id)`); pages never re-derive levels. A refusal the server still sends is shown through `messageFor`, never swallowed.
- **ETags are opaque.** A post's `ETag` is `"v{version}-{hash}"` and a job's is `"v{version}"`; the client stores the `ETag` response header it received and sends it back as `If-Match`, never one it built from `version`.
- No secret, signed media URL, or upload session ID in `localStorage`, logs, or error reports. Upload session IDs live in `sessionStorage` only (spec).
- Polling only while visible (`refetchIntervalInBackground: false`): a job with unfinished deliveries every 5 s backing off to 30 s.
- Backend gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, targeted `cargo test -p <crate>`, `cargo xtask openapi --check` (PowerShell; regenerates `web/src/api/schema.d.ts` and `mobile/lib/api/`).
- `CARGO_TARGET_DIR=K:/cargo-target/postit`; DB tests use `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, quotes stripped; `-- --test-threads=4` for `postit-data`. New `query!` macros need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data`.
- Workspace lints deny `unwrap_used` and `expect_used` outside tests.
- Commit trailer: `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`.

## Review Focus

1. **Editing in two places:** the post changed in another tab or by a delegate between opening the composer (or the publish dialog) and saving or publishing. The server answers `412 version_conflict`; the client reloads the post, keeps the user's unsaved text in a visible "Your unsaved changes" panel they can copy from, and never resends blindly. Tests in Task 7 (`save conflict reloads and keeps the unsaved text`) and Task 9 (`publish conflict reloads the dialog`).
2. **An upload that loses the network or the page:** a chunk `PUT` fails mid-file, the tab reloads, or the user picks a different file with the same name. The upload resumes from `HEAD`'s `Upload-Offset` only when name, size, and last-modified all match; a `409 upload_offset_mismatch` re-reads the offset once and continues; an expired session (`404`) starts over; a different file starts over. Tests in Task 3.
3. **A live publish the user did not mean:** a `publish` delegate or owner must tick the confirmation before the button enables; an `edit` delegate can only dry-run; the same post version already published to a target shows a warning; a post that changed after the dialog opened is refused by `If-Match`. Tests in Task 9.
4. **A schedule time across a daylight-saving change:** a time in a gap comes back as `schedule_time_nonexistent` and is shown; a time in an overlap (`schedule_time_ambiguous`) asks which offset and resends as RFC 3339 with that offset; a past time is a violation, not a silent "now". Tests in Task 5 (`ScheduleInput`) and Task 9.
5. **Acting on a delivery in the wrong state:** retry on an `outcome_unknown` delivery needs the acknowledgment checkbox; `409 not_retryable`, `not_resolvable`, `not_cancelable`, `not_reschedulable`, `job_active`, and `post_locked` each show their translated message and refresh the job, never loop. Tests in Task 11.
6. **Switching workspace mid-work:** `switchTo` (`web/src/workspace/WorkspaceProvider.tsx`) sets the new act-as owner before it navigates, `authFetch` reads the owner on every request, and the shell remounts its outlet on the owner (`Shell.tsx`, `<Outlet key={workspaceKey}>`). So an upload still running would send workspace A's chunks as workspace B, and an unsaved draft would vanish without `useBlocker` firing. The upload queue aborts on a switch and keeps its sessions for A; the switcher asks first while the composer is dirty. Tests in Task 3 (`a workspace switch stops the upload and never starts one in the other workspace`) and Task 7 (`switching workspace with unsaved changes asks`).
7. **The live counter disagrees with the server:** the counter ports core's measures and `post_text` exactly (paragraph joins, link and tag matching, `x_weighted` weights), so "OK" in the composer is never "too long" on the server. Tests in Task 5 share core's vectors.

## Decisions taken in this plan

- **Queue rows need the post (call, small backend change):** `PublishJobSummaryDto` gains `text_head`, `campaign_id`, and `account_ids` (in the caller's limit), as `CalendarItemDto` already has, so a list row needs no request per job. `text_head` is the first line of the post's **current** text, uncapped, exactly as the calendar computes it (`split_part(p.text, E'\n', 1)`); the UI truncates it with CSS. A server cap is not used because Postgres `left()` counts code points and would cut emoji sequences. A row whose `text_head` is empty (a media-only post) shows `queue:noText` ("No text"). `GET /publish-jobs` gains `phase=open|finished`: `open` is `scheduled` and `publishing`, soonest `scheduled_at` first; `finished` is every other status, newest first. `status` still narrows within a phase.
- **Routes (call):** `/compose` (drafts list and "New post"), `/compose/:postId`, `/queue`, `/history`, `/publish-jobs/:id`, `/media`, `/campaigns`, `/brand`. `/jobs…` stays the admin Jobs console.
- **Drafts list (call):** `/compose` lists the workspace's posts (`GET /posts`, filter by status, default `draft`) so a saved draft can be found again; the spec names no other place for it.
- **Save model (call):** "Save draft" is explicit; there is no autosave. The composer warns on leaving with unsaved changes (`useBlocker`). A new post is created by `POST /posts` on first save, then `PATCH /posts/{id}` with `If-Match` for the base fields and `PUT`/`DELETE /posts/{id}/overrides/{account_id}` (also `If-Match`, each answer carrying the next `ETag`) for each changed override, sent in sequence.
- **Publish needs a saved post:** the dialog opens from the composer only when nothing is unsaved; "Save and publish" saves first and opens the dialog on the new `ETag`.
- **Validation:** "Check" runs `POST /posts/validate` (unsaved) or `POST /posts/{id}/validate` (saved and clean). After the user has checked once, the composer re-validates 800 ms after the last change (spec). The dialog validates with its own `at`, `native_schedule`, and targets before enabling the button, debounced the same way.
- **Validation shares the publish bucket:** both validate routes charge `rate_limit.publish_actions` (60 per minute, burst 20), the same bucket as publish, reschedule, cancel, retry, and resolve. A `429 rate_limited` on a check keeps the last result, shows the wait from `Retry-After` once, and pauses automatic re-checks until it has passed; it never retries in a loop, so typing cannot drain the bucket the Publish click needs.
- **The live counter (call):** `measureText(text, measure)` ports `measure` in `backend/crates/core/src/validation/text.rs` exactly: `graphemes` (`Intl.Segmenter`), `utf16_units`, `bytes` (`TextEncoder`), and `x_weighted` (NFC through `String.prototype.normalize`, then per whitespace run: a run starting with `http://` or `https://` in any case counts 23 plus its trailing `.,;:!?)]}'"` graphemes; otherwise an emoji sequence (two or more code points, one of them ZWJ, VS16, U+20E3, a regional indicator, or a skin tone) counts 2 and every other code point counts 1 inside U+0000–U+10FF, U+2000–U+200D, U+2010–U+201F, U+2032–U+2037, and 2 elsewhere). When the format's `includes_link_and_tags` is true, the counted string is `post_text` in `backend/crates/core/src/compose.rs`, ported exactly: the text, then the link unless the text holds it as a whole word (trimming the punctuation in `AROUND_A_LINK`, Arabic marks included, and a trailing `/` on both sides), then the `#tags` the text does not already hold as whole tags (case-insensitive; a tag counts as held when the next character is not alphanumeric or `_`), tags joined by one space, the parts joined by `"\n\n"`. The link is counted as `new URL(link).href`, the WHATWG form that matches `Url::to_string()` (a bare host gains `/`). The counter only informs; validity is the server's.
- **"Created by" (call):** `created_by` is a user ID; the job page shows "You" for the caller, the owner's name for the owner (from `useWorkspace().current`), the delegate's name from `useGranted` (`GET /delegations`) in the own workspace, and "Someone with access" otherwise. No new route.
- **Uploads (spec):** small files (≤ `chunk_max` returned by the session) still go through a session, so every upload has one code path; one upload at a time per tab, the rest queued.
- **Campaign colors (call):** a fixed palette of eight tokens (`ColorSwatchPicker`), stored as `#RRGGBB`; the API accepts any string.
- **Workspace switches (call):** the upload queue subscribes to `subscribeActAs` (`web/src/workspace/store.ts`) and aborts the running upload on a change; its items become `stalled` and their stored entries keep their `owner`, so they resume only after switching back. `web/src/workspace/dirtyGuard.ts` holds one "unsaved work" flag the composer sets while `isDirty`; `WorkspaceSwitcher` asks through `ConfirmDialog` before `switchTo` while it is set. A switch the server forces (`delegation_invalid`) cannot be asked about; the composer then keeps the unsaved `base` in `sessionStorage` under `postit.unsavedDraft` (text and fields only, never media URLs) and offers it once in the "Your unsaved changes" panel the next time the composer opens in the same workspace.
- **Success notices (call):** the web app has no toast library, and D4a reports outcomes with an inline `role="status"` notice. The composer does the same after a publish: a notice above the editor with the outcome and a link to the job. No `sonner`.
- **The schedule zone:** `PreferencesDto.timezone` is always set and defaults to `"UTC"`; there is no "no zone" state. The publish dialog uses the saved zone. When it is `UTC` and the browser's zone differs, the dialog shows D4a's `TimeZoneSetting` above the time inputs (its existing one-time question), so the user can save the browser's zone before picking a time.
- **A clock seam (picks up a D4a deferred entry):** `web/src/lib/clock.tsx` provides `ClockProvider` and `useNow(intervalMs)`; the countdown, the schedule picker's "now", and polling back-off read it, so tests advance time without fake timers fighting MSW.

## Deferred and verification items

- Picked up: `2026-11-27-d4a-workspace-accounts-deferred.md`, "The checklist countdown tick has no test" (Task 2). Its other entry ("`identity_unreachable` is never raised", trigger D6 or G5) stays open, so the file keeps its name.
- Evaluated, no change: `2026-11-01-c3-telegram-agent-deferred.md`, "Telegram counts characters, not UTF-16 units". Its trigger is "phase D4 if a user hits the early stop". The composer's counter uses the descriptor's `utf16_units`, so an emoji-heavy Telegram text shows "too long" at 2,048 emoji, the same point the server stops; no user has reported it. Task 13 adds one line under the entry saying D4b checked it and the trigger did not fire; the entry stays open.
- Evaluated, not in D4b: `2026-10-30-c1-c2-contacts-email-agent-deferred.md`, "Client catalog entries for the new violation codes and the DNS finding codes". Its trigger names D4, and the spec gives the web half (the DNS finding catalog on the email setup screen) to D4d. D4b adds no DNS screen; the entry stays open.
- Not in D4b: `fit` and `trim` fixes stay hints until E4; email content waits for D4d; the AI panel and random mode for D4c.
- Human checks: before starting, remind the maintainer that `2026-11-27-d4a-workspace-accounts-verification.md` (the D4a Playwright run and the hand walk-through) is still open although D4a is merged. `2026-11-27-consolidated-verification.md` has no D4b-triggered item. This slice's own Playwright run goes to its verification file (Task 13).

---

## File Structure

Backend:
- `backend/crates/data/src/publishing/jobs.rs` — `JobFilter.phase`, the summary's `text_head`, `campaign_id`, `account_ids`.
- `backend/crates/api/src/dto_publishing.rs`, `routes/publishing.rs` — `phase` query, summary fields.

Web:
- `web/src/lib/clock.tsx` — `ClockProvider`, `useNow`.
- `web/src/workspace/dirtyGuard.ts` — the "unsaved work" flag the switcher checks; `paths.ts` — `KEEP_ON_SWITCH` gains the new list pages.
- `web/src/components/ui/` — `radio-group.tsx`, `accordion.tsx` (shadcn over the `radix-ui` package already installed).
- `web/src/components/kit/` — `ViolationList.tsx`, `ScheduleInput.tsx`, `UploadProgress.tsx`, `MediaThumb.tsx`, `TextCounter.tsx`, `ColorSwatchPicker.tsx`, `textMeasure.ts`; `AccountPicker.tsx` gains disabled rows.
- `web/src/features/accounts/queries.ts` — `useAllAccounts`.
- `web/src/features/media/` — `upload.ts` (the resumable uploader), `uploadStore.ts`, `queries.ts`, `MediaPage.tsx`, `MediaPicker.tsx`.
- `web/src/features/brand/` — `queries.ts`, `BrandProfilePage.tsx`.
- `web/src/features/campaigns/` — `queries.ts`, `CampaignsPage.tsx`, `CampaignDialog.tsx`, `CampaignSelect.tsx`.
- `web/src/features/compose/` — `state.ts` (reducer), `ComposerProvider.tsx`, `queries.ts`, `DraftsPage.tsx`, `ComposerPage.tsx`, `PostEditor.tsx`, `TargetPicker.tsx`, `TargetTab.tsx`, `OptionsForm.tsx`, `fixes.ts`, `useValidation.ts`.
- `web/src/features/publish/` — `PublishDialog.tsx`, `queries.ts`.
- `web/src/features/queue/` — `queries.ts`, `QueuePage.tsx`, `HistoryPage.tsx`, `JobFilters.tsx`, `PublishJobPage.tsx`, `DeliveryRow.tsx`, `DeliveryActions.tsx`, `createdBy.ts`.
- `web/src/i18n/en-US/{media,brand,campaigns,compose,publish,queue}.json`.
- `web/e2e/d4b.spec.ts`.

---

### Task 1: The publish-job list carries what a queue row shows

**Files:**
- Modify: `backend/crates/data/src/publishing/jobs.rs` (`JobFilter`, `PublishJob` list rows, `list`)
- Modify: `backend/crates/api/src/dto_publishing.rs` (`PublishJobSummaryDto`), `backend/crates/api/src/routes/publishing.rs` (the list query struct and docs)
- Test: `backend/crates/data/tests/publishing_jobs.rs`, `backend/crates/api/tests/publishing.rs`

**Interfaces:**
- Produces:
  - `pub enum JobPhase { Open, Finished }` in `postit_data::publishing::jobs`, `JobFilter.phase: Option<JobPhase>`. `Open` matches `status IN ('scheduled', 'publishing')` ordered `scheduled_at ASC, id ASC`; `Finished` matches the rest ordered `created_at DESC, id DESC` (today's order). No phase keeps today's order.
  - The list row gains `text_head: String` (the first line of the post's current text, uncapped, `split_part(p.text, E'\n', 1)` as the calendar query at `jobs.rs:330` does; the list joins `posts` for it and for `campaign_id`; document on the DTO that it is the current text, not the text of the job's `post_version`), `campaign_id: Option<CampaignId>`, `account_ids: Vec<AccountId>` (deliveries' accounts inside `scope.account_filter()`).
  - `PublishJobSummaryDto` gains `text_head: String`, `campaign_id: Option<Uuid>`, `account_ids: Vec<Uuid>` (all required in the schema; `campaign_id` nullable). `PublishJobDto` inherits them.
  - `GET /api/v1/publish-jobs?phase=open|finished` (`JobPhaseDto`, snake_case), documented; an unknown value is `422 validation_failed` as other enums.
- [ ] **Step 1: Write failing tests:** data `phase_open_lists_scheduled_and_publishing_soonest_first`, `phase_finished_lists_the_rest_newest_first`, `summary_carries_text_head_campaign_and_accounts_in_limit` (a limited delegate sees only their accounts in `account_ids`); api `list_jobs_phase_filter`, `list_jobs_rows_have_text_head`, `list_jobs_bad_phase_is_422`.
- [ ] **Step 2: Run** `cargo test -p postit-data --test publishing_jobs -- --test-threads=4` and `cargo test -p postit-api --test publishing` — fail.
- [ ] **Step 3: Implement**; `cargo sqlx prepare`; `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests and `cargo xtask openapi --check` — pass; the web gate still passes with the regenerated `schema.d.ts`.
- [ ] **Step 5: Commit** `feat: list publish jobs by phase with their post's first line`.

### Task 2: Clock seam and the countdown tick test

Picks up `docs/plans/2026-11-27-d4a-workspace-accounts-deferred.md`, "The checklist countdown tick has no test".

**Files:**
- Create: `web/src/lib/clock.tsx`
- Modify: `web/src/features/accounts/connect/DiscoveryChecklist.tsx` (read minutes left through `useNow(15_000)`), `web/src/test/render.tsx` (option `clock?: TestClock`)
- Test: `web/src/lib/clock.test.tsx`, `web/src/features/accounts/connect/ConnectPage.test.tsx`

**Interfaces:**
- Produces: `interface Clock { now(): number; subscribe(fn: () => void, intervalMs: number): () => void }`; `systemClock` (`Date.now`, `setInterval`); `ClockProvider({ clock, children })`; `useNow(intervalMs: number): number` (re-renders on each tick of the provided clock; defaults to `systemClock` without a provider); test helper `createTestClock(start: number): Clock & { advance(ms: number): void }` in `web/src/test/clock.ts` (ticks every subscriber whose interval elapsed).
- [ ] **Step 1: Write failing tests:** `useNow ticks with the test clock`; in `ConnectPage.test.tsx`, `the checklist countdown goes down and closes at zero` (render with an expiry 2 min out, `advance(60_000)` shows 1 min, `advance(60_000)` shows "This connection expired. Start again.").
- [ ] **Step 2: Run** `pnpm exec vitest run src/lib src/features/accounts/connect` (PowerShell, in `web/`) — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass. In the D4a deferred file, mark the entry **resolved in D4b (Task 2)** with the test name; the file keeps its name (one entry open).
- [ ] **Step 5: Commit** `feat(web): a clock seam, and test the checklist countdown`.

### Task 3: Resumable uploads

**Files:**
- Create: `web/src/features/media/upload.ts`, `uploadStore.ts`, `queries.ts`
- Test: `web/src/features/media/upload.test.ts`, `uploadStore.test.ts`

**Interfaces:**
- Consumes: the API client (`web/src/api/client.ts`), `wsKey`, `Clock` (Task 2) for back-off waits.
- Produces:
  - `uploadStore.ts`: `UPLOADS_KEY = 'postit.uploads'`; `interface StoredUpload { sessionId: string; owner: string | null; name: string; size: number; lastModified: number; createdAt: number }`; `saveUpload(u)`, `findUpload(owner, file: File): StoredUpload | null` (match on owner, name, size, `lastModified`), `dropUpload(sessionId)`. `sessionStorage` only, try/catch, in-memory fallback. Entries older than 24 h are dropped on read (the server's `media.upload_session_ttl` default).
  - `upload.ts`: `uploadFile(file: File, opts: { owner: string | null; signal?: AbortSignal; onProgress(sent: number, total: number): void; clock?: Clock }): Promise<MediaDto>`. The upload's session ID is also its media ID (the server creates the media row, `status: uploading`, at the start).
    1. A stored session for this file → `HEAD /media/uploads/{id}`; read the `Upload-Offset` response header (exposed by CORS). `404` → drop it and start over.
    2. Otherwise `POST /media/uploads` `{ filename: file.name, size: file.size }` with an `Idempotency-Key` (a fresh UUID per file, kept for that file's retries of this call, so a start retried after a lost answer does not reserve quota twice) → `UploadSessionDto`; store it.
    3. While `offset < size`: `PUT /media/uploads/{id}` with `chunk = file.slice(offset, offset + chunk_max)` (end exclusive), `Content-Type: application/octet-stream`, `Content-Range: bytes {offset}-{offset + chunk.size - 1}/{size}` (end **inclusive**), and the `Blob` sent as is (`bodySerializer: (b) => b`; `openapi-fetch` would JSON-encode it otherwise); the answer's `offset` is the next start.
    4. A network error, `5xx`, or `408 request_timeout` waits (1 s, 2 s, 4 s … capped at 30 s, full jitter) and re-reads the offset with `HEAD` before resending; `429 rate_limited` waits for `Retry-After` instead. After 8 failures in a row it throws `UploadStalled` (the store keeps the session, so the user can resume). `409 upload_offset_mismatch` re-reads with `HEAD` and continues, without counting as a failure; a second mismatch in a row throws.
    5. `POST /media/uploads/{id}/complete` → `MediaDto`; drop the stored entry. The server keeps checking after a timed-out `complete`, and any failure other than `415` leaves the upload open: a network error, `5xx`, or `408` on `complete` reads `GET /media/{id}` and returns it when its status is `processing` or `ready`, and otherwise sends `complete` again under the same back-off.
    6. `413 storage_quota_exceeded` and `415 unsupported_media_type` throw `ApiProblem` at once and drop the entry.
    7. An aborted `signal` stops after the request in flight and throws `AbortError`; the stored entry stays.
  - `useUploadQueue()`: `{ items: { id, file, owner, state: 'queued'|'uploading'|'stalled'|'failed'|'done', sent, total, error? }[]; add(files: File[]): void; retry(id): void; cancel(id): void }`, one upload running at a time per tab; each item records the owner active when it was added; `done` invalidates `wsKey(owner, 'media')`. Progress moves per chunk (fetch reports no upload progress). It subscribes to `subscribeActAs`: on a workspace change it aborts the running upload and marks it and every queued item of the old owner `stalled`; `retry` on an item whose owner is not the active one is disabled with "Switch back to that workspace to resume." `cancel(id)` aborts, then `DELETE /media/{id}` for a started session (the open media row goes with it), and drops the stored entry.
  - `queries.ts`: `useMediaList(page, pageSize)` (`GET /media`, key `ws('media', page, pageSize)`), `useMediaUsage()` (`ws('media', 'usage')`), `useViewUrls(id)` (`GET /media/{id}/view-url` → `ViewUrlsDto { content_url, thumbnail_url, expires_at }`, `staleTime: 8 * 60_000`, since URLs live 10 minutes), `useDeleteMedia()`.
- [ ] **Step 1: Write failing tests** (MSW handlers keep a byte offset per session):
  - `uploads in chunks of chunk_max and completes`.
  - `a dropped chunk resumes from HEAD offset` (the second `PUT` fails with a network error once).
  - `offset mismatch re-reads and continues`; `two mismatches in a row stop`.
  - `a reload resumes the same file` (store entry present, `HEAD` says 50%, only the rest is sent).
  - `a different file with the same name starts over` (other `lastModified`).
  - `an expired session starts over` (`HEAD` 404).
  - `quota exceeded stops at once and drops the session`; `unsupported type stops at once`.
  - `eight failures in a row stall and keep the session`.
  - `content-range end is inclusive and the chunk is sent raw` (the handler checks `end - start + 1 === body length` and a non-JSON body).
  - `408 and 429 retry; 429 waits for Retry-After` (test clock).
  - `the start sends an idempotency key and reuses it on retry`.
  - `a timed-out complete reads the media and returns it when processing`.
  - `cancel deletes the started session`.
  - `the queue runs one upload at a time`.
  - `a workspace switch stops the upload and never starts one in the other workspace` (switch mid-file; no request after the switch carries the new owner's act-as header for this file).
  - `session ids never reach localStorage` (spy on `Storage.prototype.setItem`).
- [ ] **Step 2: Run** `pnpm exec vitest run src/features/media` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): resumable media uploads`.

### Task 4: Media library

**Files:**
- Create: `web/src/components/kit/MediaThumb.tsx`, `UploadProgress.tsx`; `web/src/features/media/MediaPage.tsx`, `MediaPicker.tsx`; `web/src/i18n/en-US/media.json` (registered)
- Modify: `web/src/routes.tsx` (`/media`), `web/src/workspace/paths.ts` (`/media` in `KEEP_ON_SWITCH`), `web/src/components/Shell.tsx` (nav item "Media")
- Test: `web/src/features/media/MediaPage.test.tsx`, `web/src/components/kit/kit.test.tsx`

**Interfaces:**
- Consumes: Task 3.
- Produces:
  - `MediaThumb({ media: MediaDto; size?: 'sm' | 'md' | 'lg'; selected?: boolean; onClick?() })`: the thumbnail from `useViewUrls(id).data.thumbnail_url` when `has_thumbnail`, else a kind icon; status badge for `uploading`, `processing`, `failed`; video duration overlay. The grid shows open sessions too (they are `uploading` media rows); `MediaPicker` lists only `ready` media.
  - `UploadProgress({ items, onRetry, onCancel })`: one row per queue item with `ProgressBar`, file name, state label, retry for `stalled`/`failed`.
  - Media page: `PageHeader` with "Upload" (edit) and a drop zone (edit); storage usage bar (`used_bytes` of `quota_bytes`, "Unlimited" at 0, formatted through `useFormat`); a paginated grid of `MediaThumb`; a detail `Sheet` with the facts (`FactList`: kind, size, dimensions, duration, file name, created) and Delete (edit) behind `ConfirmDialog`. `409 media_in_use` shows "This file is used by a post or a scheduled job, so it cannot be deleted yet." and keeps the item.
  - `MediaPicker({ open, onOpenChange, selected: string[], onConfirm(ids: string[]): void, kinds?: MediaKindDto[] })`: the same grid in a dialog with multi-select and an Upload button, used by the composer (Task 7) and later by the email editor (D4d).
- [ ] **Step 1: Write failing tests:** `grid shows thumbnails through view URLs`, `usage shows used of quota and unlimited at 0`, `view level sees no upload or delete`, `drop starts an upload and shows progress`, `delete asks first`, `media_in_use explains and keeps the item`, `picker returns the chosen ids in order`, kit tests for `MediaThumb` and `UploadProgress` in both directions.
- [ ] **Step 2: Run** `pnpm exec vitest run src/features/media src/components/kit` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): media library with resumable upload`.

### Task 5: Kit: ViolationList, ScheduleInput, TextCounter, ColorSwatchPicker

**Files:**
- Create: `web/src/components/kit/ViolationList.tsx`, `ScheduleInput.tsx`, `TextCounter.tsx`, `textMeasure.ts`, `ColorSwatchPicker.tsx`; `web/src/components/ui/radio-group.tsx` (shadcn, over `radix-ui`)
- Test: `web/src/components/kit/ViolationList.test.tsx`, `ScheduleInput.test.tsx`, `textMeasure.test.ts`, `kit.test.tsx`

**Interfaces:**
- Produces:
  - `ViolationList({ violations: Violation[]; targets?: TargetReportDto[]; accounts: AccountDto[]; onFix?(target: string | null, v: Violation): void })`: schedule-level violations first, then one group per target with `PlatformBadge` and the account name. Each line is `t('violations:<code>', params)`; an `agent_rule` violation shows its `params.message` through `agentText`. A `shorten_text` or `remove_media` fix renders a button calling `onFix`; `fit` renders the hint `t('compose:fix.fit', { aspect })` ("Needs a {aspect} crop"), `trim` `t('compose:fix.trim', { seconds })` with `seconds` from `TrimFix.max_ms / 1000` formatted through `useFormat`, never a button (E4). `path` shows as the field label when it names `text`, `link`, `tags`, `media[n]`, or `options.<name>` (`compose:field.*`).
  - `ScheduleInput({ value: ScheduleValue | null; onChange(v: ScheduleValue | null): void; zone: string; min?: number })` with `type ScheduleValue = { local: string; zone: string } | { instant: string }`: a date and a time input in the user's zone; `toScheduleInput(v): ScheduleInput` (`{ local, zone }` or the RFC 3339 string). `local` is always `YYYY-MM-DDTHH:MM:SS` (the inputs give minutes; `:00` is appended once there). `resolveAmbiguous(local, offset: string): ScheduleValue` normalizes `local` to seconds first (the violation's `params.local` already has them, the input's may not) and returns `{ instant: `${normalized}${offset}` }`; offsets come from `params.earliest_offset` / `params.latest_offset` (`±HH:MM`).
  - `textMeasure.ts`: `measureText(text: string, measure: TextMeasure): number`, an exact port per the decision above; `countedText(base: { text: string; link?: string | null; tags: string[] }, rules: TextRules): string`, an exact port of `post_text` when `rules.includes_link_and_tags`, else `base.text`.
  - `TextCounter({ count, max })`: "{count} / {max}", warning color past 90%, danger past `max`, `aria-live="polite"`.
  - `ColorSwatchPicker({ value, onChange })`: eight swatches as a `ui/radio-group`, keyboard reachable, with a "None" choice.
- [ ] **Step 1: Write failing tests:**
  - `ViolationList`: `groups by target with the platform badge`, `fills the message from params`, `shorten_text and remove_media fixes are buttons`, `fit and trim are hints only`, `agent_rule shows the agent text`, both directions.
  - `ScheduleInput`: `emits local with seconds and zone`, `resolveAmbiguous builds an RFC 3339 instant from a local with or without seconds`, `rtl keeps date before time in reading order`.
  - `textMeasure`: every row of core's `measures_table` (`backend/crates/core/src/validation/text.rs`) copied as a test vector for all four measures, plus graphemes count `"👩‍👩‍👧"` as 1, utf16 as 8, bytes as 18; `x_weighted` counts a URL as 23 plus trailing punctuation, an emoji sequence as 2, and a CJK character as 2.
  - `countedText`: the cases of core's `compose.rs` tests copied as vectors: parts joined by a blank line, tags by a space; a link already in the text (with surrounding punctuation or a trailing `/`) is not added; a tag already in the text in other case is not added, `#tag` inside `#tagline` is; a bare-host link is counted with its added `/`; no link and no tags returns the text unchanged.
  - `TextCounter` thresholds; `ColorSwatchPicker` selects by keyboard.
- [ ] **Step 2: Run** `pnpm exec vitest run src/components/kit` — fail.
- [ ] **Step 3: Implement** (catalog keys for the hints and field labels go in `compose.json`, created here and registered).
- [ ] **Step 4: Run** — pass; `kitGuard.test.ts` passes.
- [ ] **Step 5: Commit** `feat(web): violation list, schedule input, and text counter kit components`.

### Task 6: Brand profile and campaigns

**Files:**
- Create: `web/src/features/brand/queries.ts`, `BrandProfilePage.tsx`; `web/src/features/campaigns/queries.ts`, `CampaignsPage.tsx`, `CampaignDialog.tsx`, `CampaignSelect.tsx`; `web/src/i18n/en-US/{brand,campaigns}.json` (registered)
- Modify: `web/src/routes.tsx` (`/brand`, `/campaigns`), `web/src/workspace/paths.ts` (both in `KEEP_ON_SWITCH`), `Shell.tsx` (nav: "Campaigns" in the main group; "Brand profile" under SETTINGS)
- Test: `web/src/features/brand/BrandProfilePage.test.tsx`, `web/src/features/campaigns/CampaignsPage.test.tsx`

**Interfaces:**
- Produces:
  - `useBrandProfile()` (`ws('brand')`), `useSaveBrandProfile()` (`PUT /brand-profile` with every field). Page: description, audience, offerings (`Textarea`), words to avoid (a tag input: Enter adds, each removable), default language (`OptionSelect` over the supported locales in `web/src/i18n/locales.ts`, plus "None"), up to five examples (add and remove; "Add example" disabled at five). Read-only below `edit`.
  - `useCampaigns({ includeArchived, page, pageSize })` (`ws('campaigns', …)`), `useSaveCampaign()` (`POST /campaigns` or `PATCH /campaigns/{id}` sending only changed fields; `null` clears a date or color), `useDeleteCampaign()`.
  - Campaigns page: `DataTable` (color swatch, name, dates, archived badge), "Show archived" in the URL; create and edit dialog (name, start, end, `ColorSwatchPicker`); `422 validation_failed` with `field: "ends_on"` marks the end date; archive and restore; delete behind `ConfirmDialog` "Posts in this campaign stay; they just leave it." Edit actions need `edit`.
  - `CampaignSelect({ value: string | null; onChange(id: string | null): void; allowNone?: boolean })`: an `OptionSelect` over active campaigns (archived only when it is the current value), used by the composer and the job filters.
- [ ] **Step 1: Write failing tests:** brand `saves every field`, `sixth example is refused by the button`, `view level is read-only`; campaigns `create with dates and color`, `end before start marks the end date`, `archive hides it from the default list`, `delete asks and explains posts stay`, `view level sees no edit actions`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): brand profile and campaigns`.

### Task 7: Composer state, drafts list, and the post editor

**Files:**
- Create: `web/src/features/compose/state.ts`, `ComposerProvider.tsx`, `queries.ts`, `DraftsPage.tsx`, `ComposerPage.tsx`, `PostEditor.tsx`; `web/src/workspace/dirtyGuard.ts`
- Modify: `web/src/routes.tsx` (`/compose`, `/compose/:postId`), `web/src/workspace/paths.ts` (`/compose` in `KEEP_ON_SWITCH`), `web/src/workspace/WorkspaceSwitcher.tsx` (asks while `dirtyGuard` is set), `Shell.tsx` (nav "Compose")
- Test: `web/src/features/compose/state.test.ts`, `ComposerPage.test.tsx`, `DraftsPage.test.tsx`

**Interfaces:**
- Produces (the D4c AI panel and the D4d email editor build on these names):
  - `state.ts`:
    ```ts
    interface BaseFields { text: string; link: string | null; tags: string[]; media: { media_id: string; alt_text: string | null }[]; disclose_ai_generated: boolean; campaign_id: string | null }
    interface TargetOverride { patch: PostOverride; options: Record<string, unknown> }
    interface ComposerState {
      postId: string | null; etag: string | null; version: number | null; status: PostStatusDto | null
      base: BaseFields; targets: string[]; overrides: Record<string, TargetOverride>
      generationId: string | null
      saved: { base: BaseFields; targets: string[]; overrides: Record<string, TargetOverride>; generationId: string | null } | null
      lastPublished: LastPublishedDto[]
    }
    type ComposerAction =
      | { type: 'load'; post: PostDto; etag: string }
      | { type: 'setBase'; patch: Partial<BaseFields> }
      | { type: 'setTargets'; targets: string[] }
      | { type: 'setOverride'; account: string; override: TargetOverride | null }
      | { type: 'applyDraft'; draft: AiDraftDto; generationId: string; sent: string[] }   // used by D4c
      | { type: 'saved'; post: PostDto; etag: string }
    ```
    `reduce(state, action)`, `emptyState()`, `isDirty(state): boolean`, `changedOverrides(state): { put: string[]; remove: string[] }`. `setTargets` drops overrides of accounts no longer targeted. `applyDraft` first pins each target that is **not** in `sent` and has no override to the current shared post (an override holding the current `base.text`, `tags`, and `link`), because `draft.post` carries the first sent target's text and would otherwise change targets the user did not generate for; then it sets `base.text/tags/link` from `draft.post`, `disclose_ai_generated: true`, each `draft.overrides[account]` into `overrides[account].patch` (keeping that target's `options`), and `generationId`. D4c (`2026-11-28-d4c-ai.md`, "Unsent targets keep their text") relies on this.
    - **The saved generation:** `saved.generationId` is the generation the server already links to this post. `load` sets it to `null` (the post DTO does not carry it; a re-link is accepted, see D4c Task 1), and `saved` and the base `savedStep` set it to `state.generationId`. `isDirty` ignores it. D4b's `save()` sends `generation_id` on `POST /posts` only; D4c Task 5 adds it to `PATCH /posts/{id}`, sent only when `state.generationId !== saved.generationId`, once D4c Task 1 gives the patch that field.
    - **One home for options:** core's `PostOverride` has its own `options` member, and `PutOverrideRequest` carries `options` beside `patch`; the server stores both as sent. `TargetOverride.patch` never holds `options`: `load` moves nothing into it (the server's `PostOverrideDto.options` goes to `TargetOverride.options`), and `setOverride` and `applyDraft` strip `patch.options`. `PUT` sends options top-level only.
    - **`saveStep`:** an extra action `{ type: 'savedStep'; post: PostDto; etag: string; part: 'base' | { account: string } }` records one finished write in `saved` and `etag`, so a save that stops halfway resumes with only what is still unsaved.
  - `ComposerProvider({ postId })` + `useComposer(): { state; dispatch; save(): Promise<void>; saving: boolean; conflict: BaseFields | null; clearConflict(): void }`. `save()`: new post → `POST /posts` (with `generation_id` when set; the answer's `ETag` header is stored); saved post → `PATCH /posts/{id}` with the changed base fields and targets (`If-Match: etag`; `null` clears `link` and `campaign_id`), then each `put` override (`PUT /posts/{id}/overrides/{account}` with `{ patch, options }`) and each `remove` (`DELETE`), each with the latest `ETag`, each followed by `savedStep`; then `dispatch({ type: 'saved' })`. A failure stops the sequence with the finished steps kept: `412 version_conflict` refetches the post, keeps the user's `base` in `conflict` (shown as "Your unsaved changes" with a Copy button), and loads the server's version; `409 post_locked` shows "This post is still publishing. Finish or cancel that run first." with the `/queue?post=<id>` link; anything else shows through `messageFor` and leaves the rest unsaved for the next "Save draft". While `isDirty`, the provider sets `dirtyGuard`, and after a forced switch it keeps `base` in `sessionStorage` per the "Workspace switches" decision.
  - `usePost(id)` (`ws('posts', id)`, keeps the `ETag` header), `usePosts({ status, campaign, page, pageSize })` (`ws('posts', …)`, always `kind: 'post'`, so E2's comment replies stay out), `useDeletePost()` (`DELETE /posts/{id}` with `If-Match`; `publish` level when the post has live history, so the button checks `atLeast('publish')` when `status !== 'draft'`).
  - Drafts page (`/compose`): "New post" (edit) and a `DataTable` of posts (first line, status, campaign, updated), status filter defaulting to `draft`, rows open `/compose/{id}`.
  - Post editor: text (`Textarea`) with one `TextCounter` per distinct target limit (the tightest first, from each target's selected format; see Task 8), link, tags (tag input), media (`MediaPicker`, then a reorderable list with alt text per item, move with buttons for keyboard users), AI disclosure switch, `CampaignSelect`. Below `edit` everything is read-only. A leave with unsaved changes asks first (`useBlocker`).
- [ ] **Step 1: Write failing tests:**
  - `state.test.ts`: `setTargets drops overrides of removed targets`, `applyDraft fills base and overrides and marks disclosure`, `applyDraft pins unsent targets to the old shared text`, `saved records the generation`, `isDirty after an edit and clean after saved`, `changedOverrides lists puts and removes`.
  - `state.test.ts` also: `override patches never carry options`, `savedStep keeps finished parts out of the next save`.
  - `ComposerPage.test.tsx`: `first save creates the post`, `next save patches with If-Match from the ETag header`, `changed overrides are put after the patch with the newer ETag`, `options are sent top-level, not in the patch`, `an override failing after the patch resumes with only the override`, `post_locked on save explains and links to the queue`, `save conflict reloads and keeps the unsaved text`, `leaving with unsaved changes asks`, `switching workspace with unsaved changes asks`, `a forced switch keeps the unsaved text for the next visit`, `view level is read-only`, `rtl render` (DOM snapshot: `dir`, logical classes).
  - `DraftsPage.test.tsx`: `lists drafts by default`, `asks for posts only, not comments`, `edit level sees New post; view does not`.
- [ ] **Step 2: Run** `pnpm exec vitest run src/features/compose` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): composer state, drafts, and the post editor`.

### Task 8: Targets, overrides, publish options, and validation with fixes

**Files:**
- Create: `web/src/features/compose/TargetPicker.tsx`, `TargetTab.tsx`, `OptionsForm.tsx`, `fixes.ts`, `useValidation.ts`; `web/src/components/ui/accordion.tsx` (shadcn, over `radix-ui`)
- Modify: `ComposerPage.tsx` (targets and tabs), `web/src/components/kit/AccountPicker.tsx` (disabled rows), `web/src/features/accounts/queries.ts` (`useAllAccounts`), `web/src/i18n/en-US/compose.json`
- Test: `web/src/features/compose/TargetPicker.test.tsx`, `TargetTab.test.tsx`, `fixes.test.ts`, `useValidation.test.tsx`, `web/src/components/kit/kit.test.tsx` (`AccountPicker` disabled rows in both directions), `web/src/features/accounts/queries.test.tsx`

**Interfaces:**
- Consumes: `AccountPicker`, `PlatformBadge`, `SchemaForm`/`parseSchema`/`initialValues`/`submittedValues` (D4a), `ViolationList`, `TextCounter`, `measureText`, `countedText` (Task 5), `useComposer` (Task 7), `useTargetGroups` (D4a).
- Produces:
  - `AccountPicker` gains `disabled?: (account: AccountDto) => string | null`: a row it answers for renders its checkbox disabled with that reason beside the name (`aria-describedby`).
  - `useAllAccounts({ includeDisconnected?: boolean })` (`ws('accounts', 'all', …)`): `GET /accounts` at `page_size=100` (the server's maximum), fetching the next page while `page * page_size < total`, returning one array. D4a's `{ page: 1, pageSize: 100 }` callers stay as they are; the composer and the queue use this.
  - `TargetPicker`: accounts in the caller's limit (`useAllAccounts`, `AccountPicker`, active accounts only; `needs_reconnect` shown disabled with its status) and target groups as chips; picking a group adds its visible accounts (the client expands groups, spec).
  - `selectedFormat(descriptor: AgentDescriptor, options: Record<string, unknown>, media: MediaDto[]): FormatSpec | null`: the format whose `selected_by` option matches, else inferred as the validation engine does (no media → `text`; otherwise the first format whose media rules accept the media kinds and count), used only for the counter and the "Needs a video" hint, never for validity.
  - `TargetTab({ account })`: one tab per target (an accordion below the `md` breakpoint), headed by `PlatformBadge` and the account name. Inside: "Use the shared post" or "Change it for this account"; when changed, text (with that target's `TextCounter`), link, tags, and a media subset (checkboxes over the post's media, kept in post order). Then the publish options: `GET /accounts/{id}/publish-options` (`ws('publishOptions', id)`) through `OptionsForm` → `SchemaForm`, values in `overrides[account].options`. For an account whose descriptor kind is `direct`, the tab shows `t('compose:email.notYet')` ("Email content can be written once the email editor arrives.") instead of content fields; D4d fills it with the email editor through a render prop added here: `directContent?: (account: AccountDto) => ReactNode`.
  - `fixes.ts`: `applyFix(state, target: string | null, v: Violation): ComposerAction | { focus: 'text'; account: string | null; max: number } | null`: `remove_media { index }` → a `setOverride` (target) or `setBase` (shared) without that item (all items when `index` is absent); `shorten_text { max }` → open that target's override with the shared text copied in, then `{ focus }` so the editor selects the text and shows the limit; other fixes → `null`.
  - `useValidation()`: `{ result: ValidationDto | null; check(): Promise<ValidationDto>; checking: boolean }`: unsaved or dirty → `POST /posts/validate` with the full state (`overrides` keyed by account); saved and clean → `POST /posts/{id}/validate`. After the first `check()`, changes re-check 800 ms after the last one (debounced); results for an older state are dropped. A `429 rate_limited` keeps the last result, shows "Checks are paused for {seconds} s" from `Retry-After`, and schedules no re-check before that time (the publish bucket, see the decisions).
- [ ] **Step 1: Write failing tests:** `picking a group adds its visible accounts`, `useAllAccounts fetches every page`, `needs_reconnect accounts cannot be picked`, `a limited delegate sees only accounts in limit`, `override changes only that target`, `media subset keeps post order`, `options render from the account's schema and are sent in the override`, `consent option must be checked`, `direct account shows the email placeholder`, `remove_media fix drops the item`, `shorten_text fix opens the override with the limit`, `fit fix is a hint`, `check validates unsaved drafts with overrides keyed by account`, `re-check is debounced after the first check`, `a late result for an older state is dropped`, `a 429 keeps the last result and pauses re-checks until Retry-After` (test clock), `counter uses the selected format's measure and limit`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): composer targets, overrides, options, and validation fixes`.

### Task 9: Publish dialog

**Files:**
- Create: `web/src/features/publish/PublishDialog.tsx`, `queries.ts`, `web/src/i18n/en-US/publish.json` (registered)
- Modify: `ComposerPage.tsx` ("Publish" / "Save and publish")
- Test: `web/src/features/publish/PublishDialog.test.tsx`

**Interfaces:**
- Consumes: `useComposer`, `ScheduleInput`/`toScheduleInput`/`resolveAmbiguous`, `ViolationList`, `useAccess`, `usePlatforms`, `usePreferences` (zone).
- Produces: `PublishDialog({ open, onOpenChange })`, opened on a saved, clean post; `usePublish()` (`POST /posts/{id}/publish` with `If-Match: state.etag`).
  - **When:** Now or Later (`ScheduleInput` in the user's saved `timezone`; when that is `UTC` and the browser's zone differs, D4a's `TimeZoneSetting` shows above the inputs, per "The schedule zone" decision).
  - **Native scheduling:** shown when Later is chosen and at least one target's descriptor has `native_schedule` and the `native_scheduler` capability: a switch "Let platforms hold the post until then", and per target a line "Held by {platform}" or "Sent by postit at that time" (targets whose agent cannot, or whose time is outside `native_schedule.min_lead_secs`…`max_lead_secs`, show the second).
  - **Dry run:** a switch; at `edit` level it is on and locked with "You can test posts; publishing needs the publish level."
  - **Targets:** the post's targets in the limit, each checkable (at least one).
  - **Check:** opening runs `POST /posts/{id}/validate` with `{ at, native_schedule, targets }`, and each change re-runs it 800 ms after the last one, with `useValidation`'s `429` handling; violations show in `ViolationList` and disable the button. `schedule_time_ambiguous` shows two buttons, "{earliest_offset}" and "{latest_offset}", which call `resolveAmbiguous(params.local, offset)` and re-check.
  - **Live summary:** per target `PlatformBadge`, account name, format (from `TargetReportDto.format`), each consent option's text that the override sets; a warning "This version already went to {account} on {date}" (`date` from `LastPublishedDto.published_at`) when `lastPublished` has that account with `post_version === state.version`; a checkbox "I understand this publishes for real to {n} accounts" that enables the button. The request sends `confirm: true` only for live runs.
  - **Answers:** `201` → close and show the composer's inline `role="status"` notice "Scheduled for {time}" / "Publishing now" / "Test run started" with a link to `/publish-jobs/{id}` (no toast library, see the decisions); `412 version_conflict` → reload the post into the composer and show "This post changed elsewhere. Check it again before publishing." with the dialog reopened on the new `ETag`; `422 validation_failed` → its violations in the list; `409 post_locked` → "This post is still publishing. Finish or cancel that run first." with a link to `/queue?post=<id>`; `403 account_not_in_scope` and `capability_required` through `messageFor`.
- [ ] **Step 1: Write failing tests:** `publish now sends confirm only for live`, `edit level can only dry-run`, `the button waits for the confirmation tick`, `already-published warning names the account`, `schedule later sends local and zone`, `ambiguous time asks for the offset and resends an instant`, `native scheduling shows which targets the platform holds`, `violations disable the button`, `publish conflict reloads the dialog`, `post_locked links to the queue`, `consent texts show in the summary`, `a UTC zone with another browser zone shows the zone question`, `a success shows the status notice with the job link`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): publish dialog with confirmation and scheduling`.

### Task 10: Queue and history

**Files:**
- Create: `web/src/features/queue/queries.ts`, `QueuePage.tsx`, `HistoryPage.tsx`, `JobFilters.tsx`, `web/src/i18n/en-US/queue.json` (registered)
- Modify: `web/src/routes.tsx` (`/queue`, `/history`), `web/src/workspace/paths.ts` (both in `KEEP_ON_SWITCH`), `Shell.tsx` (nav "Queue" and "History")
- Test: `web/src/features/queue/QueuePage.test.tsx`, `HistoryPage.test.tsx`

**Interfaces:**
- Consumes: Task 1's list; `useListParams`, `DateRangeFilter`, `CampaignSelect`, `PlatformBadge`.
- Produces: `usePublishJobs({ phase, status, campaign, dryRun, from, to, post, page, pageSize })` (`ws('publishJobs', …)`; the queue polls every 30 s while visible). `JobFilters`: status (the phase's statuses only), campaign, test runs (all, only, none), date range, all in the URL; `?post=<id>` narrows to one post (from the dialog's `post_locked` link) and shows a chip to clear it. Both pages are `DataTable`s: first line (`text_head`, truncated with CSS; `queue:noText` when empty), target badges (from `account_ids`, joined with `useAllAccounts({ includeDisconnected: true })` from Task 8), scheduled time in the user's zone, status label (`queue:jobStatus.<status>`, every `JobStatus`), test-run badge, campaign swatch; a row opens `/publish-jobs/{id}`. An empty queue offers "Compose a post" (edit).
- [ ] **Step 1: Write failing tests:** `queue asks for phase open`, `history asks for phase finished`, `filters live in the URL and reset the page`, `post filter chip clears`, `rows show the first line, targets, and status`, `an empty first line shows No text`, `every job status has a label`, `limited delegate sees only their account badges`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): publishing queue and history`.

### Task 11: Job detail and delivery actions

**Files:**
- Create: `web/src/features/queue/PublishJobPage.tsx`, `DeliveryRow.tsx`, `DeliveryActions.tsx`, `createdBy.ts`
- Modify: `web/src/routes.tsx` (`/publish-jobs/:id`), `web/src/features/notifications/links.ts` (delivery and job kinds), `links.test.ts`
- Test: `web/src/features/queue/PublishJobPage.test.tsx`, `DeliveryActions.test.tsx`, `createdBy.test.ts`

**Interfaces:**
- Consumes: `useNow` (Task 2) for the poll back-off, `ScheduleInput`, `useAccess`, `useWorkspace`, `useGranted` (D4a `features/delegations/queries.ts`, own workspace only).
- Produces:
  - `usePublishJob(id)` (`ws('publishJobs', 'one', id)`, keeps the job `ETag`): polls while any delivery is non-terminal, every 5 s for the first minute, then 10 s, 20 s, capped at 30 s; stops when all are terminal.
  - Job page: header with the post's first line (link to `/compose/{post_id}`), status, scheduled time, test-run badge, native-scheduling badge, created by (`createdByLabel(id, ctx)` → "You", the owner's name, a delegate's name, or "Someone with access"). Job actions (`publish` level, whole job in limit): **Reschedule** (only while status `scheduled`; `ScheduleInput`; `PATCH /publish-jobs/{id}` with `If-Match`; a `422` with `schedule_time_ambiguous` asks for the offset as the publish dialog does, and `schedule_time_nonexistent` or `schedule_in_past` shows in `ViolationList`; the answer is a bare `RescheduleOutcomeDto` with no `ETag`, so the job query is invalidated to get the next one; its `failed` list shows "{n} posts held by the platform kept their old time"), **Cancel job** (`POST /publish-jobs/{id}/cancel`, `ConfirmDialog`; the `CancelReportDto` is summarized: canceled, stopping at the next safe point, cannot be canceled).
  - `DeliveryRow`: `PlatformBadge` and account name, state (`queue:deliveryState.<state>`, every `DeliveryStateDto`), parking reason (`queue:parkReason.<reason>`, every `ParkReason`, each with one line of what clears it), remote link (`remote_url`, opens in a new tab with `rel="noopener noreferrer"`), last error (`queue:errorClass.<class>` plus the code), next attempt time, native publish time, direct-send progress ("{sent} of {audience} sent, {failed} failed, {bounced} bounced, {skipped} skipped").
  - `DeliveryActions` (`publish` level, account in limit), shown by state per plan 03:
    - **Cancel:** `pending`, `parked` (`POST /deliveries/{id}/cancel`).
    - **Retry:** `dead_letter`, `failed`, `parked`, `outcome_unknown`; a dialog with the confirmation tick, sent as `confirm: true` (`RetryRequest.confirm`), and, for `outcome_unknown` or `parked` with `outcome_unresolvable`, a second tick "This post may already be live. Send it again anyway." (`acknowledge_outcome_unknown: true`).
    - **Resolve:** `outcome_unknown`, `parked` with `outcome_unresolvable`, `dead_letter`; "It is live" (`published`) or "It did not go out" (`failed`).
    - `409 not_cancelable`, `not_retryable`, `not_resolvable`, `job_active`, `not_reschedulable` show their message and refetch the job once; `400 acknowledgment_required` shows the second tick; `400 confirmation_required` shows the first tick again (a client bug if it happens, never swallowed).
  - `linkFor`: `delivery_dead_lettered`, `delivery_failed`, `delivery_outcome_unknown`, `delivery_parked`, `delivery_stale`, `delivery_published` (payload `DeliveryNotice`) and `job_partial` (payload `JobNotice`) → `{ path: '/publish-jobs/{job_id}', scope: n.owner_id ? 'owner' : 'self' }`, the scope rule the account kinds already use; `recurrence_skipped` and `new_comments` stay `null` (E1, E2).
- [ ] **Step 1: Write failing tests:** `polls while deliveries run and stops when all end`, `backs off to 30 seconds` (test clock), `every delivery state, park reason, and error class has a label`, `actions follow the state table` (a table test over every `DeliveryStateDto`), `retry sends confirm`, `outcome_unknown retry needs both ticks`, `resolve sends the chosen outcome`, `409 shows the message and refetches once`, `reschedule sends If-Match from the job ETag`, `reschedule refetches the job for its next ETag`, `an ambiguous reschedule time asks for the offset`, `reschedule reports platform-held posts that kept their time`, `cancel job summarizes the report`, `view level sees no actions`, `limited delegate sees only deliveries in limit`, `created by names you, the owner, a delegate, or someone`, `linkFor opens the job for delivery and job kinds` (update `linkFor covers every kind` so only `recurrence_skipped` and `new_comments` may be `null`), `direct progress line`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): publish job detail with delivery actions`.

### Task 12: Navigation and access sweep

**Files:**
- Modify: `web/src/components/Shell.tsx`, `Shell.test.tsx`, `web/src/features/home/HomePage.tsx` (a "Compose a post" action and the next three queued jobs, `phase=open`, `page_size=3`)
- Test: `Shell.test.tsx`, `HomePage.test.tsx`, `web/src/features/compose/access.test.tsx`

**Interfaces:**
- Nav after this slice, per the spec's sketch: Home; Compose; Queue; History; Media; Campaigns; Accounts; SETTINGS: Profile and preferences, Brand profile, Delegations; ADMIN unchanged.
- `access.test.tsx`: one table test rendering each new page (drafts, composer, media, brand, campaigns, queue, history, job detail) at owner, `view`, `edit`, `publish`, and a limited `publish` delegate, asserting each gated action is shown or hidden per the spec's "Access gating" list.
- [ ] **Step 1: Write failing tests** (nav order, home next-up list, the access table).
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): compose and publishing navigation`.

### Task 13: End-to-end spec, live check, docs, and wrap-up

**Files:**
- Create: `web/e2e/d4b.spec.ts`, `docs/plans/2026-11-28-d4b-compose-publish-verification.md`
- Modify: `CLAUDE.md` ("Project state": D4b built), `docs/contracts/03. publishing roadmap.md` (an "As built (D4b)" note under Phase D4 with this plan's decisions), `docs/plans/2026-11-01-c3-telegram-agent-deferred.md` (the one-line D4b note under "Telegram counts characters")

**Interfaces:**
- `d4b.spec.ts` (opt-in, `POSTIT_E2E`, `login` from `e2e/support.ts`), against the dev stack with the mock agent:
  1. The owner uploads an image (a generated 1 MB PNG) on the Media page and sees its thumbnail.
  2. The owner writes a brand profile and creates a campaign.
  3. The owner composes a post to the D4a mock account with the image, in the campaign, checks it, and saves.
  4. Dry run: the job ends `published` with `simulated` deliveries.
  5. Schedules it two minutes ahead (live), sees it in the queue, and waits until history shows it `published` with a remote link.
  6. `member@postit.com`, holding `publish` on that account, switches workspace, composes, dry-runs, and publishes live with the confirmation tick.
- [ ] **Step 1: Write** `d4b.spec.ts`; `pnpm exec playwright test --list` parses it.
- [ ] **Step 2: Live check.** If the Playwright plugin's browser tools are available, start `cargo dev` (from `backend/`) and `pnpm dev`, ask the maintainer to sign in as the owner in the driven browser, and walk steps 1–5 on `https://postit.local:44315`, including one upload interrupted by going offline (DevTools network throttling) and resumed. Fix any defect in this task. Whatever could not be checked goes to the verification file.
- [ ] **Step 3: Write** the verification file: (1) run `POSTIT_E2E=1 pnpm exec playwright test e2e/d4b.spec.ts` — why: needs the Zitadel passwords; trigger: before merging D4b. (2) an upload of a 200 MB video over a throttled connection with one reload in the middle, resumed — why: a real browser, a real file, and a person; trigger: before merging D4b. (3) any item Step 2 could not cover. Remind the maintainer of the open D4a verification file.
- [ ] **Step 4: Run** the full web gate and the backend gates (`fmt`, `clippy`, `check`, `xtask openapi --check`); all pass.
- [ ] **Step 5: Commit** `chore: document D4b and add its end-to-end spec`.

Any agent work found during execution that cannot be finished inside this plan goes to `docs/plans/2026-11-28-d4b-compose-publish-deferred.md` with the issue, why, and its trigger; human-only checks go to the verification file.
