# C1 Contacts and C2 Email agent — design

Plan 03 phases C1 and C2, built as one plan. Source of truth: `docs/plans/03. publishing roadmap.md` (Phases C1 and C2, "Sending an email campaign", the Contacts, Accounts, and Email rows of "REST surface", "Audit events added by this plan") and `docs/plans/01. vision and architecture.md` (the `contacts` through `email_events` tables, crate levels 8 and 9, the owner-only list). This document records how the two phases fit the code as it stands after D1, and the calls made where the plans leave room. It does not repeat the plans; read those sections first.

## Intent

The maintainer picked email campaigns as the next thing to build, ahead of the drafter. C1 and C2 are also half of what D3 still waits for (C1 is a D3 prerequisite; the drafter is the other). When this plan ends, the backend can hold consented contacts, send a real campaign through a real SMTP server with exact per-recipient results, and honor unsubscribes.

**Nothing new is clickable.** The maintainer was told this and chose to stay on the roadmap: there are no web screens in C1 or C2, and the route that starts a publish arrives in D3. The first full click-through of plan 03 is D4. Campaigns here are proven by tests through `PublishingService`.

## Goals

### C1: `postit-contacts` (new crate)

Most of the data layer already exists in `postit-data` (`contacts.rs`, `publishing/direct.rs`, `email_hash.rs`). The crate adds the service on top of it: validation, the cap, import, export, and event application.

- **`ContactService`** over `ContactRepo`, `ContactListRepo`, `SuppressionRepo`, and `ContactImportRepo`, with the audit events `contacts_imported`, `contacts_exported`, `contact_list_deleted`, and `suppression_removed`.
- **The contacts cap** (`user_limits.contacts_cap`, `0` unlimited) is enforced for both `POST /contacts` and imports. The count and insert run in one transaction under a per-owner advisory lock, so two concurrent writers cannot pass it. An update of an existing address never counts.
- **Consent on every contact.** `POST /contacts` requires `consent_source`; `consent_at` defaults to now. A contact never exists without a recorded source.
- **CSV import** on the `contacts` queue (`contact_import` job).
- **CSV export**, streamed, optionally by list, with one column per custom field key. It is counted and audited before the first byte, so a dropped download is still audited. A cell starting with `=`, `+`, `-`, `@`, tab, or CR gets a leading `'`, and import removes that `'`, so the export still round-trips.
- **`email_event_apply`** job: takes a send ID and an event kind, records the `email_events` row, and suppresses on `bounced` and `complained`. C6's webhooks are its first real producer; C1 and C2 test it directly. A partial unique index keeps one row per send for every kind except `clicked` and `opened`, so a job retry or a second unsubscribe does not double-count.
- **Routes:** every Contacts and suppression row of the REST surface, with the Access column as written.

### C2: `postit-agent-email` (new crate, `backend/crates/agents/email/`)

A direct agent with platform ID `email`, over the existing `DirectSender`, `direct_send_batch`, cap, and retry flow from D1. The flow stays as it is, with one contract change: `send_batch` gains an `assets` argument (see Media).

