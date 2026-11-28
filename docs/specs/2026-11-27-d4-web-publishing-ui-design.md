# D4 Web publishing UI: design

Plan 03 phase D4. Source of truth: `docs/contracts/03. publishing roadmap.md` (Phase D4, the REST surface, "Key flows", "Formats and validation", "Concepts", "Localization") and `docs/contracts/01. vision and architecture.md` (delegation levels, the whole-object rule, engineering standards). This document records how D4 is cut into slices, the decisions every slice shares, the small backend changes D4 needs, and the calls made where the plans leave room. It does not repeat the plans; read those sections first.

Decisions the maintainer made in conversation are marked **(decided)**. Everything else the plans do not state is marked **(call)** and is open to change in review.

## Intent

Everything below the screens is built: the REST surface (D3), the publishing engine (D1), the drafter (D2), contacts and the email agent (C1, C2), Telegram (C3), media (B4), delegation (B6), OAuth (B7), and the web component kit (D0b). The web app today has only home, profile, users, audit, and the jobs console. Nobody can publish anything without an HTTP client.

D4 turns the finished backend into a product a non-technical user can use from a browser: connect accounts, write or AI-draft a post, publish it now or later, send an email campaign, and watch the results, in their own workspace or through a delegation. It is the web half of release R1 (household use); D6 builds the mobile half on the same routes.

## Goals

The D4 bullet list in plan 03, in full, delivered in four slices **(decided)**, each ending in a flow that works end to end against the mock agent:

| Slice | Contents | Done when |
|---|---|---|
| **D4a Workspace and accounts** | nav restructure; workspace switcher and access gating; delegations (grant, edit, revoke, received, leave); preferences (language, time zone, notification switches); notifications bell and list; translated startup error; platform icons and colors; Accounts (credentials forms with guides, OAuth start and `/oauth-done` return, discovery checklist, pending connections, account detail with check, rename, test flag, disconnect, writing style); target groups; account CSV import (with the background import below); admin platforms, the live-publishing switch, per-user limits | a user connects mock accounts by credentials and by mock OAuth, imports accounts from CSV with progress, and a delegate sees only what their level and limit allow |
| **D4b Compose and publish** | media library (resumable upload, grid, delete blocked while in use, usage); brand profile; composer in controlled mode (base content, per-target overrides, the generic publish options form, validation with suggested fixes, campaign selection); publish dialog; queue and history with delivery rows and actions; campaigns (list, create, edit, archive, filters on queue and history) | a user composes a post, dry-runs it, schedules it, and sees it published on the mock agent, as owner and as a `publish` delegate |
| **D4c AI** | AI providers (presets with guides, custom providers, key entry, test, model list, default); generation history; the composer's AI panel and random mode; `store_ai_prompts` in preferences | a user adds a provider, drafts variants for several targets, edits them, and publishes |
| **D4d Email and onboarding** | contacts, lists, suppressions; contact CSV import (column mapping, consent column, attestation) and export; the email block editor with raw HTML and a live preview at desktop and mobile widths; send test email; the DNS check screen with the finding catalog; first-run setup and the dashboard checklist; the full Playwright exit spec | a user sends a campaign to a list through Papercut, and the D4 exit criteria hold |

One plan per slice, written in `docs/plans/` when that slice starts **(decided)**; this design covers all four.

## Non-goals

- The calendar (D5). The queue shows scheduled jobs as a list; month and week views come with D5.
- Recurrence editing (E1), the inbox (E2), analytics (E3), and media fitting (E4). A `fit` or `trim` suggested fix is shown as a hint until E4 adds `POST /media/{id}/fit`.
- Mobile screens (D6).
- New agents (C4, C5); the screens must show them without code changes when they arrive.
- Languages other than English. Every string is still in a catalog and every layout still mirrors (`english-only-rtl-ready`).

## Anti-goals

- **Platform-specific code in the client.** No `if (platform === 'telegram')`, no bundled platform logos, no hand-written per-platform forms. Icons, colors, forms, guides, formats, and limits come from `GET /platforms`; a new agent appears in every screen with no web change. A test fails when a feature file names a platform ID other than the mock ones in fixtures.
- **A second validation path.** The composer shows what `POST /posts/validate` or `POST /posts/{id}/validate` answers. The client may count characters for a live counter, using the format's `TextMeasure`, but never decides validity on its own.
- **Hiding a refusal.** An action the level or limit does not allow is hidden; a refusal the server still sends (a race, a revoked grant) is shown with its translated code, never swallowed.
- **Secrets in browser storage.** Nothing secret (credential values, API keys, `connect_id`) goes into `localStorage`, logs, or error reports. `connect_id` lives in `sessionStorage` for the checklist's 10 minutes only, as plan 03 says.
- **Hand-rolled tables, selects, footers, or danger zones.** Pages build from `web/src/components/kit/` (the `kitGuard` test); a missing widget becomes a kit component.

