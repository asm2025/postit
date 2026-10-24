# B5: Agent Kit and Mock Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every agent shared machinery and one conformance suite (`postit-agent-kit`), and register an in-memory mock agent with every capability in development and CI builds (`postit-agent-mock`), so later phases have a platform to build against.

**Architecture:**
- Core gains `AgentError::Cancelled` / `RetryClass::Cancelled`. Nothing else in the contract changes.
- `postit-agent-kit` (level 8: `core`, `http`) holds handle downcasts, `HttpError` → `AgentError` mapping, `poll_until`, the chunked upload driver, and declaration helpers; its `testkit` feature holds in-memory fakes, `TestContext`, wiremock fixtures (`MockOAuthProvider`), and `run_conformance`.
- `postit-agent-mock` (level 9) implements `mock`, `mock_direct`, and `mock_oauth` over one in-memory `MockPlatform`, scripted by pasted tokens, post options, recipient addresses, or a Rust `MockScript`. `postit-server` registers `mock` and `mock_direct` behind the `mock-agent` feature. B3's test agent is replaced by the mock.

**Tech Stack:** Rust 2024, `async-trait`, `reqwest` through `postit-http`, `tokio` (time, paused-time tests), `futures`/`bytes` streams, `wiremock` (testkit only), `secrecy`.