- **Credentials** (the plan's schema), checked at connect time by opening an SMTP session and authenticating. The `remote_account_id` is the from address.
- **`EmailTransport`** trait with one SMTP implementation; C6 adds the rest behind it.
- **Rendering** of `EmailDocument` to responsive HTML through `mrml`, with the plain-text part built directly from the blocks.
- **Bounce classification**, headers, footer, and link rewriting.
- **Public routes** in `postit-api`: `GET`/`POST /public/u/{token}`, `GET /public/t/c/{token}`, `GET /public/t/o/{token}`, with localized pages.
- **`POST /accounts/{id}/test-email`** and **`GET /accounts/{id}/dns-check`**.
- **The real `LinkSigner`**, replacing `UnavailableLinks` (`server/src/unavailable.rs` is deleted).
- **Server wiring:** an `email` Cargo feature, on by default, the first real agent in the binary. `compose::agent_registry()` takes the settings it needs.

## Non-goals

- Web and mobile screens (D4, D6). No publish or campaign routes (D3).
- HTTP mail services and their webhooks (C6).
- A raw-HTML email block. D4 lists a raw-HTML mode, but `EmailBlock` has no such block and the roadmap's C2 block list does not include one. D4 adds the block and its renderer arm together with the editor that needs it.
- Resubscribing. The roadmap says an unsubscribe can be reversed only by the contact, and it specifies no route for that. Nothing here builds one, and no owner route reverses an unsubscribe.
- Open and click analytics. `email_events` rows are written; reading them is E3.

## Anti-goals

- Any path that mails an address that opted out, or lets anyone but the contact reverse the contact's own unsubscribe.
- An address, a name, a token, or message text in a log, a job payload, a job summary, an audit event, an import report, or an error.
- One bad sender reputation event suppressing a whole list (see Bounce classification).
- A link in sent mail that stops working because something was purged. Unsubscribe tokens never expire and need no row.

## Constraints

- `postit-agent-email` depends on `core`, `http`, and `agent-kit` only, plus its rendering and SMTP libraries. It never depends on `contacts`, `data`, or `postit-i18n`-backed services that reach a database. `postit-contacts` is not a dependency of any agent.
- One migration file until G1: schema changes edit `0001_init.sql`.
- All new SQL goes through `sqlx` macros with the offline cache refreshed. `cargo xtask openapi` is rerun after route changes.
- New error codes go into the web and mobile error maps in English and Arabic in this plan, even with no screen, because the contract CI job and the catalog completeness check cover them.
- `lettre` is already a workspace dependency; its feature list changes only as needed.

## Decisions on how it fits the code

### Import uses a staging table, not a file

Job payloads never carry addresses, and the job tables must never hold them, so the CSV has to wait somewhere. The request reads the upload as a stream, applies the column mapping, rejects what it can, and writes rows to a new `contact_import_rows` table in chunks (`import_id`, `owner_id`, `row_no`, `data jsonb`, composite FK to the import with cascade). The job then reads a chunk, applies it, and deletes those rows in one transaction.

This was chosen over a file on the media volume for three reasons: a crash resumes exactly where it stopped, a console retry cannot double-apply a row, and there is no file to clean up on user deletion or to share between the API and worker processes. Rows live only until applied.

- **Abandoned imports:** `data_retention` marks an import `failed` when it has been `processing` with no update for 24 hours (an upload that died, or a job out of retries) and deletes its staged rows, so staged addresses cannot outlive that. Each chunk updates the import, so a running import never matches. A failed import then falls under the normal 30-day purge.
- **A file that fails partway:** chunks are committed as the file is read, so a bad byte or a file past `csv_max_rows` can arrive after rows are staged. Any such error marks the import `failed`, deletes its staged rows, and queues no job. A client that disconnects is left to the stale sweep.

- **Request shape:** `multipart/form-data` with an `options` JSON part and then a `file` part, in that order, because the replay check needs the options before the file is read. This adds axum's `multipart` feature. `options` holds `attest_consent` (must be `true`, else `400`), an optional `list_id` (the import adds its contacts to that list; the owner must have it), and `columns`, a mapping from CSV header to contact field: `email` and `consent_source` required; `consent_at`, `first_name`, `last_name`, `tags`, and `locale` optional; any other mapped header goes into `fields`. A mapped header missing from the file answers `422 csv_invalid`. `list_id` is stored on `contact_imports`, because job payloads carry IDs only; deleting the list during an import clears it, and the import still completes. Tags in a cell are separated by `|`, which neither CSV separator uses.
- **Limits:** a new `[contacts]` config section with `csv_max_bytes` and `csv_max_rows`; over either, `413 csv_too_large`. The route's body layer sits slightly above `csv_max_bytes`, so only a declared length far beyond it answers `413 payload_too_large`. The import and export routes sit in the streaming route group, because the default group's 1 MiB limit and 30-second timeout would stop them. An `Idempotency-Key` is accepted, as plan 01 requires; a replay is recognized by the `options` JSON and `Content-Length`, as for media uploads.
- **Progress:** `ContactImportRepo` gains a progress update written with each chunk, so `GET /contacts/imports/{id}` shows live counts. `rows_total` is set when the upload finishes reading.
- **The report** keeps row numbers and reason codes only, up to 1,000 entries, plus per-code totals that keep counting past that. Rows rejected while staging reach it the same way as rows rejected by the job. Reason codes: `email_invalid`, `consent_missing`, `consent_at_invalid`, `row_malformed`, `duplicate_in_file`, `contact_limit_exceeded`.
- **At the cap,** rows for new addresses are rejected with `contact_limit_exceeded` and rows for existing addresses still update. The import completes rather than fails.
- **A re-import only adds.** An unmapped column or an empty cell keeps what the contact already has, so a file with just email and consent cannot clear names, fields, tags, or locale.

### Fixes to the existing contact repositories

- `ContactRepo::upsert` starts a new contact as `subscribed` when its only suppression is `manual`, because `contacts.status` has no `manual` value. The roadmap says suppressed addresses are imported as `unsubscribed`, so a `manual` entry maps to `unsubscribed`.
- `SuppressionRepo::remove` deletes by hash for any reason, but the route is `DELETE /suppressions/{id}` and is limited to `bounced` entries. `Suppression` gains its `id`, `remove` takes the ID, and any other reason answers `409 suppression_not_removable`. `Suppression` responses expose `id`, `reason`, and `at`, never the hash.
- `PATCH /contacts/{id}` cannot change the address (delete and add instead), and `status` accepts only `subscribed` and `unsubscribed`; `bounced` and `complained` come only from events. `PATCH status=unsubscribed` on a `bounced` or `complained` contact answers `409 status_not_settable`.
- **An owner's unsubscribe sticks.** A person often asks the owner directly, so `PATCH status=unsubscribed` also writes a `manual` suppression, and deleting and re-importing the contact does not undo it. `PATCH status=subscribed` on such a contact removes the `manual` entry, because the owner made it, and is owner-only since it removes a suppression (`403 owner_only` for a delegate). With any other suppression reason it keeps today's `409 suppressed`.
- **A contact's own opt-out outranks a bounce.** `add_hash` only upgrades `manual` today, so a contact who unsubscribes after a bounce would stay `bounced`, and the owner could remove the entry and mail them again. Reasons now rank: `manual`, then `bounced`, then `unsubscribed` and `complained`, which rank equal. A higher reason replaces a lower one. `unsubscribed` and `complained` move `subscribed` and `bounced` contacts; `bounced` moves `subscribed` contacts only. A bounce never replaces an opt-out.
- **The suppression list is usable.** `GET /suppressions` also returns `contact_id` and `email` when a contact with that hash still exists, joined by hash. Otherwise an owner could not tell which bounced entry to remove. Any `view` caller can already read contacts, so this shows nothing new. Removing a `bounced` entry moves that contact from `bounced` back to `subscribed`. When a `manual` entry was upgraded by a bounce, the contact is `unsubscribed`: removal then turns the entry back to `manual`, so the owner's unsubscribe survives.

### `LinkSigner` needs the owner

`LinkSigner::unsubscribe_url` gets a `DirectRecipient`, which has no owner, and the token must carry the owner ID. `AgentContext` is built per grant, so the owner is known there. `AccountServiceDeps.link_signer` becomes a `LinkSignerSource` (new core trait with `for_owner(UserId) -> Arc<dyn LinkSigner>`), and each context gets a signer bound to its owner. `unsubscribe_url` also gains a `language` argument, because the token carries the campaign's language and `DirectRecipient` does not. The implementation lives in `postit-api`'s public-route module, as the roadmap says, and the server injects it.

### Tokens

- Format: URL-safe base64 of a versioned payload, then a truncated HMAC-SHA256 tag with `email.signing_key`, compared in constant time.
- **Domain separation:** the key also makes `email_hash`, which is a bare HMAC of the address. Each token kind signs a fixed label prefix (`postit-unsub-v1`, `postit-click-v1`, `postit-open-v1`), so a token can never equal an address hash and one kind cannot pass as another.
- Unsubscribe carries owner, address hash, the campaign's language, and send ID. Click carries send ID and destination URL. Open carries send ID.
- **Tokens stay out of logs.** Request tracing and the leak harness treat `/public/u/` and `/public/t/` like the signed media routes.

### `GET` on the unsubscribe link does not unsubscribe

The roadmap says "either" `GET` or `POST` adds the suppression. Mail security scanners fetch links, so a state-changing `GET` would unsubscribe people who never clicked. `GET /public/u/{token}` shows a page with a confirm button; `POST` unsubscribes. The RFC 8058 one-click header posts to the same route, so one-click still works without a page. Marked as a deviation in the plan so the roadmap text is updated with it.

Pages are server-rendered with `askama` in English and Arabic (right-to-left). A tampered token answers a plain `404` page. The public routes need no bearer token and take `POST` without CSRF protection because the signed token is the authority. The pages carry their own `Content-Security-Policy`, `Referrer-Policy: no-referrer`, and `Cache-Control: no-store`, because the API sends none of these by default.

**Rate limits never break a link.** A bad token penalizes its IP, as on the signed media routes. A good token is never refused, because mail proxies and offices put many recipients behind one address. The per-IP limit only decides whether a click or open records its event, which bounds `email_events` rows.

### Validation

- **At least one list.** `take_audience` accepts an empty audience today, and the agent's `validate` runs after the audience is removed, so the agent cannot see it. The check goes in `take_audience` as `audience_invalid` for `options.lists`. D1's direct tests that use no list are updated.
- **Agent `validate`** covers a required document `language`, a postal address in the credentials, merge tags that parse, and rendered HTML within 100 KiB (`email_too_large`, new `ViolationCode`; Gmail clips above about 102 KB). The size counts every link at its signed length, since signed links are longer than the placeholders. Merge tags are refused inside URLs, in raw and percent-encoded form, because `Url` encodes braces in a path but not in a query.
- `open_tracking` (boolean, default off) is declared in the agent's options schema.

### Rendering

- **Merge tags:** `{{first_name}}` and `{{first_name|there}}` for a fallback. Allowed in subject, preheader, heading, text, and button label. A missing field with no fallback renders empty. Values are HTML-escaped in the HTML part and have CR and LF stripped in the subject.
- **Per-batch cost:** MJML compiles once per batch with sentinel placeholders, then each recipient gets a string substitution for merge values and signed links. `mrml` does not run once per recipient.
- **Document text is escaped** before it enters the MJML source, so a block can add neither a tag nor an `mj-include`.
- **Language is the campaign's.** `locale` is an optional import column and often empty, so it cannot decide direction: an Arabic campaign would render left-to-right with an English footer. `EmailDocument` gains a required `language` (`en-US` or `ar`). It sets body direction (`dir="rtl"` for Arabic), the footer language, and the unsubscribe page language, which matches the roadmap's "the campaign's language". `DirectRecipient.locale` stays in the contract but rendering does not read it.
- **Footer strings** are a small table inside the agent, English and Arabic, because the agent may not reach the catalog services. The footer holds the sender's postal address and the unsubscribe link and is added by the renderer, so no block can remove it.
- **Links:** `http` and `https` URLs in buttons and image links are rewritten through `click_url`. Others pass through.
- **Images** use `MediaHost::public_url` with `email.image_url_ttl`.

### Media kept alive for sent mail

Today's rule protects nothing an email needs. `MediaRepo::delete` reads `$.variants[*].media[*].asset` only, so an image used only in an email block is unprotected; the `email.image_url_ttl` rule from "Sending an email campaign" step 10 is not built; and any protection based on the snapshot would vanish with the post, because `publish_jobs` cascades from `posts` and deleting a post after a send is allowed.

- The snapshot query also covers `$.variants[*].email.blocks[*].asset`, for jobs that have not ended.
- **`media_assets.retain_until`** (new nullable column) is the durable rule. Materializing a direct delivery sets it to now plus `email.image_url_ttl` on every asset the document embeds, never moving it earlier, and each batch that sends mail extends it the same way. A campaign can run for weeks across cap crossings, and each batch signs its URLs for the full TTL, so the rule must count from the last send, as the roadmap says. `MediaRepo::delete` answers `media_in_use` until it passes, whatever has happened to the post. User deletion ignores it.
- **Assets reach the agent as an argument.** `MediaHost::public_url` needs a full `MediaAsset`, `EmailBlock::Image` holds only an ID, and `run_batch` discards `assembled.assets`. `send_batch` gains `assets: &[MediaAsset]`, matching `validate(&variant, &assets)`. The mock, the echo client, and the conformance suite change with it. `assemble` loads email block assets, and the test-email route does the same.

### SMTP and the address policy

The host is resolved through `postit_http::resolve_checked`, and the connection tries each checked address in turn, so `lettre` never does its own lookup (a re-resolve would reopen DNS rebinding). The route, checked against `lettre` 0.11.23, is `AsyncSmtpConnection::connect_tokio1` with a `SocketAddr`, which needs no lookup, and `TlsParameters::new(host)`, which sets the TLS name from the original host. It covers implicit TLS and STARTTLS. A server without STARTTLS is an error, never a plain fallback.

**The agent never calls `AsyncSmtpConnection::send`.** That method aborts the session on any refusal, so one bad address would end the batch, and it hides which SMTP stage refused. The session issues `MAIL`, `RCPT`, `DATA`, and the message itself through `lettre`'s public `command` and `message` methods, and sends `RSET` after a refusal.

**Every SMTP command has a timeout**, `email.command_timeout` (default 30 seconds). `lettre`'s async timeout covers the connect only, and a stalled relay would otherwise hold the batch's lease forever, because the renewer keeps extending it.

**A dropped connection reopens**, at most twice per batch. A recipient in flight when it dropped is a soft failure, which the roadmap's at-least-once rule allows. Without reopening, a relay's per-connection message limit would spend daily cap units on rows that were never attempted.

`email.allow_private_hosts` already exists and covers campaign links and images; it now also covers SMTP hosts, and its doc comment says so. `development.toml` sets it to `true` for Papercut on `localhost`. `tls = none` is refused unless it is on, so a password never crosses a public network in clear.

### Bounce classification

The roadmap says a 5xx suppresses the address. Applied literally, one sender-reputation rejection (`550 5.7.1`) would suppress every recipient of a campaign. So:

Each refusal is classified with the SMTP stage that gave it, because only `RCPT` speaks about the address:

- At `RCPT`, `550`, `551`, and `553` without a `5.7.x` enhanced code, and any `5.1.x` or `5.2.1` code, except the sender codes `5.1.7` and `5.1.8`: `Bounced { permanent: true }`, which suppresses through the existing `DirectRepo::record`.
- At `RCPT`, `5.7.x`, `5.1.7`, `5.1.8`, and other 5xx: `Failed { retryable: false }`, no suppression.
- At `DATA`, any 5xx: `Failed { retryable: false }`, no suppression.
- At `MAIL`, any 5xx refuses the sender, whatever its code, and suppresses nobody. See below.
- 4xx at any stage: `Bounced { permanent: false }`, retried by the existing soft-failure path.
- **Reason codes.** The `reason` of an outcome is stored in `direct_sends.error_code`, so it is a fixed code such as `smtp_5_1_1`. Server text is dropped, because replies often echo the address.
- **Sender-level rejection.** A relay refusing the from domain is a likely setup mistake, and per-recipient `Failed` would burn the whole audience with no resend, since D1 answers `not_retryable` once the audience is materialized. So when nothing in a batch was accepted and either a `MAIL` gets a 5xx or the first 3 recipients all get `5.7.x`, the agent returns `AgentError::Authentication`. D1's existing refused-credentials path then returns the rows and cap units, marks the account for reconnect, notifies the owner, and resumes when the setup is fixed. A `5.7.x` after a success in the batch is one recipient's server and stays per-recipient. A `MAIL` 5xx after a success ends the batch, with that recipient and the rest as soft failures.
- Authentication failure: `AgentError::Authentication` for the batch (the D1 path marks the account for reconnect). Connection failures and timeouts before anything was sent: a retryable batch error.

### Mail headers

`Message-ID` built from the send ID and the sending domain, which is also the `provider_message_id`; `List-Unsubscribe` with the HTTPS link; `List-Unsubscribe-Post: List-Unsubscribe=One-Click`; `Reply-To` when set. DKIM is the SMTP provider's job; postit does not sign. A test send carries none of the unsubscribe headers.

### Test email

`POST /accounts/{id}/test-email` takes an `EmailDocument` in its body rather than a post ID. Post routes are D3, and D4's composer needs to test unsaved drafts anyway. The route sends through `bound_client(...).direct()` to the acting user's own address, marked by the reserved `TEST_SEND_ID`. It is a send ID and not an option because a user could set an option on a real post, which would strip the unsubscribe headers from a real campaign. Since that ID is fixed, a test send takes a random `Message-ID`; Gmail drops or threads repeats. No row, no cap, no tracking rewrite. The footer unsubscribe link points at a test token that shows a "this was a test" page and suppresses nothing.

The route is a real send, so the runtime kill switch stops it (`409 live_publish_disabled`), as does a disabled platform. The document is validated first. A user with no address on file answers `409 email_address_missing`. A failed send answers with the agent's error code, and a refusal of the one recipient answers `409 test_send_refused`. Owner-only, limited by `rate_limit.email_checks`, with a 120-second handler timeout because the default 30 seconds is shorter than one slow SMTP exchange.

### DNS check

The route must reach email logic without `postit-api` knowing the agent, so core gains a small optional capability, `SenderDomainCheck`, exposed like `direct()` on the client, returning findings for SPF, DKIM, and DMARC as code plus status (`ok`, `missing`, `weak`, `unknown`), which the clients translate. Lookups use `hickory-resolver`. DKIM needs a selector, which cannot be discovered: the route takes an optional `dkim_selector`, and without one DKIM answers `unknown`. The mock agent does not implement it, and the route answers `409 capability_unsupported` for such an account. The route shares the test email's `rate_limit.email_checks`.

## What's left to the builder

- Task order, with C1 first so C2's end-to-end tests run against real contacts.
- The exact `mrml` feature set, and its MJML version and license, against the dependency rules.
- Whether address parsing uses `lettre::Address` or `email_address`, whichever adds less.
- The in-process SMTP sink: reuse the test SMTP server `postit-mail`'s tests use if it fits, provided it can refuse at any stage and answers `503` to `MAIL` inside an open transaction.
- The advisory-lock key and the import chunk size.
- Rate-limit numbers for the contacts routes, following the existing per-route settings.

## Testing

The test lists in Phases C1 and C2 apply in full, with the conformance suite in direct mode. Additions from this document:

- Import resumed after a crash mid-chunk, with no row applied twice.
- A console retry of `contact_import` after completion changing nothing.
- Two concurrent writers at the contacts cap.
- A `manual` suppression importing as `unsubscribed`.
- `DELETE /suppressions/{id}` on a non-bounced entry refused.
- Each token kind refused when used as another, and a tampered token refused.
- `GET /public/u/{token}` changing nothing.
- A test send clicking through to unsubscribe suppressing nothing.
- `550 5.7.1` not suppressing; `550 5.1.1` suppressing; a 5xx at `MAIL` suppressing nobody.
- Three leading `5.7.x` results pausing the campaign with no row failed, and it resuming after reconnect; a `5.7.x` after a success staying per-recipient.
- A refused recipient leaving the session usable for the next one.
- A dropped connection reopening, and a stalled server timing out.
- No server text in `direct_sends.error_code`.
- An owner-set unsubscribe surviving delete and re-import, and only the owner reversing it.
- An unsubscribe after a bounce replacing the entry, so it cannot be removed.
- An image's `retain_until` surviving deletion of the post, and moving forward with each batch.
- A stale `processing` import marked `failed` with its staged rows gone, and a running one left alone.
- A file failing mid-read leaving no staged rows.
- A re-import without a column keeping that column's values.
- A good token never rate-limited, and a bad one penalizing its IP.
- An Arabic campaign to contacts with no locale rendering right-to-left with an Arabic footer and unsubscribe page.
- A DNS rebinding case: the SMTP connection uses the checked address.
- An image used only in an email block refused deletion, during and after the job, until the TTL passes.
- No address or token in logs, job payloads, reports, or audit rows, through the shared leak harness.
- An end-to-end run through `PublishingService` with the email agent against the SMTP sink: multipart HTML and text arrive, unsubscribe works, per-recipient results are exact, and a cap crossing continues the next day.

## Human verification

Written to the plan's `-verification.md`: a campaign through a real Papercut, viewed in its UI, and rendering checks in real mail clients (Gmail, Outlook, Apple Mail), including Arabic. D3 adds the route that lets a person start one, so the Papercut check runs at D3 or through a test harness the maintainer runs.
