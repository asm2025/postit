# B5 agent kit and mock agent: design

Plan 03 phase B5. Roadmap: `!ref/plans/03. publishing roadmap.md`, "Phase B5 — Agent kit and mock agent". Needs B1 (HTTP resilience) and B3 (credential connection), both built.

## Intent

B5 is the phase that unlocks the most later work. Today no build registers an agent, so the B3 account routes answer "platform not found", and nothing downstream (B7 OAuth, the C agents, the D1 engine, D3 routes, D4/D6 screens, every F platform) has a platform to run against. B5 delivers two things:

1. **`postit-agent-kit`**: the shared machinery every agent uses (typed HTTP access, `HttpError` to `AgentError` mapping, polling, chunked upload, declaration helpers) and, behind the `testkit` feature, the one conformance suite every agent must pass plus shared test fixtures.
2. **`postit-agent-mock`**: an in-memory reference agent with every capability and scriptable failures. Development and CI builds register it, so the engine, REST surface, and client UIs can be built and tested before any real agent exists. It is the documented template for new agents.

## Goals

- A new agent needs only its protocol code, its declarations, and one registration line in `postit-server` (roadmap exit criterion).
- One generic conformance suite, `run_conformance(fixture)`, that any agent passes, covering: the dry-run contract, error mapping, idempotency, cancellation, redaction, the schema subset, message keys with English defaults, and declared capabilities matching exposed traits (plan 01, "One conformance suite").
- The mock agent passes the suite in broadcast and direct modes.
- A running development server lists the mock platforms and the B3 connect, list, check, and disconnect flows work against them end to end.
- The mock can be driven without code: behavior comes from the pasted credentials, the post's publish options, and recipient addresses, so D3 HTTP tests and D4/D6 Playwright and integration tests can script failures through the public API.

## Non-goals

- The OAuth flow itself (B7). B5 provides the mock OAuth provider fixture and an OAuth-mode mock agent constructor for B7's tests; registering an OAuth mock in a running server needs a reachable provider and is B7's decision.
- The publishing engine, delivery state machine wiring, quotas, and push (D1). B5 only defines how an agent reports cancellation.
- Real platform agents (C2 onward).
- The agent authoring guide document (G4, built from the mock). B5 documents the template in the mock crate's docs.
- Media storage (B4). The testkit has in-memory media fakes; the server keeps its "unavailable" media stand-ins.

## Anti-goals

- Platform API types or platform-specific code in `postit-agent-kit`, `postit-oauth`, or `postit-core`.
- A mock that can ship to production. It must not compile into the default build and a production config must refuse to start with it.
- Any secret (pasted token, OAuth token, signed URL) in a log, error, outcome, receipt, or `Debug` output.
- A conformance suite that only the mock can pass (it must be driven through a fault interface an HTTP agent can implement with wiremock).

## Constraints

- Dependency order (plan 01): `agent-kit` is level 8 and depends on `core` and `http` only. Agent crates (level 9) depend on `core`, `http`, and `agent-kit`, and never on `oauth` or `media`. Dev-dependencies from level-8 crates on the mock agent are allowed (no cycle: the mock does not depend on them).
- Workspace lints (`unwrap_used`, `expect_used`, `panic` denied outside tests), `secrecy` types for secrets, workspace-level dependency versions.
- Platform IDs match `[a-z][a-z0-9_]*`.
- `cargo fmt --check`, clippy with `-D warnings` and `--all-features`, `cargo check`, `cargo test --workspace`, `cargo deny`, and `cargo xtask openapi --check` stay green.
- The server registers each agent behind a Cargo feature named after its platform ID; the mock's feature is `mock-agent`, off by default, on in development and CI builds (plan 01).

## Design

### Crate layout

- `server/crates/agent-kit/` → `postit-agent-kit`, features `testkit` (pulls `wiremock`, `postit-http/testkit`, `tokio` test utilities).
- `server/crates/agents/mock/` → `postit-agent-mock`.
- Both are workspace members and workspace dependencies. Plan 01's crate map already lists both paths.

### Contract change in core: cancellation

The contract has no way for an agent to say "I stopped at a safe checkpoint because the user cancelled" (roadmap, "Rescheduling, cancelling, and retrying"). **Call:** add `AgentError::Cancelled` and `RetryClass::Cancelled` in `postit-core`. `retry_class` maps `Cancelled` to `Cancelled`; `after_send` leaves it unchanged. D1 maps it to the delivery's `Cancel` transition. `postit-oauth`'s exhaustive matches gain an arm (`Cancelled` cannot happen during connect; it maps to the generic platform failure). Agents never return `Cancelled` after their final create or publish call.