**Spec:** `docs/superpowers/specs/2026-10-24-b5-agent-kit-mock-design.md`, which argues from `!ref/plans/03. publishing roadmap.md` phase B5. Read both. Every **(call)** in the spec is decided; do not reopen it.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests and outside the `testkit` modules, which may `#[allow]` them as other crates' testkits do).
- Dependency order: `postit-agent-kit` depends on `postit-core`, `postit-http`, and external crates only. `postit-agent-mock` depends on `postit-core`, `postit-http`, `postit-agent-kit`. Neither depends on `oauth`, `data`, `media`, `vault`, `config`, or `api`. `postit-oauth` never depends on either except as a dev-dependency.
- New crates are workspace members and `[workspace.dependencies]` entries (`postit-agent-kit = { path = "crates/agent-kit" }`, `postit-agent-mock = { path = "crates/agents/mock" }`). No new external dependency; use the existing workspace ones. No dependency with an Arabic or Arabic-derived name.
- Platform IDs: `mock`, `mock_direct`, `mock_oauth`. Every agent `Message` key starts with `<platform>.`; kit-owned messages start with `agent_kit.`.
- No secret (pasted token, OAuth token, signed URL) in a log, span, error `Display`/`Debug`, outcome, receipt, or report.
- Production builds never contain the mock: `mock-agent` is off by default, and startup refuses `environment = "production"` when it is compiled in.
- After every task, from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests (separate `CARGO_TARGET_DIR`; see memory "Test env gotchas"). The last task runs every gate in `CLAUDE.md` plus `cargo xtask openapi --check`.
- Commits: Conventional Commits, scope = crate touched (`core`, `agent-kit`, `agent-mock`, `oauth`, `api`, `server`, `docker`, `ci`, `docs`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- American English in code, docs, and messages.

## Review Focus

1. **An empty media file or a resume offset past the end** handed to the upload driver. Both fail with a permanent `Validation` (`agent_kit.media_empty`, `agent_kit.resume_offset_beyond_end`) before any chunk is sent, never a silent success. Task 3 tests both.
2. **A pasted mock token with stray spaces or capitals** (`" Invalid "`). Keywords match after trim and ASCII lowercasing, so a hand-typed test token behaves as documented. Task 7 tests it.
3. **`account_name` with duplicates or empty segments** (`"shop,, shop ,blog"`). Discovery trims, drops empties, and de-duplicates, giving `shop` and `blog` in first-seen order. Task 7 tests it.
4. **Two clients of one mock agent publishing the same idempotency key at once.** Exactly one post exists and both get the same receipt. Task 7 tests it with `tokio::join!`.
5. **`poll_until` with a timeout shorter than its interval.** The step still runs once before `TimedOut`, so a post that is already live is reported as live. Task 3 tests it.

---

### Task 1: Cancellation in the core contract

**Files:**
- Modify: `server/crates/core/src/error.rs`, `server/crates/oauth/src/error.rs` (exhaustive matches near lines 88 and 111)
- Test: unit tests in `error.rs`

**Interfaces:**
- Produces: `AgentError::Cancelled` (unit variant, `#[error("cancelled at a safe checkpoint")]`, doc: returned only before the final create or publish call); `RetryClass::Cancelled`; `AgentError::Cancelled.retry_class() == RetryClass::Cancelled`; `after_send` leaves it unchanged.

- [ ] **Step 1: Write failing tests**: add `(AgentError::Cancelled, RetryClass::Cancelled)` to `retry_classification_table`; new `after_send_keeps_cancelled` asserting `AgentError::Cancelled.after_send() == AgentError::Cancelled`.
- [ ] **Step 2: Run** `cargo test -p postit-core error` — FAIL (no variant).
- [ ] **Step 3: Implement** the variants. In `postit-oauth` map `Cancelled` like the generic platform failure and give it the kind string `"cancelled"`.
- [ ] **Step 4: Run** `cargo test -p postit-core -p postit-oauth` — PASS.
- [ ] **Step 5: Commit** `feat(core): let agents report cancellation at a safe checkpoint`.

### Task 2: `postit-agent-kit` crate, HTTP handles, and error mapping

**Files:**
- Create: `server/crates/agent-kit/Cargo.toml`, `src/lib.rs`, `src/http.rs`, `src/error.rs`
- Modify: `server/Cargo.toml` (member + workspace dependency), `!ref/plans/01. vision and architecture.md` (dependency list: `agent-kit (core, http)`)
- Test: unit tests in `http.rs` and `error.rs`

**Interfaces:**
- Produces:
  - `pub fn standard(handles: &HttpHandles) -> Result<&reqwest::Client, AgentError>` (downcast `handles.default` to `postit_http::StandardClient`, return `.0`).
  - `pub fn public(handles: &HttpHandles) -> Result<&postit_http::PublicClient, AgentError>` (downcast `handles.user_hosts`).
  - Wrong handle type: `AgentError::Validation(vec![Violation::agent_rule(Message::new_static("agent_kit.wrong_http_handle", "The agent was given the wrong kind of HTTP client."))])`.
  - `pub enum Phase { BeforeSend, AfterSend }`
  - `pub type ErrorHook = dyn Fn(u16, Option<&str>) -> Option<AgentError> + Send + Sync;`
  - `pub fn classify(err: HttpError, phase: Phase, hook: Option<&ErrorHook>) -> AgentError`
  - Crate `testkit` feature declared (empty for now) in `Cargo.toml`.

Mapping (`hook` runs first for `Status`; `AfterSend` then applies `AgentError::after_send`):

| `HttpError` | `AgentError` |
|---|---|
| `Status { 401, .. }` | `Authentication { message: "http status 401" }` |
| `Status { 403, .. }` | `Authorization { missing_scopes: vec![] }` |
| `Status { 429, retry_after, .. }`, `RateLimited { retry_after }` | `RateLimited { retry_after }` |
| `Status { s, body_excerpt, .. }` otherwise | `Remote { status: Some(s), code: None, message: body_excerpt or "http status {s}" }` |
| `Transient(m)` | `Transport { message: m }` |
| `Permanent(m)` | `Validation([agent_rule("agent_kit.request_rejected", "The platform rejected the request.")])`, after a `tracing::warn!` of the redacted `m` |
| `OutcomeUnknown(m)` | `OutcomeUnknown { message: m }` |
| `AddressRejected { .. }` | `Validation([agent_rule("agent_kit.address_rejected", "The server address is not allowed.")])` |

- [ ] **Step 1: Write failing tests**: `downcasts_each_handle` (build `postit_http::agent_handles` from test settings; both accessors return `Ok`), `wrong_handle_is_permanent` (swap the two handles; `standard(..)` errs with `retry_class() == Permanent`), `classify_table` (one case per row above, asserting the variant and `retry_class`), `hook_runs_first` (hook maps `(400, Some("token_revoked"))` to `Authentication`), `after_send_makes_5xx_ambiguous` (`Status{500}` with `AfterSend` → `RetryClass::Ambiguous`; with `BeforeSend` → `Retryable`), `after_send_keeps_429_retryable`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit` — FAIL.
- [ ] **Step 3: Implement** the crate, both modules, and the workspace wiring. `lib.rs` docs say what the kit owns, that agents depend on it, never on `oauth` or `media`, and the rate-limit rule from the spec: an agent needing a short-window limit owns one `postit_http::RateLimiters` built in its constructor and shares it with its clients through an `Arc` (no `AgentContext` change).
- [ ] **Step 4: Run** `cargo test -p postit-agent-kit` — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): add the agent kit with HTTP handle access and error classification`.

### Task 3: Polling and the chunked upload driver

**Files:**
- Create: `server/crates/agent-kit/src/poll.rs`, `src/upload.rs`
- Test: unit tests in each (`#[tokio::test(start_paused = true)]` for polling)

**Interfaces:**
- Consumes: `PublishProgress`, `ByteStream`, `AgentError::Cancelled` (Task 1).
- Produces:
  - `pub enum Poll<T> { Ready(T), TimedOut }`
  - `pub async fn poll_until<T, F, Fut>(interval: Duration, timeout: Duration, progress: Option<&dyn PublishProgress>, step: F) -> Result<Poll<T>, AgentError> where F: FnMut() -> Fut, Fut: Future<Output = Result<Option<T>, AgentError>>`: runs `step` at once, then after each `interval` until `timeout` has elapsed; a step error returns at once; with `progress`, `cancel_requested()` before each sleep returns `Err(Cancelled)`.
  - `pub struct Chunk { pub offset: u64, pub bytes: Bytes, pub last: bool }`
  - `pub async fn upload_chunks<F, Fut>(stream: ByteStream, chunk_size: usize, resume_from: u64, progress: &dyn PublishProgress, send: F) -> Result<u64, AgentError> where F: FnMut(Chunk) -> Fut, Fut: Future<Output = Result<(), AgentError>>`: returns total length. Holds at most two chunks (one look-ahead to set `last`). Bytes below `resume_from` are discarded as they arrive. `cancel_requested()` before each `send` returns `Err(Cancelled)`. Stream read errors → `Transport`. `chunk_size == 0`, an empty stream, or `resume_from >= length` (for a non-empty stream) → `Validation` with `agent_kit.invalid_chunk_size`, `agent_kit.media_empty`, `agent_kit.resume_offset_beyond_end`, and `send` never called.

- [ ] **Step 1: Write failing tests**: `poll_ready_on_third_step` (returns `Ready`, elapsed virtual time = 2 intervals), `poll_times_out` (`TimedOut` after ≥ timeout), `poll_runs_once_when_timeout_below_interval` (Review Focus 5), `poll_cancel_stops`, `poll_step_error_returns`; `upload_exact_chunks` (10 bytes, size 4 → offsets 0,4,8, lengths 4,4,2, only the last `last == true`, returns 10), `upload_unaligned_source_pieces` (source yields 3-byte pieces; output chunks still 4,4,2), `upload_resumes` (`resume_from = 4` → offsets 4,8), `upload_rejects_empty`, `upload_rejects_offset_past_end` (Review Focus 1), `upload_cancel_between_chunks` (cancel flips after first send → `Cancelled`, one send), `upload_bounded_memory` (lazy 64 MiB source of 1 MiB pieces, chunk 1 MiB; a counter on the source asserts bytes pulled ≤ `offset + 2 × chunk_size` at every `send`).
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit poll upload` — FAIL.
- [ ] **Step 3: Implement** both modules (re-chunk with one `BytesMut`; `tokio::time::{sleep, Instant}` for polling).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): add status polling and the chunked upload driver`.

### Task 4: Declaration helpers

**Files:**
- Create: `server/crates/agent-kit/src/declare.rs`
- Test: unit tests in `declare.rs`

**Interfaces:**
- Produces:
  - `pub fn text_rules(measure: TextMeasure, max: u32, required: bool) -> TextRules`
  - `pub fn text_format(measure: TextMeasure, max: u32) -> FormatSpec` (`FormatId::Text`, required text, `MediaRules::none()`, `selected_by: None`)
  - `pub fn media_format(id: FormatId, text: TextRules, media: MediaRules) -> FormatSpec`
  - `pub fn schema(platform: &PlatformId, json: &str) -> Result<OptionsSchema, DeclareError>`; `#[derive(thiserror::Error)] pub struct DeclareError { pub platform: PlatformId, pub source: SchemaError }` displaying "schema for platform {platform} is outside the subset: {source}".
  - `pub fn message(platform: &PlatformId, key: &str, en: &str) -> Message` (key `"{platform}.{key}"`, owned `Cow`s).
  - Re-exports at the crate root: `OAuthProviderSpec`, `Pkce`, `ClientAuth`, `RefreshPolicy` from core (the builder already lives there).

- [ ] **Step 1: Write failing tests**: `text_format_shape`, `schema_names_platform_on_error` (a schema with `"pattern"` fails and the error text contains the platform ID), `message_prefixes_key` (`message(&mock, "outcome.label", "Outcome").key == "mock.outcome.label"`).
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): add format, schema, and message declaration helpers`.

### Task 5: Testkit fakes, test contexts, and wiremock fixtures

**Files:**
- Create: `server/crates/agent-kit/src/testkit/mod.rs`, `fakes.rs`, `context.rs`, `oauth_provider.rs`, `responders.rs`
- Modify: `server/crates/agent-kit/Cargo.toml` (`testkit = ["dep:wiremock", "postit-http/testkit", ...]`, optional deps), `src/lib.rs` (`#[cfg(feature = "testkit")] pub mod testkit;`)
- Test: `server/crates/agent-kit/tests/testkit.rs`

