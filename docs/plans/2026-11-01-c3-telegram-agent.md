# C3 Telegram Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish text and media to Telegram channels and groups through the engine, and fix the two contract gaps the first real broadcast agent exposes.

**Architecture:**
- Tasks 1 to 3 change shared code: `PublishRequest` carries assets, `AgentError::AccountUnreachable` parks one account instead of a whole grant, and `postit-http` gains an opaque send. They ship first and stand alone, so C4, C5, and F inherit them.
- New `postit-agent-telegram` is a broadcast credentials agent over the Bot API. One `api` module owns every call and all error mapping. It depends on `core`, `http`, and `agent-kit` only.
- `postit-server` registers it behind a default-on `telegram` feature.

**Tech Stack:** Rust 2024, `reqwest` multipart with `stream` (workspace), wiremock, sqlx tests. No new workspace dependency.

**Spec:** `docs/specs/2026-11-01-c3-telegram-agent-design.md`. It argues from `docs/plans/03. publishing roadmap.md` phase C3. Read it first: its tables pin the error mapping, the send methods, the formats, and the options.

## Global Constraints

- Branch `c3-telegram` from `stage`. At plan time, no `*-deferred.md` or `*-verification.md` entry has a C3 trigger.
- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests). American English.
- `postit-agent-telegram` depends on `postit-core`, `postit-http`, `postit-agent-kit` only, never on `data`, `oauth`, or `media`.
- No schema change. The single migration stays untouched.
- The bot token appears only in the request URL, built inside `api`, and is sent only through `postit_http::execute_opaque`. It never reaches a `format!` that can end in an error or log.
- Every human-facing string is `postit_agent_kit::message(platform, "<key>", "<plain English>")`, keyed `telegram.…`.
- Limits, decimal bytes: photo `10_000_000`, other files `50_000_000`, width plus height `10_000`, ratio `20`, text `4_096`, caption `1_024`, album `2..=10`, all measured with `TextMeasure::Utf16Units`.
- Defaults: `api_base = https://api.telegram.org`, `upload_timeout = 10 minutes`, `getUpdates` with `timeout = 0` and `limit = 100`, and never an `offset` or `allowed_updates`.
- Never call `setWebhook`, `deleteWebhook`, `logOut`, or `close`.
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=K:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data`.
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`.

## Review Focus

1. **The token is revoked at BotFather with posts scheduled on three channels.** One `401` moves the grant and all three accounts to `needs_reconnect`, and all three park rather than fail. Test in Task 9.
2. **The bot was added to a channel, then removed, inside 24 hours.** The latest update wins, so the channel is not offered. Test in Task 5.
3. **What people paste into `chats`:** `https://t.me/mychannel/`, ` @MyChannel `, `t.me/+AbCdEf` (a private invite link, which cannot be resolved). The first two work; the third is `InvalidCredentials { field: "chats" }` before any call. Test in Task 4.
4. **A media file vanishes between scheduling and sending.** The store's `NotFound` is a clear permanent violation raised before the send call, so no album goes out short one item. Test in Task 7.
5. **A video with no probed dimensions (a build without `ffprobe`).** `width`, `height`, and `duration` are left out, never sent as `0`. Test in Task 7.

---

### Task 1: `PublishRequest` carries assets

**Files:**
- Modify: `backend/crates/core/src/agent/client.rs`, `backend/crates/core/src/agent/mod.rs` (fake and tests)
- Modify: `backend/crates/publishing/src/worker.rs:600`
- Modify: `backend/crates/agent-kit/src/testkit/conformance/checks.rs:156,400`, `backend/crates/agent-kit/src/testkit/fakes.rs` (`MemoryMediaStore` refuses a wrong owner), `backend/crates/agent-kit/tests/support/echo.rs`
- Modify: every `PublishRequest { … }` in `backend/crates/agents/mock/tests/`, adding `assets: &[]`
- Test: `backend/crates/publishing/tests/publish.rs`, `backend/crates/agent-kit/tests/conformance_http.rs`

**Interfaces:**
- Produces: `PublishRequest { variant, assets: &'a [MediaAsset], mode, native_publish_at, container_ref, idempotency_key }`. `assets` holds the variant's media in `variant.media` order, full and ready. `Commenter::comment` gets them too.

- [ ] **Step 1: Write the failing tests**

