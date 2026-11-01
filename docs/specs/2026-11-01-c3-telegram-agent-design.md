# C3 Telegram agent: design

Date: 2026-11-01. Source: plan 03 (`docs/plans/03. publishing roadmap.md`), phase C3, plus "Connecting an account with credentials", "Publishing", "Delivery states", "Formats and validation", and the platform constraints. Plan 01 covers grants, the crate map, and the conformance suite. This document records intent, the phase-start findings, and the decisions the plan left open. Decisions marked **(call)** are the designer's; the user chose the phase and its order and left the rest open.

## Intent

C3 ships Telegram as a platform people can use, and it is also the **contract check**. Until now the agent contract has been proven only by the mock and by email, a direct agent. Telegram is the first real broadcast agent, so it goes first, before D2 and D3, to find contract gaps while fixing them is still cheap: D3 exposes the contract over REST, and the D4 and D6 composers build on it. C4 and C5 follow D3. The order is C3, D2, D3.

The check already paid off. Reading the code against Telegram's needs found two gaps that any broadcast agent with media or with a multi-account grant would hit. Both are fixed here (see Contract changes).

## Goals

- Everything under plan 03 phase C3, meeting its exit criteria: a Telegram channel receives text and media posts through the engine, and errors map to clear `AgentError`s.
- `postit-agent-telegram` at `backend/crates/agents/telegram/` passes the conformance suite in broadcast mode with `deduplicates: false`.
- Both contract gaps fixed in core and the engine, so C4, C5, and F inherit working contracts.
- The server's `telegram` Cargo feature, on by default, registers the agent in every build.
- An engine-level test: an account connected through `AccountService`, a post published through `PublishingService`, against a wiremock Bot API, with real files in the real media store.
- An opt-in live smoke test (`POSTIT_LIVE_TESTS=1`, `#[ignore]`).
- Phase-start checks answered and written down (below), with the roadmap corrected where they disprove it.

## Non-goals

- `InboxReader`, `Commenter`, and `MetricsReader`. Inbox and comments land with E2, metrics with E3. See the phase-start findings: both need design work those phases must do.
- `NativeScheduler`. The Bot API has no scheduled send, so postit scheduling covers it.
- Webhooks. The agent never calls `setWebhook` or `deleteWebhook`.
- A local Bot API server (uploads up to 2,000 MB). The API base is not a deployment setting.
- Forum topics (`message_thread_id`), polls, stickers, inline keyboards, voice, and video notes.
- Sending media by URL or by `file_id`. Multipart has the higher limits (10 MB and 50 MB against 5 MB and 20 MB) and works when `media.public_base_url` is private.
- Client screens and the translated connection guide (D4, D6). The `telegram.*` message keys ship with English text; their Arabic and the guide are D4's.
- A publish route (D3).
- A short-window rate limiter. Sends are rare per chat, and a `429` carries `retry_after`, which the engine honors.

## Anti-goals

- The bot token in a log line, span, error, `Debug` output, receipt, or stored field. It sits in the URL path of every call, the one credential in the system that does.
- A change to the user's bot. postit never confirms updates, never changes the bot's `allowed_updates`, and never touches its webhook. A bot that runs other software keeps working.
- A blind retry of a send whose outcome is unknown. Telegram has no lookup and no dedupe key.
- One removed channel taking down every other channel on the same bot.
- Any media file fully buffered in memory.
- A file name the user chose reaching Telegram. Upload names are generated.

## Constraints

- Agent crates depend on `postit-core`, `postit-http`, and `postit-agent-kit` only.
- Plan 01: "Adding a platform = new crate + feature + one registration line".
- Single migration policy. C3 needs no schema change: `connected_accounts.status` already allows `needs_reconnect` per account.
- Credential agents need no deployment config. `TelegramAgent::new` takes a config struct that the server builds from defaults; tests override it.
- Quality gates in `CLAUDE.md`, plus `cargo xtask openapi --check` and `cargo deny`.

## Phase-start findings

Read from `core.telegram.org/bots/api` on 2026-11-01 (Bot API 10.3, August 24, 2026) and `core.telegram.org/bots/faq`. Verify again if the phase slips.

**Upload limits.** Multipart: photos 10 MB, other files 50 MB. By URL: 5 MB and 20 MB. Photo width plus height at most 10,000, ratio at most 20. `sendVideo` takes MPEG-4; other formats go as documents. `sendMediaGroup` takes 2 to 10 items; photos and videos mix; documents only group with documents.