**Interfaces:**
- Produces (all under `postit_agent_kit::testkit`):
  - `MemoryMediaStore::new() -> Self`, `insert(&self, id: MediaAssetId, bytes: Bytes)`; `MediaStore::open` streams 64 KiB pieces, missing id → `MediaError::NotFound`.
  - `MemoryMediaHost::new() -> Self`; `public_url` → `https://media.test/{id}?ttl={secs}`; `issued(&self) -> Vec<(MediaAssetId, Duration)>`.
  - `MemoryCredentialStore::new(credentials: Credentials) -> Self`, `MemoryCredentialStore::empty() -> Self` (`credentials()` → `Authentication`), `refreshed(&self) -> Vec<OAuthTokens>`.
  - `FakeLinkSigner` with links under `https://links.test/`.
  - `TestContext::new() -> TestContext` with pub fields `clock: Arc<TestClock>`, `media_store: Arc<MemoryMediaStore>`, `media_host: Arc<MemoryMediaHost>`, `credentials: Arc<MemoryCredentialStore>`, `http: HttpHandles` (standard client from default `HttpSettings`; public client with `AddressPolicy { allow_private: true }` so wiremock on loopback works); `with_credentials(self, Credentials) -> Self`; `agent_context(&self, config: Value) -> AgentContext`; `check_context(&self, config: Value) -> CredentialCheckContext`; `event_context(&self, config: Value) -> EventContext`.
  - `MockOAuthProvider::start() -> impl Future<Output = MockOAuthProvider>`; `base_url() -> Url`; `spec() -> OAuthProviderSpec` (authorize `{base}/authorize`, token `{base}/token`, revoke `{base}/revoke`, scopes `["read","write"]`); `set_rotation(&self, rotates: bool)`; `fail_next(&self, failure: OAuthFailure)` with `OAuthFailure::{InvalidGrant, ExpiredRefreshToken, ServerError}`; `requests(&self) -> Vec<wiremock::Request>`; `revoked(&self) -> Vec<String>` (token hashes, never values). Token endpoint serves `grant_type=authorization_code` and `refresh_token`, issuing `access-{n}`/`refresh-{n}` with `expires_in: 3600`; error bodies are OAuth-standard JSON without echoing request values.
  - `responders::{rate_limited(retry_after: Option<u64>), server_error(), malformed_json(), dropped() -> ResponseTemplate}` (`dropped` delays past the test client's timeout; document the timeout it assumes).

- [ ] **Step 1: Write failing tests**: `media_store_streams_inserted_bytes` (200 KiB round-trips in ≥ 4 pieces), `credential_store_records_refresh`, `oauth_code_exchange_and_refresh` (raw `reqwest` POSTs; rotation off keeps `refresh-1`, on issues `refresh-2`), `oauth_scripted_invalid_grant` (400 `{"error":"invalid_grant"}` once, then success), `oauth_revoke_records_hash_only`, `context_public_client_reaches_loopback` (GET to a wiremock server through `public(&ctx.http)` and `postit_http::execute_public` succeeds).
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit --features testkit --test testkit` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(agent-kit): add testkit fakes, test contexts, and a mock OAuth provider`.

### Task 6: Conformance suite, proven on a test HTTP agent

**Files:**
- Create: `server/crates/agent-kit/src/testkit/conformance/mod.rs` (types, runner, report), `checks.rs` (one `async fn` per check)
- Create: `server/crates/agent-kit/tests/support/echo.rs` (test-only HTTP agent `EchoAgent`), `tests/support/mod.rs`, `tests/conformance_http.rs`
- Test: `tests/conformance_http.rs`

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces:
  - `pub enum Fault { Unauthorized, RateLimited(Option<Duration>), ServerError, AcceptedThenLost, LostBeforeAccept, Malformed }`
  - `#[async_trait] pub trait Faults: Send + Sync { async fn arm(&self, fault: Fault); async fn clear(&self); fn remote_calls(&self) -> usize; fn mutating_calls(&self) -> usize; }`
  - `pub struct CredentialSamples { pub valid: CredentialFields, pub invalid: CredentialFields }`
  - `pub struct ConformanceFixture { pub agent: Arc<dyn Agent>, pub context: Arc<dyn Fn() -> AgentContext + Send + Sync>, pub check_context: CredentialCheckContext, pub account: AccountRef, pub samples: Vec<Variant>, pub credentials: Option<CredentialSamples>, pub recipients: Vec<DirectRecipient>, pub secrets: Vec<String>, pub faults: Arc<dyn Faults> }`
  - `pub async fn run_conformance(fixture: ConformanceFixture) -> ConformanceReport`
  - `pub struct ConformanceReport { pub failures: Vec<CheckFailure> }`, `pub struct CheckFailure { pub check: &'static str, pub detail: String }`, `impl ConformanceReport { pub fn assert_passed(&self) }` (panics listing every failure).
  - Check names (exact strings): `descriptor`, `schema_subset`, `messages`, `capabilities`, `dry_run`, `validate`, `error_mapping`, `idempotency`, `cancellation`, `credentials`, `direct`, `redaction`. Each runs even when an earlier one failed; `faults.clear()` runs between checks.
  - Behavior of each check is the spec's numbered list ("Checks", 1–11). `error_mapping` expectations: `Unauthorized` → `NeedsReconnect`; `RateLimited(Some(d))` → `RetryAfter(d)`; `RateLimited(None)` → `Retryable`; `ServerError`, `AcceptedThenLost`, `LostBeforeAccept`, `Malformed` → `Ambiguous` (when `mutating_calls() > 0`). `dry_run` asserts `remote_calls()` unchanged and no `save_container_ref`. `direct` and `credentials` are skipped (not failed) when the agent is not direct or not a credentials agent. `redaction` installs `postit_http::testkit::LogCapture` for the whole run and calls `assert_no_leak` semantics over every error/outcome rendering and the captured log.
  - `EchoAgent` (test-only, platform `echo`): config `{"base_url": "<wiremock>"}`; credentials schema with secret `token`; `verify_credentials` GETs `/me` with bearer; `publish` uploads each media item first with `upload_chunks` (`POST /uploads` returns an ID it saves with `save_container_ref`, then `PUT /uploads/{id}` per chunk with a `Content-Range` header, chunk size 64 KiB) and then POSTs `/posts` with `Idempotency-Key`; the fixture includes one image sample so the upload path runs; `reconcile` GETs `/posts?key=`; uses `standard`, `classify` with the right `Phase`, and checks `cancel_requested` before its first POST (the driver then checks between chunks). `EchoFaults` implements `Faults` with wiremock mounts at higher priority than the happy-path mocks (`AcceptedThenLost`: record the key as created, then `responders::dropped()`).

- [ ] **Step 1: Write failing test** `echo_agent_passes_conformance`: build `EchoAgent` against a wiremock server, run the suite, `report.assert_passed()`.
- [ ] **Step 2: Run** `cargo test -p postit-agent-kit --features testkit --test conformance_http` — FAIL.
- [ ] **Step 3: Implement** the suite and `EchoAgent`.
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Write failing negative tests** in the same file, each building `EchoAgent` with one defect flag and asserting the report contains exactly that check: `catches_capability_mismatch` (declares `Commenter`, exposes none → `capabilities`), `catches_dry_run_call` (dry run POSTs → `dry_run`), `catches_secret_in_error` (error message includes the token → `redaction`), `catches_missing_english` (a schema label with empty `en` → `messages`), `catches_retryable_after_send` (skips `classify` phase → `error_mapping`).
- [ ] **Step 6: Run** — PASS after adding the defect flags to `EchoAgent`.
- [ ] **Step 7: Commit** `feat(agent-kit): add the conformance suite and prove it on an HTTP agent`.

### Task 7: Mock agent: descriptor, platform state, credentials, and publishing

**Files:**
- Create: `server/crates/agents/mock/Cargo.toml` (features `testkit = ["postit-agent-kit/testkit"]`), `src/lib.rs` (template docs), `src/descriptor.rs`, `src/platform.rs`, `src/credentials.rs`, `src/client.rs`, `src/script.rs`, `src/options.rs`
- Modify: `server/Cargo.toml` (member + workspace dependency)
- Test: `server/crates/agents/mock/tests/publish.rs`, `tests/credentials.rs`

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces:
  - `pub struct MockAgent` implementing `Agent`; `MockAgent::broadcast() -> Self` (`mock`), `MockAgent::direct() -> Self` (`mock_direct`), `MockAgent::oauth(provider: &Url) -> Self` (`mock_oauth`, `ConnectMethod::OAuth` with the Task 5 URL layout, `verify_credentials` → `Unsupported`), and `MockAgent::with_platform(self, id: PlatformId) -> Self` (for tests that need several platforms; keys stay `mock.` and are not run through conformance).
  - `pub fn script(&self) -> MockScript` (cloneable handle to the agent's shared state).
  - Descriptor (`mock`): `Broadcast`, credentials schema `account_name` (plain, required) + `token` (secret, required); formats `Text` (graphemes, 500), `Image` (1–4 images), `Video` (one video); capabilities all except `DirectSender`; native schedule window 600 s to 30 days; one quota bucket `posts` (`Account`, `Day`, limit 50, `UTC`, publish cost 1); options schema with `outcome` enum (`published` default, `processing`, `retryable_failure`, `permanent_failure`, `outcome_unknown_landed`, `outcome_unknown_lost`, `needs_reconnect`, `rate_limited`, `quota_exhausted`) and `comments` integer 0–20 default 0. Labels via `declare::message`.
  - `mock_direct`: `Direct`, same credentials, an email-capable text format, capabilities `{DirectSender}`.
  - `MockPlatform` (in `platform.rs`, `Arc<Mutex<…>>` shared by an agent's clients): posts by remote ID (`mock-post-{n}`), idempotency key → receipt, comments, scheduled posts, sends, call counters.
  - Credentials: token trimmed and ASCII-lowercased: `invalid` → `InvalidCredentials { field: Some("token") }`, `rate_limited` → `RateLimited { retry_after: Some(30 s) }`, `expired` connects but `account_info` reports `NeedsReconnect`, else success. `account_name` split on `,`, trimmed, empties dropped, de-duplicated in first-seen order; each name is remote ID `mock-account-{name}`.
  - Publish (live): the variant's `outcome` option decides (`published` → `Published` with `remote_url` `https://mock.test/posts/{id}`; `processing` → `Processing`, `status` then `Published`; `retryable_failure` → `Remote { 503 }` before send; `permanent_failure` → `Remote { 400 }`; `outcome_unknown_landed` → post stored, `OutcomeUnknown`; `outcome_unknown_lost` → nothing stored, `OutcomeUnknown`; `needs_reconnect` → `Authentication`; `rate_limited` → `RateLimited(30 s)`; `quota_exhausted` → `Quota { bucket: "posts" }`). A known idempotency key returns the stored receipt. `native_publish_at` stores a scheduled post with `remote_publish_at`. `cancel_requested` before storing → `Cancelled`. Dry run → `Simulated` with one planned step per action. `reconcile` → `Found` if the key is stored, else `Absent`.
  - `MockScript` methods (used by Task 9's migrated tests and by `Faults`): `set_verify(&self, VerifyScript)` with `VerifyScript::{Accounts(Vec<DiscoveredAccount>), Invalid(Option<String>), RateLimited, Fail, ByFields(Decide)}` and `pub type Decide = Arc<dyn Fn(&CredentialFields) -> (Duration, VerifyScript) + Send + Sync>`; `set_account_info(&self, InfoScript)` with `InfoScript::{Ok, AuthFailed}`; `revoke_calls(&self) -> usize`; `fail_revoke(&self, fail: bool)`; `seen_remote_ids(&self) -> Vec<String>`; `arm(&self, Fault)`/`clear(&self)` (testkit feature: `MockScript` implements `Faults`); `remote_calls`/`mutating_calls`. A script override wins over token keywords and options.

- [ ] **Step 1: Write failing tests** (`tests/credentials.rs`): `keywords_pick_results` (each keyword), `keywords_ignore_case_and_space` (Review Focus 2), `account_names_trimmed_deduped` (Review Focus 3), `expired_token_reports_needs_reconnect`, `script_overrides_keywords`, `by_fields_delays` (paused time; `ByFields` delay honored).
- [ ] **Step 2: Write failing tests** (`tests/publish.rs`): one test per `outcome` value asserting the outcome/error and `retry_class`; `processing_then_status_published`; `same_key_same_receipt`; `concurrent_same_key_one_post` (Review Focus 4: two clients, `tokio::join!`, one stored post, equal receipts); `reconcile_landed_found` / `reconcile_lost_absent`; `native_schedule_stores_publish_at`; `cancel_before_store`; `dry_run_stores_nothing`.
- [ ] **Step 3: Run** `cargo test -p postit-agent-mock --features testkit` — FAIL.
- [ ] **Step 4: Implement.** `lib.rs` docs are the template walkthrough: descriptor, client, one module per capability, `Faults` for conformance, and the `postit-server` registration line.
- [ ] **Step 5: Run** — PASS.
- [ ] **Step 6: Commit** `feat(agent-mock): add the mock agent with scripted credentials and publishing`.

### Task 8: Mock capabilities and conformance in both modes

**Files:**
- Create: `server/crates/agents/mock/src/capabilities/mod.rs`, `comment.rs`, `inbox.rs`, `metrics.rs`, `schedule.rs`, `direct.rs`
- Modify: `src/client.rs` (capability accessors)
- Test: `server/crates/agents/mock/tests/capabilities.rs`, `tests/conformance.rs`

**Interfaces:**
- Consumes: Task 6 (`run_conformance`), Task 7.
- Produces:
  - `Commenter`: `resolve_target` accepts `https://mock.test/posts/{id}` for a stored post (else `Validation` with `mock.target_unknown`); `comment` stores a reply and follows the same `outcome` scripting and dedup; `reconcile_comment` like `reconcile`.
  - `InboxReader::fetch_since`: comments seeded by the `comments` option (`mock-comment-{n}`, authors `@fan{n}`), ordered by creation, cursor = last comment ID; `MockScript::push_comment(&self, remote_post_id: &str, text: &str)` adds one on demand.
  - `MetricsReader`: deterministic numbers (builder's formula, documented), `account_metrics` sums the account's posts.
  - `NativeScheduler`: `reschedule` updates `remote_publish_at` (outside the window → `Validation`); `delete` removes it; `status` reports `Published` once `ctx.clock` passes `remote_publish_at`.
  - `DirectSender` (`mock_direct`): one result per recipient in order; address containing `soft-bounce` → `Bounced { permanent: false }`, `bounce` → `Bounced { permanent: true }`, `fail` → `Failed { retryable: true }`, else `Sent { remote_id: Some("mock-send-{send_id}") }`; reasons never include the address.
  - `revoke` counts calls and fails when `fail_revoke(true)`.

- [ ] **Step 1: Write failing tests** (`tests/capabilities.rs`): `comment_on_resolved_target`, `unknown_target_rejected`, `inbox_returns_seeded_comments_and_cursor`, `push_comment_appears_after_cursor`, `metrics_deterministic`, `reschedule_and_delete`, `reschedule_outside_window_rejected`, `scheduled_post_published_after_clock_passes` (advance `TestClock`), `direct_outcomes_by_address` (all four, results in input order, `send_id`s match), `revoke_counts_and_fails`.
- [ ] **Step 2: Write failing tests** (`tests/conformance.rs`): `mock_broadcast_passes` and `mock_direct_passes`, each building a `ConformanceFixture` from `TestContext`, `MockAgent::script()` as `Faults`, samples for every declared format, valid (`account_name = "shop"`, `token = "s3cret-token"`) and invalid (`token = "invalid"`) credentials, three recipients for direct, secrets `["s3cret-token"]`; `report.assert_passed()`.
- [ ] **Step 3: Run** `cargo test -p postit-agent-mock --features testkit` — FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** — PASS.
- [ ] **Step 6: Commit** `feat(agent-mock): add every capability and pass conformance in both modes`.

### Task 9: Replace B3's test agent with the mock

**Files:**
- Modify: `server/crates/oauth/src/testkit.rs` (delete `TestCredentialAgent`, `Script`, `VerifyScript`, `InfoScript`, `Decide`, `credentials_schema`; keep `NoMedia`, `NoLinks`, `test_vault`, `test_accounts_settings`, `account_service`), `server/crates/oauth/Cargo.toml` and `server/crates/api/Cargo.toml` (dev-dependency `postit-agent-mock = { workspace = true, features = ["testkit"] }`), `server/crates/oauth/tests/{common/mod.rs,catalog.rs,connect.rs,disconnect.rs,import.rs,leak.rs,manage.rs,store.rs}`, `server/crates/api/tests/accounts.rs`
- Test: the moved tests

**Interfaces:**
- Consumes: `MockAgent::{broadcast, oauth, with_platform, script}`, `MockScript` (Task 7). Field names change from `server_url`/`token` to `account_name`/`token`; CSV fixtures change their header accordingly.

- [ ] **Step 1: Switch the tests** to the mock, one file at a time, keeping every assertion's meaning (no test deleted; count before and after with `cargo test -p postit-oauth -p postit-api -- --list | wc -l` and record both numbers in the commit body).
- [ ] **Step 2: Delete** the test agent code from `testkit.rs`.
- [ ] **Step 3: Run** `cargo test -p postit-oauth -p postit-api` (DB-backed; set `DATABASE_URL` from the development vault) — PASS, same test count.
- [ ] **Step 4: Commit** `refactor(oauth): run the account tests against the mock agent`.

### Task 10: Server registration, production guard, and builds

**Files:**
- Modify: `server/crates/server/Cargo.toml` (`[features] mock-agent = ["dep:postit-agent-mock"]`, optional dependency, dev-dependency on itself with `features = ["mock-agent"]`), `src/compose.rs` (`agent_registry`), `src/lib.rs` (guard + log), `server/.cargo/config.toml` (alias), `docker/server.Dockerfile`, `docker/docker-compose.development.yml`, `.github/workflows/ci.yml` (docker job `build-args`), `docker/ci/smoke.sh`
- Test: `server/crates/server/tests/agents.rs` (new)

**Interfaces:**
- Produces:
  - `compose::agent_registry()` registers `MockAgent::broadcast()` and `MockAgent::direct()` under `#[cfg(feature = "mock-agent")]`, one line each.
  - `pub fn refuse_mock_in_production(environment: Environment) -> anyhow::Result<()>` in `compose.rs`: errs with "the mock agent is compiled in; production builds must not enable the mock-agent feature" when `cfg!(feature = "mock-agent")` and `environment == Environment::Production`. Called at startup before serving.
  - Startup `tracing::info!(platforms = ?ids, "registered agents")` once after building the registry.
  - Alias `dev = "run --features mock-agent --"`.
  - Dockerfile `ARG CARGO_FEATURES=""` passed as `--features "$CARGO_FEATURES"` to both `cargo chef cook` and `cargo build` (keep unification identical); development compose `build.args: CARGO_FEATURES: mock-agent`; CI docker job `build-args: CARGO_FEATURES=mock-agent`; production and qa compose unchanged.
  - `smoke.sh` asserts the API container's log contains `registered agents` and `mock`.

- [ ] **Step 1: Write failing tests**: `registry_has_both_mocks` (`agent_registry()` contains `mock` and `mock_direct`), `production_refuses_mock` (`refuse_mock_in_production(Production)` errs; `Development` and `Qa` ok).
- [ ] **Step 2: Run** `cargo test -p postit-server --test agents` — FAIL.
- [ ] **Step 3: Implement** the feature, registry lines, guard, and log.
- [ ] **Step 4: Run** `cargo test -p postit-server` — PASS. Run `cargo build -p postit-server` (no feature) and confirm `cargo tree -p postit-server -e normal | grep agent-mock` prints nothing.
- [ ] **Step 5: Update builds**: alias, Dockerfile, development compose, CI build args, smoke assertion. Verify with `./stack.ps1` per the `stack` skill (build the development image, start the app profile, check the log line), and `bash -n docker/ci/smoke.sh`.
- [ ] **Step 6: Commit** `feat(server): register the mock agent in development and CI builds` (Docker and CI changes may be a second commit, `build(docker): enable the mock agent in development and CI images`).

### Task 11: Documentation and full gates

**Files:**
- Modify: `CLAUDE.md` ("Project state": B5 built, crate list gains `agent-kit` and `agents/mock`, replace the "platform not found until B5" sentence with how to run with the mock: `cargo dev` natively, the development image, and that production refuses it), `docs/configuration.md` (the `mock-agent` feature, where it is on, the guard), `docs/superpowers/plans/2026-10-23-b3-credential-connection-deferred.md` only if an item there is now resolved (none expected)

- [ ] **Step 1: Write the docs.**
- [ ] **Step 2: Run every gate** from `server/`: `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo check --workspace --all-targets`; `cargo test --workspace` (background, log to a file, poll for the exit line); `cargo deny check`; then `cargo xtask openapi --check` from PowerShell. Expected: all pass, OpenAPI unchanged (B5 adds no routes).
- [ ] **Step 3: Fix** anything found, minor issues included. Anything truly blocked goes in `docs/superpowers/plans/2026-10-24-b5-agent-kit-mock-deferred.md` (issue, why not now, trigger), committed with the branch.
- [ ] **Step 4: Commit** `docs: describe the agent kit and the mock agent`.