`publish_hands_the_agent_its_assets` in `publishing/tests/publish.rs`: a post with two stored images, run through `run_publish`; a recording fake agent sees `request.assets` with the two real IDs, the real `owner_id`, in order. `echo_reads_real_assets` in `conformance_http.rs`: a new `Defects::invents_assets` flag restores the old zero-owner `stored(item)`, and the media-store fake (taught to refuse an asset whose owner differs from the one it was given) makes the run fail.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-publishing --test publish publish_hands_the_agent_its_assets`
Expected: compile error, no field `assets`.

- [ ] **Step 3: Add the field and pass `&assembled.assets` in the worker**

The conformance suite passes `&fx.assets`. In `EchoClient::upload`, take the asset from `request.assets` by ID and delete `stored`, except behind the defect flag. Update the doc comment on `PublishRequest` and the conformance fixture's `assets` doc, which today names only `validate` and `send_batch`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-core -p postit-agent-kit -p postit-agent-mock -p postit-agent-email -p postit-publishing`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates
git commit -m "feat(core): hand publish the variant's assets"
```

### Task 2: `AccountUnreachable` parks one account

**Files:**
- Modify: `backend/crates/core/src/error.rs`
- Modify: `backend/crates/publishing/src/outcome.rs`, `backend/crates/publishing/src/worker.rs:622,863`
- Modify: `backend/crates/oauth/src/manage.rs`
- Modify: `backend/crates/data/src/accounts.rs`, the `.sqlx/` cache
- Modify: `backend/crates/agents/mock/src/script.rs`, `backend/crates/agents/mock/src/client.rs`
- Test: `backend/crates/core/src/error.rs` (unit), `backend/crates/data/tests/accounts.rs`, `backend/crates/oauth/tests/manage.rs`, `backend/crates/publishing/tests/publish.rs`

**Interfaces:**
- Produces, in core: `AgentError::AccountUnreachable { message: String }`, `code()` `"account_unreachable"`, `retry_class()` `NeedsReconnect`.
- Produces, in data: `AccountRepo::mark_needs_reconnect(conn, scope, id: AccountId) -> Result<bool, DataError>` (true when it moved `active` to `needs_reconnect`) and `AccountRepo::mark_active(conn, scope, id: AccountId) -> Result<bool, DataError>` (true when it moved `needs_reconnect` to `active`, only while the account's grant is `active`).
- Produces, in oauth: `AccountService::mark_account_needs_reconnect_in(&self, conn, scope, account: AccountId, audit) -> Result<(), AccountError>`.
- Produces, in publishing: `FollowUp::MarkAccount`.

- [ ] **Step 1: Write the failing tests**

- `account_unreachable_needs_reconnect` (core): class `NeedsReconnect`; `after_send()` leaves it unchanged.
- `mark_active_needs_an_active_grant` (data): moves the account back under an active grant, moves nothing under a `needs_reconnect` grant.
- `check_marks_only_the_unreachable_account` (oauth): the mock's script answers `AccountUnreachable` for one of two accounts on a grant; that one is `needs_reconnect`, its sibling and the grant stay `active`, one `account_needs_reconnect` event and one notice.
- `check_recovers_an_unreachable_account` (oauth): a good answer moves it back to `active`. `check_leaves_a_failed_grant_alone`: under a `needs_reconnect` grant it stays `needs_reconnect`.
- `publish_parks_one_account_when_unreachable` (publishing): two deliveries on one grant; the first answers `AccountUnreachable`, parks `account_needs_reconnect`, gives its quota back, and the second still publishes. `authentication_still_marks_the_grant` keeps the old behavior.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-core account_unreachable`
Expected: compile error, no variant.

- [ ] **Step 3: Implement**