### `postit-agent-kit` (production code)

- **`http`**: typed access to the handles in `HttpHandles`. `standard(&HttpHandles) -> Result<&reqwest::Client, AgentError>` and `public(&HttpHandles) -> Result<&PublicClient, AgentError>` downcast through `HttpHandle::as_any` to `postit_http::StandardClient` and `PublicClient`. A wrong handle type is a programming error reported as a permanent `AgentError` (not a panic).
- **`error`**: `HttpError` → `AgentError` classification, the one place this mapping lives (B1 design deferred it here). Defaults:
  - 401 → `Authentication`; 403 → `Authorization { missing_scopes: [] }`; 429 or `RateLimited` → `RateLimited { retry_after }`; other 4xx and `Permanent` → `Remote` with its status (permanent); 5xx and `Transient` → `Remote`/`Transport` (retryable); `OutcomeUnknown` → `OutcomeUnknown`; `AddressRejected` → a permanent error naming no path (a `Validation` with an agent-rule violation, code chosen by the builder).
  - A `Phase` argument (`BeforeSend`, `AfterSend`): after-send results pass through `AgentError::after_send`.
  - An agent can supply a hook `Fn(status, body_excerpt) -> Option<AgentError>` that runs first, for platform error codes in the body (for example "token revoked" on a 400). The body excerpt is already redacted by `postit-http`.
  - Messages carried into `AgentError` are redacted text only.