**Text.** Message text is 1 to 4,096 characters and captions 0 to 1,024, both after entities parsing. The docs do not define "character" for these limits; entity offsets are UTF-16 code units. **(call)** Measure with `TextMeasure::Utf16Units`, which never counts fewer than Telegram can. An emoji-heavy post may be stopped a little early; it is never refused by the platform. Live checks may relax this later.

**Rate limits.** About one message per second per chat, 20 per minute per group, about 30 per second per bot. Over the limit: `429` with `parameters.retry_after` in the JSON body.

**Errors.** Failures are `{ok: false, error_code, description, parameters?}`. `parameters` has `retry_after` and `migrate_to_chat_id`. `description` is free text with no stability promise, and `error_code` is "subject to change", so the agent decides from the HTTP status and `parameters` wherever it can.

**Updates.** `getUpdates` keeps updates at most 24 hours, fails while a webhook is set, and conflicts with any other `getUpdates` caller. Calling it with an `offset` confirms everything below that offset, for every reader of the bot. `allowed_updates`, once passed, stays in force for later calls. The default covers `my_chat_member`.

**Metrics: the roadmap is wrong.** The Bot API has no message view count; `views` appears nowhere in it. E3's "Telegram views" cannot be built on the Bot API. `getChatMemberCount` gives the subscriber count, so an account metric is possible and a per-post one is not. The roadmap is corrected in this phase.

**Inbox: feasible, with design work for E2.** Comments on a channel post arrive in the linked discussion group (`ChatFullInfo.linked_chat_id`). The bot must be in that group and either be an admin or have privacy mode off, since comments reply to an automatic forward rather than to the bot. Reading them means `getUpdates` with a confirming `offset`, which does not fit the contract: the queue belongs to the bot and so to the **grant**, while `InboxReader::fetch_since` takes one account and one cursor. Fetching for one channel would consume another's updates, and confirming would erase the `my_chat_member` updates discovery reads. Retention also requires a poll at least every 24 hours. E2 must add a per-grant update reader that routes updates to accounts. No contract change now: nothing can test one until E2. Recorded in the deferred file.

## Contract changes

### Gap 1: `publish` cannot read media

`PublishRequest` carries the `Variant`, whose `MediaItem` holds only an asset ID. `MediaStore::open` takes a full `MediaAsset`, and `FsMediaStore::open` reads `owner_id` and `id` from it. `validate` and `send_batch` receive assets; `publish` does not. The test-only `EchoAgent` hid this by inventing an asset, with a zero owner that the real store would refuse. So no real agent can publish media today, and an agent also cannot learn a file's type, size, or dimensions, which Telegram needs to choose its method.

Fix: `PublishRequest` gains `assets: &'a [MediaAsset]`, in the order the variant lists them. The worker already holds them in `assembled.assets`. The conformance suite passes the fixture's assets, and `EchoAgent` drops its invented asset. Comments get the same field, since `Commenter::comment` takes `PublishRequest`.

### Gap 2: a lost account marks the whole grant

`RetryClass::NeedsReconnect` means `FollowUp::MarkGrant`, and a non-active `AccountInfo` makes `check` mark the grant too. One bot token is one grant reaching many channels. A bot removed from one channel would move every channel on that token to `needs_reconnect` and park all their posts. Facebook Pages and YouTube channels share this shape.

Fix: **(call)** a new `AgentError::AccountUnreachable { message }`: "The credential still works, but this account can no longer be reached through it." Its `retry_class` is `NeedsReconnect`, so the delivery parks and the owner is told, as for a revoked credential.

- `outcome.rs` follows it with a new `FollowUp::MarkAccount` instead of `MarkGrant`. The worker counts it as a proven refusal and gives the quota back.
- `AccountService::check` marks that one account on `Err(AccountUnreachable)`, through a new `mark_account_needs_reconnect_in` that shares the audit event and notice with the grant version. Preflight gets this through `check`.
- **Recovery:** when `check` gets a good answer for a `needs_reconnect` account whose grant is `active`, it moves the account back to `active`. The user puts the bot back in the channel and presses Check, without finding the token again; the sweep then unparks within a minute. A grant in `needs_reconnect` still needs a real reconnect.

### Not changed: remote account IDs can change

A Telegram group upgraded to a supergroup gets a new chat ID, and the contract has no way to update `remote_account_id`. **(call)** The agent follows `migrate_to_chat_id` inside the call: the refusal proves nothing was posted, so it repeats the call once on the new ID. The stored ID stays old, costing one refused request per post. If E2 needs the current ID, it adds the update path then.

## Decisions

### Crate layout