`outcome.rs`: in the `RetryClass::NeedsReconnect` arm, follow with `MarkAccount` when the error is `AccountUnreachable`, else `MarkGrant`. `worker.rs`: `FollowUp::MarkAccount` calls `mark_account_needs_reconnect_in`; add the variant to `no_remote_call`. `manage.rs`: extract the audit-and-notify part of `mark_needs_reconnect_in` into one private helper over a list of accounts, used by both versions; in `check`, `Err(AccountUnreachable)` calls the account version and an active answer also calls `AccountRepo::mark_active`. The mock's `MockScript` gains a way to force the error; `InfoScript::Unreachable` fits next to `AuthFailed`. Run `cargo sqlx prepare` for the two new queries.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-core -p postit-data -p postit-oauth -p postit-publishing -- --test-threads=3`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend
git commit -m "feat(core): park one account when only it is unreachable"
```

### Task 3: Opaque send and `post_text`

**Files:**
- Modify: `backend/crates/http/src/client.rs`, `backend/crates/http/src/lib.rs`
- Create: `backend/crates/agent-kit/src/text.rs`
- Modify: `backend/crates/agent-kit/src/lib.rs`
- Test: `backend/crates/http/tests/redaction.rs`, `backend/crates/agent-kit/src/text.rs` (unit)

**Interfaces:**
- Produces: `postit_http::execute_opaque(client: &reqwest::Client, request: reqwest::Request) -> Result<reqwest::Response, HttpError>`. Same as `execute_traced` with `Sensitivity::Opaque`.
- Produces: `postit_agent_kit::post_text(variant: &Variant) -> String`.

- [ ] **Step 1: Write the failing tests**

`opaque_failure_carries_no_url` (http): a send to an unused local port with `/botSECRET/getMe` in the path; the `HttpError` text and the captured logs (`LogCapture`) do not contain `SECRET`.

`post_text` table (kit): `("hi", None, [])` gives `hi`; a link gives `hi\n\nhttps://a.example/`; a link already in the text is not repeated; tags give `hi\n\n#one #two`; link and tags give `hi\n\nhttps://a.example/\n\n#one`; a tag whose `#tag` is already in the text is skipped, matched case-insensitively; empty text with a link gives only the link; no extras returns the text unchanged, right-to-left text included.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-http --test redaction opaque && cargo test -p postit-agent-kit post_text`
Expected: compile errors.

- [ ] **Step 3: Implement both**

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-http -p postit-agent-kit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/http backend/crates/agent-kit
git commit -m "feat(http): add an opaque send and the shared post text"
```

### Task 4: Crate, descriptor, and credentials

**Files:**
- Create: `backend/crates/agents/telegram/Cargo.toml`, `src/lib.rs`, `src/descriptor.rs`, `src/credentials.rs`
- Modify: `backend/Cargo.toml` (member and workspace dependency)
- Test: `backend/crates/agents/telegram/src/credentials.rs` (unit), `backend/crates/agents/telegram/src/descriptor.rs` (unit)

**Interfaces:**
- Produces: `TELEGRAM: &str = "telegram"`; `TelegramConfig { api_base: Url, upload_timeout: Duration }` with `Default`; `TelegramAgent::new(config: TelegramConfig) -> Self`, a `Shared { descriptor, config }` behind an `Arc` as the email agent has.
- Produces, in `descriptor`: `descriptor() -> AgentDescriptor` and the limit constants from Global Constraints.
- Produces, in `credentials`: `Parsed { token: SecretString, chats: Vec<ChatRef> }`, `enum ChatRef { Username(String), Id(i64) }`, `parse(fields: &CredentialFields) -> Result<Parsed, AgentError>`.

- [ ] **Step 1: Write the failing tests**

`token_shape`: `123456:ABC-def_1` passes; empty, no colon, non-digit prefix, and a token with `/`, `?`, `#`, or a space give `InvalidCredentials { field: Some("bot_token") }`.

`chat_refs`: from the string `"@MyChannel, https://t.me/other/ ,t.me/third,-1001234567890"` come `Username("MyChannel")`, `Username("other")`, `Username("third")`, `Id(-1001234567890)`. An absent or blank field gives none. `t.me/+AbCdEf`, `t.me/joinchat/x`, `@a b`, and `https://example.com/x` give `InvalidCredentials { field: Some("chats") }`.

`descriptor_is_valid`: `descriptor().validate()` passes; the four formats match the spec's table; `kind` is `Broadcast`; no capabilities; no quota buckets; the options schema has `silent`, `protect_content`, `no_link_preview`, and `as_file`, each boolean defaulting to false.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-agent-telegram`
Expected: package not found.

- [ ] **Step 3: Implement**

Follow `agents/mock/src/descriptor.rs` for shape. `bot_token` is `x-postit-secret`. The `chats` help text names both accepted forms and the private-invite limit. The icon is a small paper-plane PNG drawn for postit in `#26A5E4`, stored as a file and read with `include_bytes!`. `verify_credentials` and `create_client` come in the next tasks; here, `create_client` returns `Unsupported`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-agent-telegram`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/Cargo.toml backend/Cargo.lock backend/crates/agents/telegram
git commit -m "feat(telegram): add the crate, descriptor, and credentials"
```