## Constraints

- Web stack as built: React 19, React Router 8, TanStack Query 5, `openapi-fetch` over the generated `schema.d.ts`, `oidc-client-ts`, i18next, Radix-based shadcn primitives, Tailwind 4 with logical properties, Vitest with MSW, Playwright for opt-in end-to-end runs. pnpm only, through PowerShell on this machine (`use-pnpm-never-npm`).
- New dependencies follow the dependency rules (maintained, permissive license, no Arabic-derived names) and the "prefer libraries" rule.
- Web gates: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`. Backend gates from the root `CLAUDE.md` for the slices that touch the backend. `cargo xtask openapi --check` after any route change.
- One migration file until G1: schema changes edit `0001_InitialSchema.sql` in place.
- Playwright end-to-end specs need the dev stack and a person's Zitadel passwords (`POSTIT_E2E`, `POSTIT_ADMIN_PASSWORD`, `POSTIT_MEMBER_PASSWORD`), so an agent writes them and the maintainer runs them; each slice's run is an entry in that slice's `-verification.md`.

## Shared decisions

### Navigation

The sidebar groups by what the user is doing **(call)**. Items appear in the slice that builds their screen.

```text
[ Workspace: My workspace  v ]     <- switcher (D4a)
  Home
  Compose                           (D4b)
  Queue · History                   (D4b)
  Media                             (D4b)
  Campaigns                         (D4b)
  Contacts                          (D4d)
  Accounts                          (D4a)
SETTINGS
  Profile and preferences           (existing page, extended)
  Brand profile                     (D4b)
  AI providers                      (D4c)
  Delegations                       (D4a, own workspace only)
ADMIN
  Users · Platforms · Publishing · Audit · Jobs
                                         [ bell ]  [ theme ]   <- header