`postit-agent-telegram`, modules: `descriptor` (limits, formats, schemas), `credentials` (parsing and checks), `api` (the Bot API client: every call and all error mapping), `discover`, `compose` (the method and fields for a variant), `client` (`AgentClient`), `lib` (`TelegramAgent`, `TelegramConfig`). `TelegramConfig { api_base: Url, upload_timeout: Duration }` defaults to `https://api.telegram.org` and 10 minutes; tests set a wiremock URL and a short timeout. **(call)** The timeout is a constant in practice because a credential agent has no deployment config; 50 MB at 1 Mbit/s takes about 400 seconds.

### Credentials

- `bot_token` (secret, required) and `chats` (plain, optional): chats to add by hand, comma-separated. Each entry is `@name`, a `t.me/name` link, or a numeric chat ID.
- **(call)** The token must match digits, `:`, then `[A-Za-z0-9_-]`, checked before any call, because it goes into a URL path. Anything else is `InvalidCredentials { field: "bot_token" }`, as are a `401` and a `404` from `getMe` (Telegram answers `404` for a malformed token).

### Discovery

`verify_credentials`, in order: `getMe`; one `getUpdates` peek; each named chat. `discover_accounts` runs the same code from the stored fields.

- **The peek is read-only (call).** No `offset`, so nothing is confirmed, and no `allowed_updates`, since a value would stay in force and could silently stop another program using the same bot from receiving messages. `timeout` 0, `limit` 100.
- Each `my_chat_member` update gives a chat. The latest update per chat wins. A chat counts when it is a `channel`, `group`, or `supergroup` and `can_post` holds.
- `can_post`: in a channel, status `administrator` with `can_post_messages`, or `creator`. In a group or supergroup, `administrator` or `creator`. Discovery and hand-added chats share this one rule, matching the catalog's "where it is an admin".
- **A `409` from the peek is not an error (call).** It means a webhook or another poller owns the queue. Discovery falls back to the named chats, and the guide says so.
- A named chat is read with `getChat` and `getChatMember`. If it cannot be read, or `can_post` fails, the connect fails with `InvalidCredentials { field: "chats" }`: the user asked for that chat by name.
- Nothing found is not an error: B3 returns `discovered: []` and the client explains. Updates older than 24 hours are gone and only the first 100 pending are read, so the hand-added path is the documented fallback.
- `remote_id` is the numeric chat ID as a string, since a username can change. `display_name` is the title, `handle` is `@username`. **(call)** `avatar_url` is always `None`: a chat photo's URL contains the bot token.

### Account check

`account_info` calls `getMe`, `getChat`, then `getChatMember` for the bot. A failed token is `Authentication`. A chat the bot cannot reach, or where `can_post` fails, is `AccountUnreachable`.

### Error mapping

One `ErrorHook` in `api`, used by every call:

| Response | Result |
|---|---|
| `429` | `RateLimited`, `retry_after` from the JSON body, else the `Retry-After` header |
| `401` | `Authentication`: the token was revoked |
| `403` | `AccountUnreachable`: removed from the chat, or blocked |
| `400` with `migrate_to_chat_id` | an internal `Migrated`, repeated once on the new ID |
| `400` whose `description` says the chat is missing or admin rights are lacking | `AccountUnreachable` |
| other `400` | `Remote` (permanent) with the description |
| `5xx`, timeouts, connection loss | the kit's defaults; `Phase::AfterSend` on send calls makes them `OutcomeUnknown` |
| `200` with an unreadable body on a send | `OutcomeUnknown`: the post may exist |

**(call)** The one match on `description` text is case-insensitive on `chat not found`, `not enough rights`, and `admin`, because `400` is otherwise all that separates a lost chat from a bad message. If Telegram rewords it, the failure becomes a clear permanent error rather than a park. The excerpt is at most 512 bytes and already redacted; the hook parses it as JSON and treats anything else as no match.

### Publishing

- The live path checks `cancel_requested` once, before the single send call. There is no multi-step upload and nothing to resume: an album is one request, so a post never lands in part.
- **Text (call):** the agent sends plain text with no `parse_mode`, so what the user wrote is what goes out, with no escaping risk, and Telegram detects links, mentions, and hashtags itself. That is the plan's "entity-aware measure": nothing is stripped, so the measured text is the sent text.
- **Composition (call):** a new `postit_agent_kit::post_text(&Variant)` joins the text, the link (unless the text already contains it), and the tags as `#tag` (skipping any already in the text). C4, C5, and F reuse it. The core engine measures only `variant.text`, so the agent's `validate` reports `text_too_long` when composition alone pushes it over.
- **Method:**