### Task 5: Bot API client, discovery, and account check

**Files:**
- Create: `backend/crates/agents/telegram/src/api.rs`, `src/discover.rs`, `src/client.rs`
- Modify: `backend/crates/agents/telegram/src/lib.rs`
- Test: `backend/crates/agents/telegram/tests/discover.rs`, `backend/crates/agents/telegram/tests/common/mod.rs`

**Interfaces:**
- Consumes: Task 3's `execute_opaque`, Task 4's `Parsed`, `ChatRef`.
- Produces, in `api`: `Api::new(http: &reqwest::Client, base: &Url, token: &SecretString, clock: &dyn Clock)` and typed reads `get_me() -> Result<Bot, ApiError>`, `peek_updates() -> Result<Vec<ChatMemberChange>, ApiError>`, `get_chat(&ChatRef)`, `get_chat_member(chat: i64, user: i64)`. Private `post<T>(method, Phase, RequestBody) -> Result<T, ApiError>` is the only place a URL is built.
- Produces: `enum ApiError { Agent(AgentError), Migrated(i64), Conflict }`, with `From<ApiError> for AgentError` where `Conflict` and `Migrated` that escape become permanent `Remote`.
- Produces, in `discover`: `can_post(chat_type: &str, member: &ChatMember) -> bool`, `discover(api: &Api<'_>, parsed: &Parsed) -> Result<Discovered, AgentError>`.
- Produces, in `client`: `TelegramClient { shared, ctx }` with `account_info`, `discover_accounts`, `publish_options`; the rest in Tasks 6 and 7.

- [ ] **Step 1: Write the failing tests**

With a wiremock Bot API in `tests/common/mod.rs` (`/bot<token>/<method>`, a recording of each request):

- `valid_token_lists_admin_chats`: `getMe` plus a peek with one channel where `can_post_messages` is true, one where it is false, and a private chat. Discovers only the first, with `remote_id` `-1001…`, the title, and `@handle`.
- `removed_then_lost_chat_is_not_offered`: updates `administrator` then `left` for one chat; nothing offered. The reverse order offers it.
- `peek_is_read_only`: the recorded `getUpdates` body has no `offset` and no `allowed_updates`.
- `peek_conflict_falls_back_to_named_chats`: `409` on the peek; a named chat still connects. With none named, `discovered` is empty and the call succeeds.
- `named_chat_without_rights_is_refused`: `InvalidCredentials { field: Some("chats") }`, both for `400 chat not found` and for a plain member of a channel.
- `named_group_member_needs_admin`: a `member` in a supergroup is refused; `administrator` passes.
- `bad_token_is_invalid_credentials`: `401` and `404` from `getMe` give `InvalidCredentials { field: Some("bot_token") }`.
- `rate_limit_reads_the_body`: `429` with `parameters.retry_after: 7` and no header gives `RetryAfter(7s)`. With no JSON, the `Retry-After` header wins.
- `account_info_active`, `account_info_lost_chat` (`403` gives `AccountUnreachable`), `account_info_bad_token` (`401` gives `Authentication`), `account_info_not_admin` (`AccountUnreachable`), `account_info_has_no_avatar`.
- `peek_skips_unknown_updates`: updates of unknown kinds and an unknown member `status` are ignored, not an error.
- `discover_accounts_matches_verify`: the client, reading stored fields, finds what `verify_credentials` found.

Hook table as unit tests in `api.rs`, matching the spec's "Error mapping" table row by row, with `Chat Not Found` proving the match ignores case.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-agent-telegram --test discover`
Expected: FAIL.

- [ ] **Step 3: Implement**

Deserialize only the fields used, each optional where the docs say so; `id` is `i64`, `status` a `String`. A failed call goes through `status_error`. `post` parses that `HttpError::Status` excerpt once: status `409` returns `Conflict`, `migrate_to_chat_id` returns `Migrated`, and anything else goes to `classify` with the one hook. The order in `discover`: `get_me`, peek, each named chat, dedupe by `remote_id` keeping the first.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-agent-telegram`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/agents/telegram
git commit -m "feat(telegram): add the Bot API client, discovery, and account check"
```

### Task 6: Validate and publish text