- **`poll`**: `poll_until(interval, timeout, step)` for processing status checks. `step` is an async closure returning `Result<Option<T>, AgentError>`; `None` means "not yet". Uses `tokio::time` so tests run with paused time. Timeout returns a distinct result (not an error) so the caller decides (`Processing` receipt versus `OutcomeUnknown`). Checks `PublishProgress::cancel_requested` between polls when given one.
- **`upload`**: the chunked upload driver. Takes a `ByteStream`, a chunk size, a resume offset, the `PublishProgress`, and an async per-chunk closure `(offset, Bytes, is_last) -> Result<(), AgentError>`. It re-chunks the stream into fixed-size chunks with one bounded buffer (memory stays at about one chunk regardless of file size), skips bytes below the resume offset without buffering them, and checks `cancel_requested` before each chunk, returning `AgentError::Cancelled`. Saving the container ref stays the agent's job (it knows when the platform issued one).
- **`declare`**: small helpers for descriptors: text-only and media `FormatSpec` constructors, `TextRules` presets for the common `TextMeasure`s, a schema-from-JSON helper that turns a subset violation into a startup error naming the platform, and `Message` helpers that prefix keys with the platform ID. The `OAuthProviderSpec` builder already exists in core (A1); the kit re-exports it rather than duplicating it.
- **Rate limiting (call):** an agent that needs a short-window rate limit owns one `postit_http::RateLimiters` built in its constructor and shares it across its clients through an `Arc`. Each agent is built once per process at startup, so this is process-wide without changing `AgentContext`. (B1's note said B5 would wire limiters into the context; owning them in the agent needs no core change and keeps buckets next to the code that knows them.)

### `postit-agent-kit` `testkit` feature

- **In-memory fakes:** `MemoryMediaStore` (asset ID → bytes, streamed in small chunks so drivers see a real stream), `MemoryMediaHost` (deterministic `https://media.test/...` URLs, records TTLs), `MemoryCredentialStore` (holds `Credentials`, records `store_refreshed`).
- **Context builders:** `TestContext` producing `AgentContext`, `CredentialCheckContext`, and `EventContext` with real `postit-http` clients (the public client using a policy that allows loopback, so wiremock works), a `TestClock`, the fakes above, and a fake `LinkSigner`.
- **Shared wiremock fixtures:** `MockOAuthProvider` (token exchange, refresh with optional rotation, revoke, scripted `invalid_grant` and expired refresh token, request recording, never echoing secrets in failures), plus responders for rate limiting (429 with `Retry-After` or a reset header), server errors, connection drops (wiremock delay past the client timeout), and malformed payloads.
- **Conformance suite:** `run_conformance(fixture: ConformanceFixture)` (async, returns a report and fails with every broken check listed, not just the first). The fixture is generic over how the platform is faked:
  - the agent (`Arc<dyn Agent>`), a context factory, one connected `AccountRef`, a valid sample `Variant` per declared format;
  - for credential agents: valid and invalid `CredentialFields`;
  - for direct agents: sample recipients;
  - the secret strings the run must never reveal;
  - a `Faults` implementation: `async fn arm(&self, fault: Fault)` and `fn remote_calls(&self) -> usize`. `Fault` is `Unauthorized`, `RateLimited(Duration)`, `ServerError`, `AcceptedThenLost` (the platform acted, the response was lost), `LostBeforeAccept`, and `Malformed`. An HTTP agent implements it with wiremock mounts; the mock implements it with its script.
- **Checks:**
  1. Descriptor validates and registers; config, options, credentials, and per-account publish-options schemas are inside the subset (serialize, re-parse, compare).
  2. Every `Message` reachable from the descriptor and schemas has a non-empty English default and a key prefixed `<platform>.`.
  3. Declared capabilities equal `exposed_capabilities(client)`.
  4. Dry run: `publish` and, when offered, `comment` in `DryRun` return `Simulated`, make zero remote calls, and never call `save_container_ref`.
  5. Validation: each sample variant passes `validate` with no violations.
  6. Error mapping, per fault, on a live publish: `Unauthorized` → `NeedsReconnect`; `RateLimited(d)` → `RetryAfter(d)` (or `Retryable` when no delay); `ServerError` and `LostBeforeAccept` after the request reached the platform → `Ambiguous`, never `Retryable` (before it reached the platform, `Retryable` or `Ambiguous`); `AcceptedThenLost` → `Ambiguous`; `Malformed` on a mutating call → `Ambiguous`.
  7. Idempotency and reconcile: after `AcceptedThenLost`, `reconcile` with the same key returns `Found` (or `Unresolvable` when the agent returns `Unsupported`); after `LostBeforeAccept`, `Absent` or `Unresolvable`; a second live publish with an already-published key returns the same receipt or an `Ambiguous` error, never a second post. The proof is the fixture's mutating-call count (no mutating call for the repeated key) or the first post's remote ID in the receipt; a receipt with no remote ID proves nothing by itself. A fixture for a platform that cannot deduplicate (a webhook with no lookup, plan 03 C4) sets `deduplicates: false`, and the repeat-key part is skipped (the check reports skipped when the rest passed).
  8. Cancellation: with `cancel_requested` true from the start, a live publish returns `Cancelled` and makes no mutating remote call.
  9. Credentials: valid fields discover at least one account; invalid fields give `InvalidCredentials`; `Unsupported` is accepted only for an OAuth agent.
  10. Direct mode: `send_batch` returns exactly one result per recipient with matching `send_id`s.
  11. Redaction: the fixture's secrets appear in no error `Display`/`Debug`, outcome, receipt, or captured log line (reusing `postit_http::testkit::LogCapture` and `assert_no_leak`).
- **Proof that HTTP agents can pass:** a small test-only HTTP agent in `agent-kit/tests/` (talks JSON to a wiremock server through the kit's `http`, `error`, and `upload` modules) passes the suite with a wiremock `Faults`.

### `postit-agent-mock`

- **Three agents from one crate**, sharing one implementation: `mock` (broadcast, credentials), `mock_direct` (direct, credentials), and an OAuth-mode constructor `MockAgent::oauth(provider: &Url)` for B7's tests (platform `mock_oauth`, spec pointing at the given `MockOAuthProvider`). Registration in a running server covers `mock` and `mock_direct` only.
- **State:** an in-memory `MockPlatform` shared by an agent's clients (`Arc<Mutex<…>>`): published posts by remote ID, idempotency keys seen, comments, scheduled posts, sends. State is per process and lost on restart; documented.
- **Every capability:** `Commenter`, `InboxReader` (returns comments seeded on the mock's posts), `MetricsReader` (deterministic numbers derived from the post), `NativeScheduler` (posts with `remote_publish_at`; `status` reports `Published` once the clock passes it), and `DirectSender` on `mock_direct`. The descriptor declares a native schedule window and a quota bucket so D1 has both to exercise.
- **Formats:** text plus image and video formats with media rules, so validation and format picking have something to bite on.
- **Scripting without code (call):**
  - Credentials schema: `account_name` (plain) and `token` (secret). The token picks the result: `invalid` gives `InvalidCredentials`, `rate_limited` gives `RateLimited`, `expired` connects but makes `account_info` report `NeedsReconnect`, anything else succeeds; `account_name` may list several comma-separated names to discover several accounts from one grant.
  - Publish options schema: an `outcome` enum (`published`, `processing`, `retryable_failure`, `permanent_failure`, `outcome_unknown_landed`, `outcome_unknown_lost`, `needs_reconnect`, `rate_limited`, `quota_exhausted`) with `published` as default, and a `comments` count seeding that many inbox comments on the new post.
  - Direct recipients: an address containing `bounce` bounces permanently, `soft-bounce` bounces temporarily, `fail` fails retryably; others are sent.
  - A Rust `MockScript` handle overrides any of these in unit tests and implements the conformance `Faults`, and can delay calls (CSV progress tests), count calls, and record revokes.
- **Template quality:** the crate's top-level docs walk a new agent author through descriptor, client, capabilities, faults for conformance, and the server registration line. Code is organized the way a real agent should be (descriptor module, client module, one module per capability).

### Server wiring

- `postit-server` feature `mock-agent` (off by default) enables the optional `postit-agent-mock` dependency; `compose::agent_registry()` registers `mock` and `mock_direct` under `#[cfg(feature = "mock-agent")]`, one line each.
- **Production guard (call):** startup refuses to run when the mock is compiled in and `environment = "production"`, with a clear error. Development and qa are allowed (qa is a test environment).
- **Builds that enable it:** a cargo alias in `server/.cargo/config.toml` for native development runs (for example `cargo dev` → `run --features mock-agent`), a `CARGO_FEATURES` build argument in `docker/server.Dockerfile` (default empty) set to `mock-agent` by the development compose file and the CI smoke compose, and the CI `test` job's server tests. The production compose leaves it empty.
- Startup logs the registered platform IDs once (`registered agents`). The CI smoke test (which has no signed-in user) checks that log line names `mock`; the connect flow against the mock is proven by `postit-api` route tests. `GET /platforms` is a D3 route and does not exist yet.

### Replacing B3's test agent (call)

`postit_oauth::testkit::TestCredentialAgent` was a stand-in "superseded by B5" (B3 design). Keeping both would be two owners of one concern. The `postit-oauth` and `postit-api` tests move to the mock agent (via `MockScript`, which covers every behavior the test agent had: scripted verify results, per-row delays from pasted values, `account_info` failures, revoke counting and failure). The test agent and its script types are then deleted from `postit-oauth`'s `testkit` module. The module and feature stay for the service-building helpers (`test_vault`, `test_accounts_settings`, `account_service`, the "no media" and "no links" stand-ins), since `postit-oauth` may not depend on `postit-agent-kit`; tests reach the mock through a dev-dependency on `postit-agent-mock` (feature `testkit`).

### Docs and housekeeping

- `CLAUDE.md` "Project state" and crate list gain `agent-kit` and `agents/mock`, and the note that B3 routes answer "platform not found" is replaced by how to run with the mock.
- Plan 01 dependency list: `agent-kit (core, http)`.
- `docs/configuration.md`: the `mock-agent` feature, where it is on, and the production guard.

## Testing

- Kit unit tests: handle downcasts (right and wrong type), the full error mapping table including the hook and after-send phase, `poll_until` with paused time (success, timeout, cancel), the upload driver (fixed chunk sizes, last-chunk flag, resume offset, cancel between chunks, bounded memory with a large generated stream), declaration helpers.
- Testkit tests: `MockOAuthProvider` exchange, refresh, rotation, revoke, scripted failures; the fakes.
- Conformance: the mock as `mock` and `mock_direct`, and the test-only HTTP agent, all passing; plus negative tests proving the suite catches a broken agent (capability declared but not exposed, a dry run that calls the platform, a secret in an error, a message without English text).
- Mock behavior tests: every scripted outcome through publish, comment, reconcile, status, inbox, metrics, native schedule, direct sends, and credentials.
- Moved B3 tests stay green on the mock; the leak harness tests still pass.
- Server: registry with the feature on lists both mocks; the production guard refuses to start; the smoke test above.

## Left to the builder

- Exact module names and function signatures inside the kit, as long as the behavior above holds.
- The `Validation` violation code used for a refused address.
- Mock metric formulas, default quota numbers, and the native schedule window values.
- How the conformance report is shaped, as long as it lists every failed check by name.
