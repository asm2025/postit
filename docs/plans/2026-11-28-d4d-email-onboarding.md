# D4d Email and onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The last D4 slice: a web user manages contacts, lists, and suppressions, imports contacts from CSV with column mapping and consent, exports them, checks their sending domain, writes an email campaign in a block editor with raw HTML and a live preview, sends a test, and sends the campaign to a list; a new user is walked through first-run setup and sees what is left on the dashboard. The D4 exit criteria hold.

**Architecture:** Four small backend changes: a preview route rendered by the email agent's own renderer (a new `DirectSender::preview` method, so the preview is what recipients get); a raw-HTML email block, sanitized with `ammonia` and rendered through `mj-raw`; a typed DNS finding code so the clients' catalogs can be checked for completeness; and two setup fields on the preferences. Then web screens: a contacts area, an email document editor that plugs into the D4b composer's `TargetTab.directContent` slot, the DNS check on the email account's page, and a setup wizard with a derived checklist.

**Tech Stack:** Rust (axum, sqlx, `mrml`, `ammonia`, `html2text`), React 19, React Router 8, TanStack Query 5, `openapi-fetch`, i18next, Radix/shadcn, Tailwind 4, Vitest + MSW, Playwright.

**Spec:** `docs/specs/2026-11-27-d4-web-publishing-ui-design.md` (sections "Shared decisions", "Backend changes D4 needs: D4d", "Slice details: D4d Email and onboarding"). Also read plan 03 Phases C1, C2, and D4, "Sending an email campaign", and the REST surface in `docs/contracts/03. publishing roadmap.md`; the C1/C2 plan `docs/plans/2026-10-30-c1-c2-contacts-email-agent.md`; and the as-built D4b and D4c plans (`docs/plans/2026-11-28-d4b-compose-publish.md`, `docs/plans/2026-11-28-d4c-ai.md`). **Before Task 7, check the D4b names this plan consumes (`TargetTab`'s `directContent`, `ComposerState.overrides`, `ComposerAction.setOverride`, `MediaPicker`, `ViolationList`) against the merged code** and use the built names where an "As built" note renamed one.

## Global Constraints

- US English everywhere (code, catalogs, docs, commits).
- Web: pnpm only, run from PowerShell. Gate: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build` in `web/`.
- Pages use `web/src/components/kit/` and `web/src/components/ui/`; `kitGuard.test.ts` stays green; new kit components are tested in both directions.
- Every user-visible string is in an `en-US` catalog; a new namespace is registered in `web/src/i18n/index.ts` and `i18next.d.ts`. Layouts use logical properties. The email document's own direction follows its `language`, not the app's.
- No platform ID literal in features or components outside tests. The email editor is chosen by the descriptor (`kind: 'direct'` with a format `html_email`), never by `platform === …`.
- Workspace keys through `wsKey`; access through `useAccess()`. Per the REST surface: contacts, lists, and list membership need `edit` to change and `view` to read; contact import and export, suppression removal, the test send, and the DNS check are owner-only; the preview needs `edit` with the account in limit.
- **Contact data stays out of logs and storage.** Imported CSV bytes, previewed rows, and addresses are held in component state only; nothing contact-related goes to `localStorage`, `sessionStorage`, a URL (except IDs), or an error report.
- **The preview is sandboxed:** an `iframe` with `sandbox=""` and `srcdoc`, never `allow-scripts` or `allow-same-origin`, never `dangerouslySetInnerHTML` in the app's document.
- Backend gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, targeted `cargo test -p <crate>`, `cargo xtask openapi --check` (PowerShell; also regenerates `mobile/lib/api/`, so the Dart client gets the new block, route, and fields).
- New dependencies: `ammonia` and `html2text`, both maintained with permissive licenses, as workspace dependencies in the root `backend/Cargo.toml`; nothing else new.
- Schema changes edit `backend/crates/data/migrations/0001_InitialSchema.sql` in place (single-migration policy); `cargo sqlx prepare` after new `query!` macros.
- `CARGO_TARGET_DIR=K:/cargo-target/postit`; DB test settings as in the D4b plan.
- Commit trailer: `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`.

## Review Focus

1. **Hostile or broken raw HTML:** a pasted block with `<script>`, an `onclick`, a `javascript:` link, a `<form>`, an `<iframe>`, a `<style>` with `@import`, an unclosed tag, or a 200 KB table. Scripts, handlers, forms, frames, and unsafe URLs are removed; the rest renders; the size check refuses what is over 100 KiB with `email_too_large`; the footer and unsubscribe still come from the renderer. Tests in Task 2.
2. **A preview that differs from the sent mail:** the preview and a real send of the same document go through the same `Skeleton` with the same blocks, images, and footer; only the merge values (sample), the tracking (off), and the unsubscribe link (the test token) differ. Test in Task 1 (`preview_matches_a_test_send_but_for_tracking`).
3. **A contact file that is not what it seems:** a semicolon-separated file (the server sniffs it), a UTF-8 BOM, a header row with duplicate or empty names, a formula-escaped header (`'=x`), a 50 MB file, a file with no consent column, or the attestation left unchecked. The mapping screen previews the first rows in the browser and refuses to start without the email and consent columns mapped and the attestation ticked; the server's `csv_invalid`, `csv_too_large`, `consent_attestation_required`, and `contact_limit_exceeded` show plainly. Tests in Task 5.
4. **A suppression the owner tries to undo:** only `bounced` entries offer Remove; `unsubscribed`, `complained`, and `manual` explain why they stay (the contact must opt in again, or the owner sets the contact `subscribed`); `409 suppression_not_removable` shows its message. Test in Task 4.
5. **First-run setup interrupted or done elsewhere:** the wizard reopens after a reload until finished or skipped, resumes at the first undone step, never reopens once `setup_completed_at` is set (also from the mobile app later), and a delegate never sees it for the owner's workspace. Tests in Task 9.

## Decisions taken in this plan

- **Preview through the agent (spec, shape is a call):** `DirectSender` gains `async fn preview(&self, account: &AccountRef, variant: &Variant, assets: &[MediaAsset]) -> Result<DirectPreview, AgentError>` with `pub struct DirectPreview { pub subject: String, pub html: String, pub text: String }` in core. A required method (no default), so a future direct agent must decide what its preview is. The email agent compiles the `Skeleton` and renders it for a sample recipient under `TEST_SEND_ID` (no tracking, the test unsubscribe token) with merge fields `first_name` "Alex", `last_name` "Rivera", and every other tag's fallback. The mock direct agent and the agent-kit echo client return a plain HTML page of the document's text.
- **Route:** `POST /api/v1/accounts/{id}/email-preview` `{ email: EmailDocument }` → `200 EmailPreviewDto { subject, html, text }`; `edit`, account in limit; charged to `state.limits.publish`; a document that does not compile answers `422 validation_failed` with its violations, as the test-email route does; an account without `DirectSender` answers `409 capability_unsupported`. Not stopped by the live switch (it sends nothing).
- **Raw HTML block (spec):** `EmailBlock::Html { html: String }` (`"type": "html"`). Sanitized with an `ammonia::Builder` that keeps the email-safe tags (tables, `p`, `div`, `span`, headings, lists, `a`, `img`, `br`, `hr`, `strong`, `em`, `b`, `i`, `u`, `center`, `font`) and the attributes `style`, `align`, `valign`, `width`, `height`, `bgcolor`, `border`, `cellpadding`, `cellspacing`, `colspan`, `rowspan`, `href`, `src`, `alt`, `title`, `dir`; URL schemes `http`, `https`, `mailto`, `tel`; everything else stripped, including `script`, `style`, `form`, `input`, `iframe`, `object`, `embed`, and every `on*` attribute. Its links go through the same click rewriting as other links (ammonia's `attribute_filter` swaps each `href` for the renderer's link sentinel); `img src` must be `https` (a `data:` or `http:` image is dropped, since mail clients block them). Merge tags are allowed in text, refused inside a URL (`email_merge_tag_invalid`) as elsewhere. The plain-text part is `html2text` of the sanitized HTML. It renders inside `mj-raw` in its own section.
- **Typed finding codes (call):** `DomainFinding.code` becomes `DomainFindingCode` (an enum with the eleven snake_case values the email agent already returns), so `backend/openapi.json` lists them and both clients' catalogs can be checked like violations. The wire is unchanged.
- **Setup state (spec):** `user_preferences.setup_completed_at TIMESTAMPTZ` and `checklist_dismissed BOOLEAN NOT NULL DEFAULT FALSE`; `PreferencesDto` gains `setup_completed_at: Option<DateTime<Utc>>` and `checklist_dismissed: bool`; `UpdatePreferences` gains `setup_completed: Option<bool>` (`true` sets `now()` when unset and is idempotent; `false` is `422 validation_failed`, since setup is never reopened) and `checklist_dismissed: Option<bool>`. Self preferences, so they follow the signed-in user, not the workspace.
- **Routes (call):** `/contacts`, `/contacts/:id`, `/contacts/lists`, `/contacts/suppressions`, `/contacts/import`, `/setup`. The DNS check sits on the email account's detail page (`/accounts/:id`), shown when the descriptor has the `sender_domain_check` capability.
- **Email editor placement:** the D4b `TargetTab` for a direct account renders `EmailEditor` through `directContent`; the document lives in `overrides[account].patch.email`, and the target's direct-send options (lists, tag filter, open tracking) stay the agent's options schema through `SchemaForm`, as every other option.
- **Block reordering (call):** "Move up" and "Move down" buttons on each block (keyboard reachable, no drag library).

## Deferred and verification items

- Picked up: `2026-10-30-c1-c2-contacts-email-agent-deferred.md`, "Client catalog entries … DNS finding codes", web half (Tasks 3 and 6). The entry is then **resolved for the web** and stays open for D6 (mobile), so the file keeps its name; its other entries (click bucket, NUL characters, link index) have G1 and E3 triggers and stay as they are.
- No other open `-deferred.md` entry has a D4d trigger. (The B3, B4, and B6 Dart entries and the C3 guide's mobile half are D6.)
- Human checks triggered here: "A campaign through Papercut, checked in its UI" in `docs/plans/2026-11-27-consolidated-verification.md` ("repeat once in D4 when the web composer can send a campaign"). Remind the maintainer at the start and in the final report; when they confirm it, mark it resolved with the date and result. Also remind them of the still-open D4a, D4b, and D4c verification files. The full D4 Playwright exit spec is this slice's verification item (Task 11).

---

## File Structure

Backend:
- `backend/crates/core/src/agent/capability.rs` — `DirectPreview`, `DirectSender::preview`, `DomainFindingCode`.
- `backend/crates/core/src/email.rs` — `EmailBlock::Html`.
- `backend/crates/agents/email/src/render/` — `mod.rs`, `mjml.rs`, `text.rs`, new `html.rs` (sanitizer and link sentinels); `client.rs` (`preview`), `dns.rs` (typed codes).
- `backend/crates/agents/mock/src/capabilities/direct.rs`, `backend/crates/agent-kit/tests/support/echo.rs` — `preview`.
- `backend/crates/api/src/routes/accounts.rs`, `dto_accounts.rs`, `routes/mod.rs`, `openapi.rs` — the preview route.
- `backend/crates/data/migrations/0001_InitialSchema.sql`, `backend/crates/data/src/preferences.rs`, `backend/crates/api/src/dto.rs`, `backend/crates/api/src/routes/me.rs` — setup fields.

Web:
- `web/src/features/contacts/` — `queries.ts`, `ContactsPage.tsx`, `ContactDialog.tsx`, `ContactPage.tsx`, `ListsPage.tsx`, `ListMembers.tsx`, `SuppressionsPage.tsx`, `ContactsNav.tsx`, `import/ImportContactsPage.tsx`, `import/csvPreview.ts`, `import/ColumnMapper.tsx`, `ExportButton.tsx`.
- `web/src/features/email/` — `EmailEditor.tsx`, `BlockEditor.tsx`, `blocks.ts`, `MergeFieldMenu.tsx`, `EmailPreview.tsx`, `TestSendButton.tsx`, `queries.ts`.
- `web/src/features/accounts/DnsCheck.tsx`.
- `web/src/features/setup/` — `SetupWizard.tsx`, `checklist.ts`, `SetupChecklist.tsx`.
- `web/src/i18n/en-US/{contacts,email,dns,setup}.json`.
- `web/e2e/d4d.spec.ts`, `web/e2e/d4.spec.ts`.

---

### Task 1: Email preview through the agent's renderer

**Files:**
- Modify: `backend/crates/core/src/agent/capability.rs`, `backend/crates/agents/email/src/client.rs`, `backend/crates/agents/mock/src/capabilities/direct.rs`, `backend/crates/agent-kit/tests/support/echo.rs`
- Modify: `backend/crates/api/src/routes/accounts.rs` (reuse `test_email_assets`), `dto_accounts.rs`, `routes/mod.rs`, `openapi.rs`
- Test: `backend/crates/agents/email/tests/render.rs` (or the crate's existing render test file), `backend/crates/api/tests/accounts.rs`

**Interfaces:**
- Produces: `DirectPreview`, `DirectSender::preview` (Decisions); `EmailPreviewRequest { email: EmailDocument }` (`additionalProperties: false`), `EmailPreviewDto { subject: String, html: String, text: String }`; `POST /api/v1/accounts/{id}/email-preview`, `operation_id = "preview_email"`, documented with 403 (`capability_required`, `account_not_in_scope`, `delegation_invalid`), 404, 409 (`capability_unsupported`), 422, 429, 500, 503.
- [ ] **Step 1: Write failing tests:** email agent `preview_fills_sample_merge_fields` ("Hi {{first_name|there}}" → "Hi Alex"), `preview_has_footer_with_postal_address`, `preview_has_no_tracking_or_pixel`, `preview_matches_a_test_send_but_for_tracking` (same HTML as a test send's rendered body, with the recipient's merge values swapped); api `preview_answers_html_and_text`, `preview_needs_edit_and_account_in_limit` (view 403, limited delegate outside the limit 404), `preview_of_bad_document_is_422_with_violations`, `preview_on_broadcast_account_is_409`, `preview_works_with_live_publishing_off`, `preview_charges_the_publish_bucket`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-email -p postit-api --test accounts` — fail.
- [ ] **Step 3: Implement**; `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests, `cargo test -p postit-agent-mock -p postit-agent-kit`, and `cargo xtask openapi --check` — pass.
- [ ] **Step 5: Commit** `feat: preview an email through the agent's renderer`.

### Task 2: Raw HTML email block

**Files:**
- Modify: `backend/Cargo.toml` (`ammonia`, `html2text` workspace dependencies), `backend/crates/agents/email/Cargo.toml`, `backend/crates/core/src/email.rs`, `backend/crates/agents/email/src/render/{mod,mjml,text}.rs`; Create: `backend/crates/agents/email/src/render/html.rs`
- Modify: every exhaustive `match` on `EmailBlock` (`rg "EmailBlock::" backend/crates`), including the media-reference walk (`variant.media_ids()`: an `Html` block references no asset)
- Test: `backend/crates/agents/email/tests/` (new `html_block.rs`), `backend/crates/core/tests/wire_format.rs`

**Interfaces:**
- Produces: `EmailBlock::Html { html: String }`; `render::html::sanitize(input: &str, links: &mut LinkSentinels) -> Sanitized { html: String, text: String }` (the sanitized markup with link sentinels in place of each `href`, and its `html2text` plain text at the renderer's text width).
- [ ] **Step 1: Write failing tests:** `wire_format::html_block_round_trips` (`{"type":"html","html":"<p>x</p>"}`); `html_block_strips_scripts_handlers_forms_and_frames`; `html_block_drops_javascript_and_data_urls`; `html_block_keeps_tables_and_inline_styles`; `html_block_links_are_click_tracked` (a campaign send rewrites its `href` to `/public/t/c/…`; a test send does not); `html_block_merge_tag_in_url_is_refused`; `html_block_plain_text_is_derived`; `html_block_counts_toward_the_size_limit` (a 120 KB block gives `email_too_large`); `footer_and_unsubscribe_still_added_after_html_block`; `unclosed_tags_are_balanced`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-email --test html_block` and `cargo test -p postit-core --test wire_format` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests, `cargo test -p postit-agent-email`, `cargo xtask openapi` then `--check`, and `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass. The web's `schema.d.ts` now has `EmailHtmlBlock`; the mobile client regenerates.
- [ ] **Step 5: Commit** `feat: a raw HTML email block, sanitized and tracked`.

### Task 3: Typed DNS finding codes and setup preferences

**Files:**
- Modify: `backend/crates/core/src/agent/capability.rs` (`DomainFindingCode`, `DomainFinding.code`), `backend/crates/agents/email/src/dns.rs`, `backend/crates/agents/email/tests/dns.rs`
- Modify: `backend/crates/data/migrations/0001_InitialSchema.sql` (`user_preferences`), `backend/crates/data/src/preferences.rs`, `backend/crates/api/src/dto.rs` (`PreferencesDto`, `UpdatePreferences`) and the preferences route in `backend/crates/api/src/routes/me.rs`
- Test: `backend/crates/core/tests/wire_format.rs`, `backend/crates/data/tests/preferences.rs`, `backend/crates/api/tests/preferences.rs`

**Interfaces:**
- Produces:
  - `#[serde(rename_all = "snake_case")] pub enum DomainFindingCode { SpfOk, SpfMissing, SpfWeak, SpfUnknown, DkimOk, DkimMissing, DkimUnknown, DmarcOk, DmarcMissing, DmarcWeak, DmarcUnknown }` with `utoipa::ToSchema` under the `openapi` feature; `DomainFinding.code: DomainFindingCode`.
  - The setup fields per Decisions, with `PreferencesRepo` reading and writing them.
- [ ] **Step 1: Write failing tests:** `wire_format::finding_codes_are_snake_case` (every variant); data `setup_completed_is_set_once` (a second `true` keeps the first time), `checklist_dismissed_round_trips`; api `preferences_show_setup_fields`, `setup_completed_false_is_422`, `setup_fields_refuse_act_as` (the route is self-only already; assert the 403 stays).
- [ ] **Step 2: Run** `cargo test -p postit-core --test wire_format`, `cargo test -p postit-data --test preferences -- --test-threads=4`, `cargo test -p postit-api --test preferences` — fail.
- [ ] **Step 3: Implement**; `cargo sqlx prepare`; `cargo xtask openapi` (PowerShell).
- [ ] **Step 4: Run** the tests, `cargo test -p postit-agent-email --test dns`, and `cargo xtask openapi --check` — pass.
- [ ] **Step 5: Commit** `feat: typed DNS finding codes and setup state in preferences`.

### Task 4: Contacts, lists, and suppressions

**Files:**
- Create: `web/src/features/contacts/queries.ts`, `ContactsPage.tsx`, `ContactDialog.tsx`, `ContactPage.tsx`, `ListsPage.tsx`, `ListMembers.tsx`, `SuppressionsPage.tsx`, `ContactsNav.tsx`, `web/src/i18n/en-US/contacts.json` (registered)
- Modify: `web/src/routes.tsx` (`/contacts`, `/contacts/lists`, `/contacts/suppressions` with `keepOnSwitch`; `/contacts/:id`), `web/src/components/Shell.tsx` (nav "Contacts" after Campaigns)
- Test: `web/src/features/contacts/*.test.tsx`

**Interfaces:**
- Produces:
  - `useContacts({ list, tag, status, page, pageSize })`, `useContact(id)`, `useSaveContact()` (`POST /contacts` or `PATCH /contacts/{id}`), `useDeleteContact()`, `useContactLists(page, pageSize)`, `useSaveList()`, `useDeleteList()`, `useListMembership()` (`PUT`/`DELETE /contact-lists/{id}/members/{contact_id}`), `useSuppressions(page, pageSize)`, `useRemoveSuppression()`; all keys `ws('contacts', …)`.
  - `ContactsNav`: tabs Contacts · Lists · Suppressions (and Import, owner) as links, shared by the three pages.
  - Contacts page: `DataTable` with server paging (email, name, status `contacts:status.<status>` for every `ContactStatusDto`, tags, updated), filters in the URL (list, tag, status; there is no free-text search route, so the page says "Filter by list, tag, or status"); "Add contact" (`edit`); "Export CSV" and "Import" (owner).
  - `ContactDialog`: email (create only; it never changes), first and last name, locale, tags (tag input), custom fields (key and value rows), consent source (required on create) and consent date; status (`edit`, existing contacts): setting `unsubscribed` explains "postit will not email this address again until you set it back to subscribed"; `409 status_not_settable` and `suppressed` show their messages; `403 contact_limit_exceeded` shows the cap.
  - Contact page: `FactList`, the lists it is in with add and remove (`edit`), delete in a `DangerZone`.
  - Lists page: `DataTable` (name, member count, updated), create, rename, delete with `ConfirmDialog` ("Contacts stay; they just leave the list."); a row opens `ListMembers` (the contacts page filtered by `list`).
  - Suppressions page: `DataTable` (address while a contact exists, else "Address not kept", reason `contacts:suppressionReason.<reason>` for every `SuppressionReasonDto`, date); Remove only for `bounced` and only for the owner, behind `ConfirmDialog`; other reasons show why they stay (Review Focus 4).
- [ ] **Step 1: Write failing tests:** `contacts list filters live in the URL`, `create needs consent source`, `email is read-only when editing`, `setting unsubscribed explains`, `contact_limit_exceeded shows the cap`, `list membership add and remove`, `delete list explains contacts stay`, `only bounced suppressions offer remove, and only to the owner`, `suppression_not_removable shows its message`, `every status and reason has a label`, `view level is read-only`, `delegate sees no import or export`, `rtl render`.
- [ ] **Step 2: Run** `pnpm exec vitest run src/features/contacts` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): contacts, lists, and suppressions`.

### Task 5: Contact CSV import and export

**Files:**
- Create: `web/src/features/contacts/import/ImportContactsPage.tsx`, `csvPreview.ts`, `ColumnMapper.tsx`, `web/src/features/contacts/ExportButton.tsx`
- Modify: `web/src/routes.tsx` (`/contacts/import` under `RequireOwn`)
- Test: `web/src/features/contacts/import/*.test.tsx`, `csvPreview.test.ts`

**Interfaces:**
- Consumes: `POST /contacts/import` (multipart `ContactImportForm`: `options` JSON `ImportOptions` and `file`), `GET /contacts/imports/{id}`, `GET /contacts/export?list_id=`, `ProgressBar`, `DataTable`.
- Produces:
  - `previewCsv(file: File, rows = 5): Promise<{ headers: string[]; rows: string[][]; delimiter: ',' | ';' | '\t' }>`: reads the first 64 KB only (`file.slice`), strips a UTF-8 BOM, detects the delimiter from the header line, and parses quoted cells (RFC 4180); used only for the mapping screen, never sent. Duplicate or empty header names are reported as `{ problem: 'duplicate_header' | 'empty_header', name }` so the screen explains before upload. The server still decides: it sniffs `,` or `;` from the header the same way (`sniff_delimiter` in `backend/crates/contacts/src/import.rs`: `;` when the header has more semicolons than commas outside quotes), so the preview uses that rule and a tab is not offered.
  - `ColumnMapper({ headers, value: ColumnMap, onChange })`: one `OptionSelect` per contact field (email and consent source required; consent date, first and last name, locale, tags optional) over the file's headers, plus "Custom field" rows mapping a header to a merge field name. Guesses defaults from header names (`email`, `e-mail` → email; `first name`, `first_name` → first name; …), case-insensitively.
  - Import page: pick a file → preview table (5 rows) and mapping → target list (optional `CampaignSelect`-like `OptionSelect` over lists, or "No list") → the attestation checkbox with the C1 wording ("Everyone in this file agreed to get email from me, and the consent column says where.") → Start. Then `useContactImport(id)` polls every 2 s while `processing`; shows `rows_ok`, `rows_rejected`, the report entries (row and `contacts:importRejection.<code>`), and the totals. The import ID is in the URL (`?import=<id>`). Errors: `consent_attestation_required`, `csv_invalid` (with the server's detail), `csv_too_large`, `contact_limit_exceeded`, each translated.
  - `ExportButton({ listId? })`: fetches `GET /contacts/export` through the API client (the route needs the bearer), saves it as `contacts-YYYY-MM-DD.csv` through an object URL that is revoked right after; a hint says formula-like cells are escaped with a leading `'`.
- [ ] **Step 1: Write failing tests:** `preview reads only the head of a big file`, `preview strips a BOM and picks the delimiter as the server does`, `duplicate and empty headers are reported`, `mapper guesses email and names`, `start is disabled until email and consent are mapped and the attestation is ticked`, `import polls and shows the report`, `every rejection code in the C1 importer has a label` (`row_malformed`, `email_invalid`, `consent_missing`, `consent_at_invalid`, `duplicate_in_file`, `contact_limit_exceeded`, from `backend/crates/contacts/src/import.rs`, hard-coded with a pointer there), `server errors are translated`, `export downloads and revokes the URL`, `owner only`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): contact CSV import with column mapping, and export`.

### Task 6: The DNS check on the email account

Picks up `docs/plans/2026-10-30-c1-c2-contacts-email-agent-deferred.md`, "Client catalog entries … DNS finding codes" (web half).

**Files:**
- Create: `web/src/features/accounts/DnsCheck.tsx`, `web/src/i18n/en-US/dns.json` (registered), `web/src/i18n/dns.test.ts`
- Modify: `web/src/features/accounts/AccountDetailPage.tsx` (the section when the descriptor has `sender_domain_check`, owner only)
- Test: `web/src/features/accounts/DnsCheck.test.tsx`

**Interfaces:**
- Produces:
  - `dns.json`: per code a `title` and `guidance` (what the record does and what to add, in plain words): `spf_ok`, `spf_missing`, `spf_weak`, `spf_unknown`, `dkim_ok`, `dkim_missing`, `dkim_unknown_no_selector` and `dkim_unknown_failed` (the client picks by whether it sent a selector, as the deferred entry asks), `dmarc_ok`, `dmarc_missing`, `dmarc_weak`, `dmarc_unknown`.
  - `dns.test.ts`: every `DomainFindingCode` from `backend/openapi.json` has an entry (`dkim_unknown` through both variants) and no entry is unused.
  - `DnsCheck({ accountId })`: an optional DKIM selector input (one DNS label; a `422` marks it), "Check my domain" → `GET /accounts/{id}/dns-check?dkim_selector=`; three rows (SPF, DKIM, DMARC) with a status badge (`ok` success, `weak` warning, `missing` danger, `unknown` neutral) and the guidance; `429 rate_limited` shows the wait.
- [ ] **Step 1: Write failing tests:** catalog completeness; `shows three findings with guidance`, `dkim unknown wording depends on the selector`, `bad selector marks the field`, `only the owner sees the check`, `hidden for accounts without the capability`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass. In the C1/C2 deferred file, mark the DNS half **resolved for the web in D4d (Task 6)**; the entry stays open for D6 (mobile), so the file keeps its name.
- [ ] **Step 5: Commit** `feat(web): check the sending domain's DNS records`.

### Task 7: Email document editor

**Files:**
- Create: `web/src/features/email/EmailEditor.tsx`, `BlockEditor.tsx`, `blocks.ts`, `MergeFieldMenu.tsx`, `web/src/i18n/en-US/email.json` (registered)
- Modify: `web/src/features/compose/TargetTab.tsx` call site in `ComposerPage.tsx` (pass `directContent={(account) => <EmailEditor account={account} />}` when the account's descriptor declares the `html_email` format; other direct agents keep D4b's placeholder)
- Test: `web/src/features/email/EmailEditor.test.tsx`, `blocks.test.ts`

**Interfaces:**
- Consumes: `useComposer` (`overrides[account].patch.email`, `setOverride`), `MediaPicker` (images only), `ViolationList`.
- Produces:
  - `blocks.ts`: `newBlock(type: EmailBlock['type']): EmailBlock` (heading `h1`, empty text, image without asset, button with empty label and URL, divider, html empty); `moveBlock(blocks, from, to)`; `emptyDocument(language: EmailLanguage): EmailDocument` (`subject: ''`, `preheader: null`, `blocks: [heading, text]`); `insertMergeTag(text, cursor, field, fallback?)` → `{{field|fallback}}` or `{{field}}`.
  - `EmailEditor({ account })`: subject (required mark), preheader, language (`OptionSelect` over `EmailLanguage`; it sets the preview's direction), the block list, "Add block" (heading, text, image, button, divider, and under "Advanced", raw HTML), and the preview (Task 8) beside it on wide screens, below on narrow ones. A post without a document starts from `emptyDocument`.
  - `BlockEditor({ block, onChange, onMove, onRemove, first, last })`: heading (text and level), text (`Textarea` with `MergeFieldMenu`), image (`MediaPicker`, alt required, optional link), button (label and URL), divider, raw HTML (a monospace `Textarea` with "Scripts, forms, and frames are removed when the email is sent." and a link to the preview). Move up and down and remove on each block.
  - `MergeFieldMenu({ onInsert })`: `first_name`, `last_name`, `email`, and custom field names seen in the workspace's contacts (from the first page of `useContacts`), with an optional fallback ("there").
- [ ] **Step 1: Write failing tests:** `new email starts with a heading and a text block`, `add, edit, move, and remove blocks`, `image block needs an asset and alt text`, `merge field inserts with a fallback`, `raw HTML block is under Advanced and warns`, `document is stored in the target's override`, `direct agents without html_email keep the placeholder`, `email violations from validation point at the block` (`path` `email.blocks[2].url` highlights block 3), `rtl app with an en-US document keeps the document ltr`, `view level is read-only`.
- [ ] **Step 2: Run** `pnpm exec vitest run src/features/email` — fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): email block editor in the composer`.

### Task 8: Live preview and test send

**Files:**
- Create: `web/src/features/email/EmailPreview.tsx`, `TestSendButton.tsx`, `queries.ts`
- Modify: `EmailEditor.tsx`
- Test: `web/src/features/email/EmailPreview.test.tsx`, `TestSendButton.test.tsx`

**Interfaces:**
- Consumes: Task 1's route, `POST /accounts/{id}/test-email`.
- Produces:
  - `usePreview(account, doc)`: `POST /accounts/{id}/email-preview`, 600 ms after the last change, dropping answers for older documents; key `ws('email', 'preview', account, hash(doc))` with `staleTime: 60_000`.
  - `EmailPreview({ account, doc })`: a width switch "Desktop" (600 px) and "Phone" (375 px), the subject and preheader as an inbox line, and an `iframe` with `sandbox=""`, `srcdoc={html}`, `title={t('email:preview.title')}`, `referrerPolicy="no-referrer"`; a "Plain text" tab with the `text` in a `pre`. A `422` shows its violations through `ViolationList`; the previous preview stays dimmed meanwhile.
  - `TestSendButton({ account, doc })` (owner only): sends the current document (saved or not) to the signed-in user's verified address; `204` → "Sent to your address. Check your inbox (in development, Papercut)."; `409 email_address_missing` → "Verify your email address in your profile first."; `409 live_publish_disabled` → "Live sending is off for this deployment."; `409 test_send_refused` → the message with its reason code; `422` → violations; `429` → the wait.
- [ ] **Step 1: Write failing tests:** `preview renders in a sandboxed iframe with srcdoc`, `the iframe never gets allow-scripts`, `width switch changes the frame width`, `preview waits for typing to stop and drops stale answers`, `plain text tab`, `preview violations show`, `test send success and each 409`, `test send is owner only`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): live email preview and test send`.

### Task 9: First-run setup and the dashboard checklist

**Files:**
- Create: `web/src/features/setup/SetupWizard.tsx`, `checklist.ts`, `SetupChecklist.tsx`, `web/src/i18n/en-US/setup.json` (registered)
- Modify: `web/src/routes.tsx` (`/setup`), `web/src/workspace/WorkspaceProvider.tsx` or `Shell.tsx` (redirect to `/setup` once per session when in the own workspace and `setup_completed_at` is null), `web/src/features/home/HomePage.tsx` (the checklist)
- Test: `web/src/features/setup/*.test.tsx`, `checklist.test.ts`

**Interfaces:**
- Consumes: `usePreferences`, `useUpdatePreferences` (with Task 3's fields), `useBrandProfile` (D4b), `useAccounts` (D4a), `useAiProviders` (D4c), the D4a connect flow, the D4c add-provider page.
- Produces:
  - `checklistItems(ctx: { prefs: PreferencesDto; brand: BrandProfileDto; accounts: number; providers: number }): { id: 'language' | 'brand' | 'account' | 'ai'; done: boolean; to: string }[]`: `language` done when the user saved a locale or a zone differing from the defaults, or finished setup; `brand` when the profile has an `id`; `account` when at least one non-disconnected account exists; `ai` when at least one provider exists.
  - `SetupWizard`: four steps (language and time zone; brand profile; connect a first account, opening the D4a connect page with a return to `/setup`; add an AI key, opening the D4c add page with a return to `/setup`), each with Skip and Next, resuming at the first step not done; Finish or "Skip setup" sends `setup_completed: true` and goes home. The language step shows only the zone while `MULTILINGUAL` is off.
  - The redirect fires only in the own workspace, only once per tab session, and never from `/oauth-done`, `/setup`, or the connect pages.
  - `SetupChecklist` on Home: the undone items with links, "Dismiss" (`checklist_dismissed: true`); hidden when all are done or dismissed; never shown in a delegated workspace.
- [ ] **Step 1: Write failing tests:** `checklist derives each item from data`, `wizard opens on first visit and resumes at the first undone step`, `skip setup never reopens it`, `a reload mid-setup returns to the wizard`, `connect step returns to setup`, `delegated workspace never redirects or shows the checklist`, `dismiss hides the checklist`, `all done hides the checklist`, `rtl render`.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `feat(web): first-run setup and the dashboard checklist`.

### Task 10: Access and navigation sweep

**Files:**
- Modify: `web/src/components/Shell.tsx`, `Shell.test.tsx`, `web/src/features/compose/access.test.tsx`
- Test: same

**Interfaces:**
- Nav, final for D4: Home; Compose; Queue; History; Media; Campaigns; Contacts; Accounts; SETTINGS: Profile and preferences, Brand profile, AI providers, Delegations; ADMIN: Users, Platforms, Publishing, Audit, Jobs.
- Extend the access table with every contacts page, the import, export, the DNS check, the email editor, preview, test send, and setup, at owner, `view`, `edit`, `publish`, and a limited delegate.
- [ ] **Step 1: Write the failing rows.** **Step 2: Run** — fail. **Step 3: Fix.** **Step 4: Run** — pass.
- [ ] **Step 5: Commit** `test(web): email and contacts access at every delegation level`.

### Task 11: The D4 exit spec, live check, docs, and wrap-up

**Files:**
- Create: `web/e2e/d4d.spec.ts`, `web/e2e/d4.spec.ts`, `docs/plans/2026-11-28-d4d-email-onboarding-verification.md`
- Modify: `CLAUDE.md` ("Project state": D4 built; the D4 line moves from "D4b onward" to D5), `docs/contracts/03. publishing roadmap.md` ("As built (D4d)" note and an "As built (D4)" summary), `web/CLAUDE.md` (the email editor's sandboxed preview rule, one line)

**Interfaces:**
- `d4d.spec.ts` (opt-in `POSTIT_E2E`): the owner creates a list, adds three contacts (one with a first name, one without, one then set `unsubscribed`), imports a 3-row CSV with mapping and attestation, checks DNS on the email account, composes an email campaign (heading, text with `{{first_name|there}}`, image, button, raw HTML block), previews at both widths, sends a test, and publishes live to the list.
- `d4.spec.ts`, the plan 03 exit spec: a fresh user goes through setup (skipping AI), connects a mock account, composes, schedules, and sees the post published; sends a campaign to Papercut; then `member@postit.com`, as a `publish` delegate, does the same in the owner's workspace. It reuses the D4a–D4d specs' helpers from `e2e/support.ts` (move shared steps there).
- [ ] **Step 1: Write** both specs; `pnpm exec playwright test --list` parses them.
- [ ] **Step 2: Live check.** If the Playwright browser tools are available, start the Docker stack with Papercut (`./compose.ps1` per the `scripts` skill) or `cargo dev` plus Papercut, ask the maintainer to sign in, and walk `d4d.spec.ts`'s steps by hand on `https://postit.local:44315`, opening the received mail in Papercut. Fix defects here.
- [ ] **Step 3: Write** the verification file: (1) run `d4d.spec.ts` and `d4.spec.ts` — why: Zitadel passwords and Papercut's UI; trigger: before merging D4d. (2) the consolidated "A campaign through Papercut, checked in its UI" entry, now from the web composer, including the raw HTML block's link going through `/public/t/c/…` and its script removed — why: Papercut's UI and a signed-in browser; trigger: D4d. (3) anything Step 2 could not cover. Remind the maintainer of the open D4a, D4b, and D4c verification files.
- [ ] **Step 4: Run** the full web gate and the backend gates (`fmt`, `clippy`, `check`, targeted tests, `xtask openapi --check`); all pass.
- [ ] **Step 5: Commit** `chore: document D4 and add the exit spec`.

Any agent work found during execution that cannot be finished inside this plan goes to `docs/plans/2026-11-28-d4d-email-onboarding-deferred.md`; human-only checks go to the verification file.