**Files:**
- Create: `backend/crates/agents/telegram/src/compose.rs`
- Modify: `backend/crates/agents/telegram/src/client.rs`, `src/api.rs`
- Test: `backend/crates/agents/telegram/src/compose.rs` (unit), `backend/crates/agents/telegram/tests/publish.rs`

**Interfaces:**
- Consumes: Task 1's `assets`, Task 3's `post_text`.
- Produces, in `compose`: `enum Send { Message, Photo, Animation, Video, Document, Album }`, `choose(variant: &Variant, assets: &[MediaAsset]) -> Send`, `Options::read(variant: &Variant) -> Options { silent, protect_content, no_link_preview, as_file }`, `violations(variant, assets) -> Vec<Violation>`.
- Produces, in `api`: `send_message(&SendText) -> Result<SentMessage, ApiError>`; `SentMessage { message_id: i64, chat: SentChat { id, kind, username } }`.
- Produces, in `client`: `receipt(&SentMessage) -> PublishReceipt`, used by Task 7.

- [ ] **Step 1: Write the failing tests**

`choose` table, unit: every row of the spec's method table, including QuickTime going to `Document` with `as_file` false.

`violations`, unit: a photo of 10,000,001 bytes without `as_file` gives one violation at `media[0]` naming the option; with `as_file`, none; width 6,000 and height 4,001 gives one; ratio 21 gives one, measured on the wider side both ways; missing dimensions give none. A 4,090-unit text with tags pushing it past 4,096 gives `text_too_long` with `max` `4096` and a `shorten_text` fix; text already over the limit alone gives none (the engine reports that). A caption with one image uses `1_024`.

`publish` integration, in `tests/publish.rs`:

- `text_post_is_plain_text`: the body has `chat_id` as a number, the composed text, and no `parse_mode`. The receipt has `remote_id` `"42"`.
- `options_reach_the_call`: `disable_notification`, `protect_content`, and `link_preview_options.is_disabled` are sent when set and left out when not.
- `receipt_links`: `https://t.me/name/42` for a public channel; `https://t.me/c/1234567890/42` for `-1001234567890`; `None` for a basic group.
- `dry_run_makes_no_call`, `cancel_before_send_makes_no_call`.
- `lost_response_is_outcome_unknown`, `server_error_after_send_is_outcome_unknown`, `unreadable_success_is_outcome_unknown`.
- `rate_limit_on_send_stays_retryable`: `429` is not `OutcomeUnknown`.
- `bad_request_is_permanent`.
- `migrated_group_repeats_once`: `400` with `migrate_to_chat_id` then success; the second call uses the new ID. A second `400` with a new ID is permanent, and exactly two sends went out.
- `reconcile_is_unsupported`, `status_needs_no_call`, `revoke_is_unsupported`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-agent-telegram`
Expected: FAIL.

- [ ] **Step 3: Implement**

`validate` returns `violations`. The publish path: dry run returns a plan (one line per item plus "send the post", and a line saying alt text is not sent when any item carries it); check `cancel_requested`; take the token; send on `Phase::AfterSend`; on `Migrated`, repeat once on the new ID. Media arrives in Task 7; here a variant with media returns `Unsupported`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-agent-telegram`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/agents/telegram
git commit -m "feat(telegram): validate and publish text posts"
```

### Task 7: Publish media

**Files:**
- Modify: `backend/crates/agents/telegram/src/api.rs`, `src/client.rs`, `src/compose.rs`
- Test: `backend/crates/agents/telegram/tests/media.rs`

**Interfaces:**
- Consumes: Task 6's `Send`, `choose`, `Options`, `receipt`.
- Produces, in `api`: `send_media(method: &str, form: multipart::Form) -> Result<SentMessage, ApiError>` and `send_album(form: multipart::Form) -> Result<Vec<SentMessage>, ApiError>`, both with `RequestBuilder::timeout(upload_timeout)` on `Phase::AfterSend`.
- Produces, in `client`: `part(asset: &MediaAsset, index: usize) -> Result<multipart::Part, AgentError>`: opens the store, wraps the stream in `postit_http::stream_body`, uses `Part::stream_with_length(body, asset.bytes)`, the asset's MIME type, and a generated `file{index}.{ext}`.

- [ ] **Step 1: Write the failing tests**

Multipart bodies are matched by a small parser in `tests/common/mod.rs` over wiremock's recorded bytes.

- `photo_goes_to_send_photo`: field `photo` has the file's bytes and `image/jpeg`; `caption` is the composed text; filename `file0.jpg`.
- `gif_goes_to_send_animation`; `as_file_goes_to_send_document`; `quicktime_goes_to_send_document`.
- `video_carries_probed_fields`: `supports_streaming`, `width`, `height`, and `duration` in seconds, rounded up.
- `video_without_probe_omits_fields`: none of the three is present.
- `album_shape`: `sendMediaGroup` with a `media` JSON field of three `{type, media: "attach://file{i}"}` in order, photos and videos mixed, `caption` on the first only, and three file parts. `remote_id` is the first message's ID.
- `album_as_file_is_all_documents`.
- `migrated_album_reopens_media`: the second attempt carries the full bytes of every file.
- `missing_media_stops_before_sending`: the store's `NotFound` for the third item gives a permanent `Validation` violation at `media[2]`, and no send call went out.
- `upload_stays_streamed`: a generated 40 MiB file through a counting store; the fake store's bytes are pulled while the server reads, never all at once. Reuse the bounded-memory idea of `upload_bounded_memory` in `agent-kit/src/upload.rs`.
- `upload_timeout_applies`: a server delaying past a short `upload_timeout` gives `OutcomeUnknown`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-agent-telegram --test media`
Expected: FAIL.