| Content | Call |
|---|---|
| no media | `sendMessage` |
| one GIF | `sendAnimation` |
| one image | `sendPhoto` (`sendDocument` with `as_file`) |
| one MP4 | `sendVideo` with `supports_streaming`, plus width, height, and duration when known (`sendDocument` with `as_file`) |
| one QuickTime | `sendDocument`, always |
| 2 to 10 items | `sendMediaGroup`, all as documents with `as_file`; caption on the first item |

- **Media is streamed.** Each file is one `reqwest::multipart::Part::stream_with_length` over `MediaStore::open`, with the asset's stored type and a generated name (`file0.jpg`). Multipart calls use `upload_timeout`; others keep the client's 30-second default.
- **Receipt:** `remote_id` is the message ID (the first of an album). `remote_url` is `https://t.me/{username}/{id}` for a public chat, `https://t.me/c/{id}/{message}` for a private channel or supergroup (the chat ID without its `-100` prefix), and `None` for a basic group.
- `status` returns `Published` from the receipt with no call: Telegram posts have no processing step. `reconcile` returns `Unsupported`, so ambiguous sends park as `outcome_unresolvable`. `revoke` stays `Unsupported`: `logOut` would disable the bot for 10 minutes, so the guide sends people to BotFather.
- `disclose_ai_generated` has no Telegram field and is not added to the text. Alt text has no Telegram field and is dropped; the dry-run plan says so when items carry it.

### Formats and options

| Format | Text | Media |
|---|---|---|
| `text` | required, 4,096 | none |
| `image` | optional, 1,024 | 1; JPEG, PNG, WebP, GIF; at most 50 MB |
| `video` | optional, 1,024 | 1; MP4, QuickTime; at most 50 MB |
| `carousel` | optional, 1,024 | 2 to 10 images or videos; JPEG, PNG, WebP, MP4; at most 50 MB each |

`image` and `carousel` say 50 MB because `as_file` raises the photo limit, and a format ID can be declared only once. The agent's `validate` holds the photo rules: without `as_file`, a photo over 10 MB, with width plus height over 10,000, or a ratio over 20 is a violation at `media[i]` that names the `as_file` option. All sizes are decimal (10,000,000 and 50,000,000 bytes), the safe reading of the docs.

Options, all boolean, default false: `silent` (`disable_notification`), `protect_content`, `no_link_preview` (`link_preview_options.is_disabled`), and `as_file`.

No quota buckets: Telegram has no calendar quota. Capabilities: none. Brand color `#26A5E4`. The icon is a generic paper-plane PNG drawn for postit, never Telegram's logo.

### Token safety

The token cannot move out of the URL. Every Bot API call goes through a new `postit_http::execute_opaque`, which sends with `Sensitivity::Opaque`: the span shows `[redacted-url]` and errors carry no URL. Unlike the built-in `api.telegram.org` redactor pattern, this holds for any base URL, including wiremock, so the conformance `redaction` check proves the real path. C4's webhook URLs need the same call. The token lives in a `SecretString` and is exposed only to build the URL.

## Testing

- Unit: token and chat parsing, `can_post`, the latest-update-wins rule, `post_text`, method selection, receipt URLs, the error hook table.
- Wiremock per client call, including: a `429` with body `retry_after` and no header; a `401` at publish; a `403` giving `AccountUnreachable`; `migrate_to_chat_id` repeating once and only once, with media re-opened; a `409` peek falling back to named chats; a peek that sends no `offset` and no `allowed_updates`; an album's multipart shape and `attach://` references; a streamed upload of a large generated file with bounded memory.
- Conformance with `deduplicates: false`: `reconcile` is `Unsupported`, and `redaction` runs with the token in the fixture's secrets.
- Contract: `EchoAgent` reads real assets; a broken one fails. `AccountUnreachable` parks one account and leaves its siblings active; `check` recovers it; a grant-level failure still marks them all.
- Engine (`server/tests/telegram_publish.rs`): connect through `AccountService`, publish a text post, a single image, and an album through `run_publish` with files in the real media store; a send lost mid-flight parks `outcome_unresolvable` with one send made; a retry after resolve is the user's.
- Isolation: no new route, so no new matrix. `check`'s change is covered in `postit-oauth`.
- Live (`#[ignore]`, `POSTIT_LIVE_TESTS=1`): token and channel from the environment; connect, post text, an image, and an album.

## Left to the builder

- Internal function signatures and module boundaries, as long as the behavior above holds.
- The exact English wording of `telegram.*` messages, in plain language.
- The icon's drawing.