```

The bell sits in the header next to the theme toggle. When acting in another workspace, a banner under the header reads "You are working in {owner}'s workspace ({level})" with a link back to the user's own.

### Workspace and act-as

- **Active workspace per tab (call).** The active owner ID is kept in `sessionStorage` (it is an ID, not a secret), so two tabs can work in two workspaces and a reload keeps the tab's choice. It is not in the URL, so routes stay flat and the act-as header stays one middleware's job.
- **One place adds the header.** `createAuthFetch` gains a workspace source and sets `X-Postit-Act-As` on every request while a delegated workspace is active, except for the self routes (`/me…`, `/notifications…`, `/push…`, `/delegations/received…`, `/users/lookup`, `/ai/presets`, `/auth/config`) and admin routes, which reject act-as (call: an allowlist of self and admin prefixes in one module, tested).
- **Cache isolation.** Every query key for workspace data starts with the active owner ID, and switching workspaces removes the workspace queries, so one workspace's data never shows in another.
- **A grant that ends** (`delegation_invalid` from any request) switches the tab back to the user's own workspace and says why.
- **Notifications** carry `owner_id`; opening one from another workspace switches to that workspace first, then follows the link.

### Access gating

A `useAccess()` hook, fed by `GET /me/workspaces`, answers `level` (`owner`, `view`, `edit`, `publish`), `can(action)`, and `inLimit(accountId)`. Pages hide actions with it; they never re-derive levels themselves (call). The rules come from the Access column of the REST surface:

- `owner` only (hidden when acting as): connecting, importing, disconnecting, renaming, and checking accounts; AI provider management; contact import and export; suppression removal; delegations; the email test send and DNS check.
- `edit`: composing, overrides, publish options, writing styles, target groups, brand profile, campaigns, contacts and lists, media upload and delete, AI generation, dry runs.
- `publish`: live publishing, reschedule, cancel, retry, resolve, deleting a post with live history.
- Account limit: accounts, targets, and deliveries outside the limit are not shown (the server already filters them); account pickers list only accounts in the limit.

### Text the agents send

Agent text arrives as `Message { key, en }` (labels, help, enum labels, consent text, guide steps). One helper, `agentText(message)`, looks the key up in an `agents` catalog namespace and falls back to `en` (call). The English catalog stays empty in D4: `en` is already English, and a future language adds keys there. The completeness test does not require agent keys, because new agents bring new keys.

### Text people write

Post text, overrides, tags, prompts, generated variants, names, and contact fields show and are edited with `dir="auto"` (plan 03 "Localization"), so content in another script keeps its own direction whatever the app's.

### The schema form

One kit component, `SchemaForm`, renders the options and credentials schema subset from `postit-core` (`backend/crates/core/src/schema/mod.rs`): string (`minLength`, `maxLength`), multiline string, enum with labels, boolean, consent boolean (`x-postit-consent-text`, must be checked), integer (`minimum`, `maximum`), date-time (in the user's zone, sent as RFC 3339 with offset), `x-postit-secret` (a password input with show and hide, never prefilled, never echoed), `x-postit-help`, `x-postit-no-default` (empty until chosen, counts as required), and `x-postit-depends-on` (shown only while the earlier field equals the value; a hidden field is not sent). An unknown keyword renders nothing for that field and logs a development warning, because the server rejects such schemas at startup anyway (call). It is used by the credentials forms (D4a) and the publish options (D4b); its tests cover every widget in both directions, the "every widget of the schema subset" test in plan 03.

### Errors and violations

- Problem codes render through the existing `messageFor` and `errors` catalog. New codes reached by D4 screens get entries; the completeness test already guards them.
- Violations render through one kit component, `ViolationList`, from the D3 `violations` catalog with their parameters, grouped per target, with the target's icon. Suggested fixes become buttons where the client can apply them: `shorten_text` opens that target's override with the text selected and the limit shown; `remove_media` removes the item from that target's override. `fit` and `trim` show as a hint ("Needs a 9:16 crop") until E4 (call).
- A `412 version_conflict` on any write with `If-Match` reloads the object and tells the user it changed elsewhere; nothing is retried blindly.
- `429 rate_limited` shows the wait from `Retry-After`.

### Times and zones

Times show in the user's `user_preferences.timezone` (the acting user's own zone when acting as, matching the server's schedule default), through the existing `useFormat`. The schedule picker sends `{ local, zone }` so the server resolves DST; `schedule_time_ambiguous` asks the user which of the two offsets they mean and resends with an explicit offset (call). A user with no zone set is asked once, preselecting the browser's zone (`Intl.DateTimeFormat().resolvedOptions().timeZone`); the preferences screen lists zones from `Intl.supportedValuesOf('timeZone')`.

### Polling

No server push to the browser beyond Web Push. Lists that change on their own poll while visible (call): the unread count every 60 seconds and on window focus (it has an ETag); a job detail with unfinished deliveries every 5 seconds, backing off to 30; an import every 2 seconds while running. TanStack Query's `refetchInterval` and `refetchOnWindowFocus` do this; nothing polls in a hidden tab.

### Tests, shared rules

- Component tests with MSW against the generated types, for every screen at each access level (owner, `view`, `edit`, `publish`, a limited delegate): the action is hidden or shown as the Access column says.
- Every new kit component has tests in both directions (`dir="ltr"` and `dir="rtl"`).
- The composer's "visual snapshots in both directions" are DOM snapshots in Vitest (structure, `dir`, logical classes); real screenshots are in the opt-in Playwright run (call: pixel snapshots need the dev stack and a browser, which CI does not run).
- Each slice adds its part of the Playwright exit spec under `web/e2e/`, opt-in through `POSTIT_E2E`; the maintainer runs it (the slice's `-verification.md`).

## Backend changes D4 needs

Small, each in the slice that first needs it, each with HTTP tests and the regenerated contract.

### D4a: background account import

Picks up `2026-10-23-b3-credential-connection-deferred.md`, "Import progress for large files".

- `POST /accounts/import/{platform}?commit=…` answers `202 { import_id }` instead of the report. Both the dry run (`commit=false`) and the commit run in the background, because both call the platform once per row (call).
- An `account_import` job on the jobs ledger processes the rows with the existing streaming service; `GET /accounts/imports/{id}` (owner) answers status (`processing`, `done`, `failed`), processed, connected, and rejected counts, and the per-row report so far (row number, outcome, account IDs connected, error code; never a credential value).
- **Staged rows hold credentials, so they are sealed (call).** The upload is streamed into staging rows, each row's values sealed through the vault as connected-account credentials are, applied and deleted a chunk per transaction like `contact_import_rows`, so a crash resumes and a console retry applies no row twice. Staged rows of an import left `processing` for 24 hours with no progress are deleted by `data_retention` and the import marked `failed`. The job summary carries only the import ID.
- The existing rate limit (`credential_checks`) is charged once for the upload; the per-row platform calls stay bounded by `accounts.csv_max_rows` and the platform's own rate limits.
- The B3 deferred entry is then resolved; its other entry (the Dart client) stays open for D6, so the file keeps its name.

### D4a: connection guides in the descriptor

Picks up `2026-11-01-c3-telegram-agent-deferred.md`, "The connection guide".

- `AgentDescriptor` gains `guide: Vec<Message>` (ordered steps, default empty) and `guide_url: Option<Url>` (call: the guide is platform data, so it travels with the descriptor and a new agent needs no client change). The Accounts connect screen shows the steps above the credentials form.
- Telegram writes the steps from the C3 entry; the email agent writes steps for SMTP (where to find host, port, and an app password); the mock agents write a one-line guide for tests.

### D4b: queue rows and live history

- `PublishJobSummaryDto` gains the post's first line (cut to 80 graphemes, as the calendar does), its campaign, and the accounts in the caller's limit, and `GET /publish-jobs` gains `phase=open|finished`, so a queue row needs no request per job.
- `PostSummaryDto` gains `has_live_history` (any job that is not a dry run), because deleting such a post needs `publish` whatever its status, and an edited post is `draft` again.

### D4c: patch links a generation, and AI may be absent

- `PATCH /posts/{id}` accepts `generation_id`, so a draft saved before generating still links to its generation.
- A server built without the `ai` feature answers `404` on every `/ai` route; the client reads that from `GET /ai/presets` once and shows no AI screen, panel, switch, or setup step.

### D4d: email preview

- `POST /accounts/{id}/email-preview` (edit, account in limit) with an `EmailDocument` and the target's options answers `{ subject, html, text, violations }`: the email agent's own renderer, merge fields filled with sample values (`first_name` "Alex"), no tracking, the footer with the account's postal address. One renderer, so the preview is what recipients get (call). A document that still breaks rules but renders previews with its violations; only one the renderer refuses answers `422`. It has its own rate bucket, `rate_limit.email_previews`, so editing an email never spends the publish bucket (D4d plan; this spec first said the publish bucket).

### D4d: contacts

- `GET /contacts` gains `q`, a case-insensitive search over email and names, for the contacts table's search box.
- `GET /contacts/{id}/lists` lists the lists one contact is in, for the contact page.
- The DNS finding codes and the contact import rejection codes become typed enums in the contract, so both clients' catalogs are checked for completeness.
- The editor shows the HTML in a sandboxed `iframe` (`sandbox=""`, `srcdoc`) at 600 and 375 pixels wide.

### D4d: raw HTML block

- `EmailBlock` gains `Html { html }` (call: "raw-HTML mode" is one block holding hand-written HTML, placed among the other blocks, so a document can mix both; a document made of one `html` block is the full raw mode). It renders through `mj-raw`; its links are rewritten for click tracking like other links; the plain-text part is derived from it; the footer and unsubscribe headers are still added by the renderer, never by the user's HTML. The size check covers it. Scripts, forms, and iframes are removed when rendering.
- The editor offers raw HTML as an "Advanced" block type with a code textarea; it is not a WYSIWYG editor.

### D4d: setup state

- `user_preferences` gains `setup_completed_at` (set when the user finishes or skips the first-run setup) and `checklist_dismissed` (call: on the server, so the mobile app in D6 honors the same choice). Both go through `GET/PUT /me/preferences`.
- The first-run setup opens on a user's first visit while `setup_completed_at` is null. The dashboard checklist is derived from data (language chosen, brand profile written, an account connected, an AI provider added) and hides when everything is done or it is dismissed.

## Slice details

### D4a Workspace and accounts

- **Switcher** at the top of the sidebar: own workspace first, then received delegations with the owner's name and level; a limited grant says how many accounts.
- **Delegations** (own workspace only): grant by exact email (`GET /users/lookup`), level, all or chosen accounts, optional expiry; edit; revoke through the kit `DangerZone` pattern; a "Received" tab with leave.
- **Preferences**: the profile page gains language, time zone, and the existing notification switches.
- **Notifications**: bell with an unread badge, a popover with the newest ten, and a full list page (paginated, mark one or all read). Each kind deep-links: a delivery or job to the job detail (from D4b; in D4a these open no page and are marked read in the list), an account notice to the account, a delegation notice to Delegations, a pending user to Users. The existing `/?notification=<id>` push click resolves through the same mapping.
- **Startup error**: `StartupError.tsx` strings move to the catalog.
- **Accounts list**: grouped by platform with icon and color, status (active, needs reconnect, revoked), the test flag, and the unfinished-connection rows from `GET /accounts/pending-connections` ("Unfinished connection to {platform}, expires in {n} min" with Cancel and Start again, never Confirm).
- **Connect**: a platform picker from `GET /platforms`. Credential platforms show the guide and the `SchemaForm`; submitting returns `{ connect_id, discovered[] }` and opens the discovery checklist. OAuth platforms call `POST /accounts/connect/{platform}` with `return_to` set to `/oauth-done` and send the browser to `authorize_url`. `/oauth-done` reads `result` and `connect_id`, shows the checklist (or the translated result code, such as `account_connected_elsewhere`), confirms, and returns to Accounts.
- **Discovery checklist**: every discovered account with a checkbox (all checked), Confirm and Cancel. `connect_id` and `pending_id` are kept in `sessionStorage` under one key with their expiry, removed on confirm, cancel, or expiry.
- **Account detail**: facts, rename, test flag, check now, writing style form, disconnect in a danger zone (the server explains what happens to scheduled deliveries).
- **Target groups**: a list and an editor with an account picker limited to the caller's limit.
- **CSV import**: pick a file, dry run with the per-row report, then commit, both with progress from the background import. Commit sends the same file again from the page's memory, so the user does not re-pick it; a dry run's staged rows are not reused (call: one import is one upload, and nothing sealed lingers waiting for a commit that may never come).
- **Admin**: a Platforms page (configured, compiled in, enabled switch, redirect URI with a copy button); a Publishing page (live switch with the native-scheduled count and the warning that those cannot be stopped); per-user limits as a dialog from the Users page.

### D4b Compose and publish

- **Media library**: a thumbnail grid through view URLs, upload by picker or drop, storage usage from `GET /media/usage`, delete refused with `media_in_use` explained. **Resumable upload:** `POST /media/uploads` starts a session; chunks of `chunk_max` are `PUT` with the offset; on a network error the client retries with backoff after `HEAD` reports the stored offset; the session ID and file name, size, and last-modified time are kept in `sessionStorage`, so after a reload the user re-selects the same file and the upload continues where it stopped (a different file starts over). Several uploads run one at a time per tab (call).
- **Brand profile**: one form (description, audience, products and services, words to avoid, default language, up to five examples).
- **Composer**, controlled mode. A post editor (text with a live counter per target from its `TextMeasure` and limit, link, tags, media from the library with alt text, AI disclosure flag), a target picker (accounts and target groups; groups expand to accounts on the client), and a tab per target for its override (text, link, tags, media subset) and its publish options (`GET /accounts/{id}/publish-options` through `SchemaForm`, showing `consent_text`). An email target's tab shows the email document editor once D4d builds it; until then it shows that email content is not available yet (call). Validation runs on demand and before the publish dialog opens, debounced while typing (call: 800 ms after the last change, only once the user has asked for validation once). Campaign selection is a field of the post. Drafts save with `POST /posts` and then `PATCH` with `If-Match`.
- **Publish dialog**: now or scheduled (date and time in the user's zone; per target, whether it uses native scheduling, with the native-schedule toggle when the platform offers it); dry-run toggle; the post version it opened on, sent as `If-Match`, a conflict reloads the dialog; for a live run, a per-target summary with each `consent_text`, a warning when `last_published` shows the same version already went to a target, and an explicit confirm checkbox before the button enables.
- **Queue and history**: two views of `GET /publish-jobs` (queue: scheduled and running; history: finished), filtered by status, campaign, dry run, and date range, paginated through `useListParams`. A job detail page lists deliveries (state, parking reason, remote link, last error code, created by, direct-send progress) with cancel per job or delivery, reschedule, retry (with the `OutcomeUnknown` acknowledgment where required), and resolve.
- **Campaigns**: list with archived filter, create and edit (name, dates, color), archive, delete (unlinks posts, as the dialog says).

### D4c AI

- **AI providers** (owner): presets from `GET /ai/presets` with each guide link and the subscription note, local presets shown only when `available`, a custom provider form (opencode-shaped fields, paste one entry), key entry as a secret field, Test, the model list (with the non-text marking from D2), choose a default, delete. Delegates with `edit` see provider names and models only.
- **Generation history**: `GET /ai/generations` in a `DataTable` (time, actor, mode, model, outcome, tokens).
- **AI panel** in the composer: prompt, chosen targets, language, attached media, provider and model; Generate fills each target's override with its variant, marks the post's AI disclosure, and keeps `generation_id` for `POST /posts`. `ai_provider_required` shows "Connect an AI provider" with a link, never a hidden button. Repair notes (`ai.repair_failed`) and what is still wrong show through `ViolationList`.
- **Random mode**: the composer's mode switch; in random mode the user writes only the prompt and picks media and targets, and the panel's result becomes the editable draft. Both modes end in the same draft.
- **Preferences**: the `store_ai_prompts` switch.
- The D2 live provider checks in `2026-11-27-consolidated-verification.md` are triggered here; the plan reminds the maintainer.

### D4d Email and onboarding

- **Contacts**: contacts table (search through `q`, status, tags, list filter), contact editor (fields, tags, locale, custom fields), lists with membership, suppressions with remove for `bounced` only.
- **Contact import** (owner): pick a file, map columns (the first rows previewed in the browser), choose the consent column, check the attestation, start, and watch `GET /contacts/imports/{id}`; export as CSV.
- **Email account setup**: on the email account's detail page, the DNS check (`GET /accounts/{id}/dns-check` with an optional DKIM selector) with findings translated from a new `dns_findings` catalog and its completeness test (picks up the open half of `2026-10-30-c1-c2-contacts-email-agent-deferred.md`, "Client catalog entries"); the entry is then resolved for the web and stays open for D6.
- **Email block editor** in the composer's email target tab: subject, preheader, language, and blocks (heading, text, image from the library, button, divider, raw HTML) added, edited, reordered, and removed; merge-field insertion (`{{first_name|there}}`); the live preview at desktop and mobile widths through the preview route; "Send test email" to the user's verified address. An unfinished image or button block stays in the editor, out of the stored document, and blocks saving until it is finished, because the wire type requires its image and its URL.
- **Audience picker** for every direct target: the contact lists (at least one) and an optional tag filter, written to the target's `options.lists` and `options.tags`. They are read by publishing, not declared in the agent's options schema (whose subset has no arrays), so `SchemaForm` cannot render them; the agent's own options (open tracking) stay the schema form.
- **First-run setup**: language, brand profile, connect a first account, add an AI key; each step has Skip; the dashboard checklist shows what is left.
- **Exit**: the full Playwright spec of plan 03 (set up, connect, compose, schedule, see it published; send a campaign to Papercut; then `member@postit.com` does the same as a `publish` delegate) and the Papercut campaign entry in the consolidated verification file.

## Deferred and verification items

Picked up by D4:

- B3, "Import progress for large files": D4a (above).
- C3, "The connection guide": D4a, web half; the entry stays open for D6.
- C1/C2, "Client catalog entries … DNS finding codes": D4d, web half; stays open for D6.
- C3, "Telegram counts characters, not UTF-16 units": triggers "phase D4 if a user hits the early stop". The composer counts with the format's `TextMeasure`, so the D4a plan checks whether the counter makes the early stop visible and probes it then if so (call: no change unless it shows).

Human checks D4 triggers (reminders, not agent work): the D2 provider checks (D4c) and the Papercut campaign in its UI (D4d) in `2026-11-27-consolidated-verification.md`; each slice's Playwright run.

## Left to the builder

- Component names and file layout under `web/src/features/` (one folder per area, following `jobs/`), hook names, and query key shapes, within the workspace-prefix rule.
- Which kit components each slice adds (expected: `SchemaForm`, `ViolationList`, `PlatformBadge`, `AccountPicker`, `UploadProgress`, `NotificationBell`, `ScheduleInput`), as long as pages do not draw their own.
- The DTO names and table names of the backend changes, following the existing `dto_*.rs` files and `contact_import_rows`.
- Whether the composer's override tabs are tabs or an accordion on narrow screens.
- Updating `CLAUDE.md` "Project state", the roadmap's D4 section with an "As built (D4)" note per slice, and `docs/configuration.md` when config changes.