- [ ] **Step 3: Implement**

Open every stream before sending, so a missing file stops the post whole. Build the form fresh on each attempt, since a stream cannot be replayed. The asset for each `MediaItem` comes from `request.assets` by ID; a missing one is the same violation as a store `NotFound`. The extension comes from a fixed MIME table in `compose`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-agent-telegram`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/agents/telegram
git commit -m "feat(telegram): publish images, video, and albums"
```

### Task 8: Conformance

**Files:**
- Create: `backend/crates/agents/telegram/tests/conformance.rs`
- Modify: `backend/crates/agents/telegram/tests/common/mod.rs`

**Interfaces:**
- Consumes: `postit_agent_kit::testkit::conformance::{ConformanceFixture, Faults, Fault, run_conformance}`. `agent-kit/tests/support/mod.rs` has the wiremock `Faults` pattern and `responders`.

- [ ] **Step 1: Write the test**

`telegram_passes_conformance`: `deduplicates: false`, samples `[image, text, album, video]` with real assets in `MemoryMediaStore`, `upload_timeout` of one second, credentials valid and invalid (a `401` from `getMe`), and `secrets` holding the token. Faults mount on the `send*` paths; `mutating_calls` counts those paths only. `report.assert_passed()`, and `report.skipped` equals `["idempotency", "direct", "oauth_config_fields"]`.

`conformance_catches_a_token_leak`: a test-only wrapper that puts the token in an error's text fails `redaction`, which proves the check looks at the real token.

- [ ] **Step 2: Run it**

Run: `cargo test -p postit-agent-telegram --test conformance`
Expected: PASS; if a check fails, fix the agent, not the fixture.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/agents/telegram
git commit -m "test(telegram): pass the conformance suite"
```

### Task 9: Server feature and the engine test

**Files:**
- Modify: `backend/crates/server/Cargo.toml`, `backend/crates/server/src/compose.rs:29-45`
- Create: `backend/crates/server/tests/telegram_publish.rs`
- Test: `backend/crates/server/tests/agents.rs`

**Interfaces:**
- Consumes: `TelegramAgent::new`, `run_publish`, `run_sweep`, and the fixture pieces in `backend/crates/server/tests/campaign/mod.rs`.
- Produces: feature `telegram = ["dep:postit-agent-telegram"]`, in `default`.

- [ ] **Step 1: Write the failing tests**

`telegram_is_registered` in `agents.rs`: `compose::agent_registry` lists `telegram`.

In `telegram_publish.rs`, with a test registry holding `TelegramAgent` on a wiremock `api_base`, and an account connected through `AccountService` with a token and a named channel:

- `text_post_reaches_the_channel`: `Published`, `remote_url` stored.
- `album_is_read_from_the_real_store`: two images uploaded through `MediaService`; the bytes at wiremock equal the files.
- `lost_send_parks_unresolvable`: the response drops; the delivery ends `Parked(outcome_unresolvable)`, one send reached the server, and the sweep does not re-send.
- `revoked_token_parks_every_channel`: three accounts on one grant and three scheduled posts; the first `401` marks the grant; all three park `account_needs_reconnect`, and only one send went out.
- `removed_bot_parks_one_channel`: `403` on one of two; the other publishes; the `check` after the bot returns moves it to `active`, and the sweep unparks it inside `park_grace`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-server --test telegram_publish -- --test-threads=3`
Expected: FAIL, no such feature or test.

- [ ] **Step 3: Add the feature and one registration line**

The `cfg_attr` on `settings` and `mut` in `agent_registry` gain `telegram`. `TelegramConfig::default()` needs no settings.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p postit-server -- --test-threads=3`, then `cargo check -p postit-server --no-default-features`
Expected: PASS. `cargo deny check` from `backend/` passes.

- [ ] **Step 5: Commit**

```bash
git add backend
git commit -m "feat(server): register the Telegram agent"
```

### Task 10: Live test, docs, and close-out

**Files:**
- Create: `backend/crates/agents/telegram/tests/live.rs`
- Create: `docs/plans/2026-11-01-c3-telegram-agent-deferred.md`, `docs/plans/2026-11-01-c3-telegram-agent-verification.md`
- Modify: `docs/plans/03. publishing roadmap.md`, `docs/plans/01. vision and architecture.md`, `docs/configuration.md`, `docs/security.md`, `CLAUDE.md`

- [ ] **Step 1: Write the live test**

`#[ignore]`, returns early unless `POSTIT_LIVE_TESTS=1`. Reads `POSTIT_TELEGRAM_TOKEN` and `POSTIT_TELEGRAM_CHAT`; connects, posts a text, an image, and a two-item album, and asserts each receipt. Add a header comment saying it posts for real.

- [ ] **Step 2: Correct the roadmap**

- C3: note it was done first as the contract check, and that `Unsupported` reconcile and the phase-start answers live in the spec.
- E2: replace "if C3's phase-start check allowed it" with: feasible, needs a per-grant update reader that routes one bot's updates to accounts, a confirming `offset`, and a poll at least every 24 hours, and discovery's peek must keep working once updates are confirmed.
- E3 and the risk list: Telegram has no per-post views on the Bot API; the account metric is the subscriber count.
- Parallel map: the build order is C3, D2, D3, then C4 and C5.
- `MediaSource` note: agents read media through `PublishRequest.assets`.

- [ ] **Step 3: Update the other docs**

`01. vision`: `AccountUnreachable`, in the grants and contract notes. `configuration.md`: the `telegram` feature. `security.md`: `execute_opaque`, the read-only peek, and no avatar URLs. `CLAUDE.md`: the crate in the list and a C3 line in Project state with the open-item file names.

- [ ] **Step 4: Write the deferred file**

Each entry with its issue, why not now, and trigger:
- **Telegram inbox and comments** (trigger E2): the per-grant update reader, from the spec's inbox finding.
- **Telegram account metric** (trigger E3): subscriber count through `getChatMemberCount`; no post views.
- **Translated strings and the connection guide** (trigger D4, then D6): Arabic for every `telegram.*` key, and the BotFather guide: create a dedicated bot, add it as a channel admin with "Post messages", add a private channel by numeric ID, revoke from BotFather.
- **The stored chat ID after a group upgrade** (trigger E2): a `remote_account_id` update path if the inbox needs the current ID.

- [ ] **Step 5: Write the verification file**

Each entry with the item, why an agent cannot do it, and its trigger:
- **A real channel and a real group** (now, or D3 at the latest): run `live.rs` with a real bot, then check on a phone: text, a photo, a WebP, a GIF, an MP4, a MOV as a file, a 10-item album, a caption in Arabic, and the link in each receipt, for a public and a private channel.
- **Limits in practice** (with the above): an emoji-heavy 4,096-unit text, to learn whether `Utf16Units` can be relaxed; a 49 MB video on a real uplink within `upload_timeout`.
- **Discovery on a real bot** (with the above): add the bot to a channel and see it offered; confirm a bot with a webhook keeps it and still connects by name.

- [ ] **Step 6: Run the gates**

From `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace -- --test-threads=3` in the background with a log, and `cargo deny check`. Through PowerShell: `cargo xtask openapi --check`.
Expected: all pass, and no contract diff, since no route changed.

- [ ] **Step 7: Commit**

```bash
git add backend docs CLAUDE.md
git commit -m "docs: record C3 and correct the Telegram roadmap"
```
