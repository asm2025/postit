# B1: HTTP resilience and outbound address policy (design)

Date: 2026-10-19. Source: plan 03 (`docs/plans/03. publishing roadmap.md`), phase B1, plus the sections it links to: Connecting an account with credentials, Publishing (retry and `OutcomeUnknown`), Quotas, Media, and Notifications and push. Plan 01 covers the "one owner per concern" rule (`postit-http` is the only `reqwest` factory), the dependency order (`http` depends on `config` only), and the no-secret-leak rule. This document does not repeat the plan. It records intent, the decisions the plan left open, and what the builder decides.

The user skipped the question round. Every decision below is the designer's call unless the plan states it, and each call is marked.

## Intent

Every later outbound call goes through `postit-http`: agents (C, F), OAuth token exchange and refresh (B7), the drafter's LLM calls (D2), and Web Push (D1). B1 makes those calls safe to retry, polite to platform rate limits, unable to leak a secret into a log, and unable to reach a private network address that a user typed in. A retry mistake here posts twice on someone's real account. An address-policy mistake turns a Mastodon instance URL field into a server-side request forgery into the deployment's own network.

## Goals

- Everything under plan 03 phase B1, meeting its exit criteria:
  - Retry, timeout, rate limiting, redaction, error mapping, and the address policy for both HTTP and SMTP are verified without real network calls.
  - No crate other than `postit-http` constructs a `reqwest::Client`, and this is enforced by tooling, not only by review.
- Agent crates (B5 on) can describe their retry and rate-limit needs as data and get the behavior without writing loops.

## Non-goals

- No mapping from `HttpError` to `AgentError`. That belongs to `postit-agent-kit` (B5, "error classification"). B1 makes `HttpError` carry everything that mapping needs.
- No quota tables or cross-process limits. Those are B2's `platform_quota_usage` and `platform_app_quota_usage`. B1 limiters are per process, as the plan says.
- No SMTP client. B1 exposes the resolve-and-check function; the email agent (C2) connects `lettre` to the checked address.
- No Web Push encryption (D1) and no LLM client (D2).
- No changes to OIDC discovery or JWKS fetching beyond what the new error variant forces. Those talk to the configured issuer, which is operator config, not user input, and keep using the standard client.

## Anti-goals

- A non-idempotent request (a publish, a comment, an upload finalize) retried after its body may have reached the platform.
- A user-supplied host that resolves to a loopback, private, link-local, unique-local, or cloud-metadata address being contacted while the matching `allow_private_*` flag is off, including through DNS rebinding, an IP literal in the URL, an IPv4-mapped IPv6 address, a redirect, or an `HTTP_PROXY` environment variable.
- A signed media URL, webhook URL, push endpoint, bearer token, or OAuth code in a tracing span, an error message, or a test snapshot.
- Tests that sleep in real time or depend on the network.
- A hand-rolled rate limiter when `governor` is already a workspace dependency.

## Constraints

- Dependency order: `postit-http` depends on `postit-config` only. It must not depend on `postit-core`. It knows nothing about agents, deliveries, or platforms; agent-specific behavior arrives as data (patterns, header names, bucket definitions).
- Prefer maintained libraries: `governor` (already in the workspace, used by `postit-api`), `ipnet` (already in the workspace), `rand` (workspace) for jitter, `chrono` (workspace) for HTTP-date parsing. No new dependency with an Arabic or Arabic-derived name.
- `reqwest` keeps its current feature set (rustls, json, gzip, multipart, stream). No `blocking`, `cookies`, or `socks`.
- Existing public functions (`build_client`, `execute_traced`, `classify_status`, `retry_after_from_headers`, the redaction functions) keep working. Callers in `postit-identity` and `postit-server` change only where a new enum variant needs a match arm.

## Decisions

### Clients: one factory, two kinds

- `build_client(&HttpSettings)` stays as the **trusted client** for operator-configured hosts (the OIDC issuer, platform APIs with fixed hosts such as `api.telegram.org`). It keeps honoring proxy environment variables (designer's call): an operator may need an egress proxy for fixed hosts.
- New `build_public_client(&HttpSettings, AddressPolicy) -> Result<reqwest::Client, HttpError>` builds the **public client** for user-supplied hosts (plan's "client variant"). It differs from the trusted client in four ways:
  1. A custom `reqwest::dns::Resolve` implementation resolves through the system resolver (`tokio::net::lookup_host`), drops every address the policy forbids, and fails with a policy error when none remain. reqwest connects only to the addresses the resolver returns, so the check happens on the address actually dialed, and a DNS answer that changes between "check" and "connect" cannot slip through (no separate check step to race).
  2. `redirect(Policy::none())`. A 3xx is returned to the caller as a response, never followed.
  3. `.no_proxy()`, so an `HTTP_PROXY`/`HTTPS_PROXY` environment variable cannot route around the resolver (designer's call).
  4. reqwest does not call the resolver for IP-literal hosts. So every request sent through the public client goes through `execute_public` (below), which rejects a URL whose host is an IP literal that the policy forbids before sending. A test sends `http://127.0.0.1:<wiremock port>/` and `http://[::ffff:127.0.0.1]:<port>/` and asserts the policy error with zero requests received by the mock.
- **`AddressPolicy`** (designer's call on shape): `AddressPolicy { allow_private: bool }`, built by callers from the matching flag: `ai.allow_private_endpoints`, `push.allow_private_endpoints`, `email.allow_private_hosts`. Mastodon instances and Bluesky PDS hosts have no flag in the plan; agents use `AddressPolicy::strict()` (designer's call; a self-hoster with a LAN Mastodon is out of scope and noted in `docs/configuration.md`).
- **Forbidden ranges** (designer's call; one table in the crate, tested address by address): IPv4 `0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10` (carrier-grade NAT), `127.0.0.0/8`, `169.254.0.0/16` (includes cloud metadata `169.254.169.254`), `172.16.0.0/12`, `192.0.0.0/24`, `192.168.0.0/16`, `198.18.0.0/15`, `224.0.0.0/4` (multicast), `240.0.0.0/4` and broadcast; IPv6 `::/128`, `::1/128`, `fc00::/7` (unique local), `fe80::/10` (link local), `ff00::/8` (multicast), `64:ff9b::/96` (NAT64; its embedded IPv4 is checked instead), and any IPv4-mapped (`::ffff:0:0/96`) or IPv4-compatible address, which is checked as its IPv4 address. `allow_private = true` lifts the private, loopback, link-local, unique-local, and CGNAT ranges, but never unspecified (`0.0.0.0`, `::`), multicast, or broadcast.
- **SMTP.** `resolve_checked(host: &str, port: u16, policy: AddressPolicy) -> Result<Vec<SocketAddr>, HttpError>` exposes the same resolve-and-filter logic as a plain async function. The email agent (C2) connects `lettre` to one returned address and uses the original host name for TLS server-name verification. The resolver used by the public client and this function share one implementation, so HTTP and SMTP cannot drift.
- **Tooling enforcement of "no other crate builds a client"** (designer's call): add `backend/clippy.toml` with `disallowed-methods` for `reqwest::Client::new`, `reqwest::Client::builder`, and `reqwest::ClientBuilder::new`, each with a reason pointing at `postit-http`. `postit-http`'s two factory functions carry a narrowly scoped `#[allow(clippy::disallowed_methods)]` with a comment. The existing CI clippy gate then fails any other crate that builds a client.

### Errors

- `HttpError` gains:
  - `OutcomeUnknown(String)`: a non-idempotent request whose body may have reached the server and that then failed (timeout after send, connection reset after send, 5xx). Never retried by this crate.
  - `AddressRejected { host: String }`: the policy refused every address for `host`. The host name is user-supplied but not secret; the message carries the host, never the URL path or query.
  - `Status { status: u16, retry_after: Option<Duration>, body_excerpt: Option<String> }` (designer's call): a non-2xx response that the caller asked `postit-http` to classify, carrying the status and a redacted excerpt (first 512 bytes, after `redact_secrets`) so B5 can map platform error codes without re-reading the body. `Transient`, `RateLimited`, and `Permanent` stay for the existing callers. The builder may instead extend those variants with the status, as long as the B5 mapping gets the status, the retry delay, and a redacted excerpt.
- `classify_status` and the `From<reqwest::Error>` conversion stay the single classification point. A `reqwest` error that is neither a connect error, a timeout, nor a status error (body decode, builder) stays `Permanent`.
- `retry_after_from_headers` now also parses the HTTP-date form (`Wed, 21 Oct 2015 07:28:00 GMT`) relative to an injected "now", and a date in the past yields zero. It never returns more than a configured cap (`RetryPolicy::max_retry_after`, default 15 minutes, designer's call), so a hostile server cannot park a worker for a day; a longer wait surfaces to the caller as `RateLimited` with the full value for the engine to schedule.

### Retry

- **Shape.** `RetryPolicy { max_attempts: u32, base_delay: Duration, max_delay: Duration, max_retry_after: Duration, budget: Duration }`. `budget` is the cap on total time spent sleeping between attempts for one call (the plan's "retry budget"; designer's call on meaning). Defaults (designer's call): 3 attempts, 500 ms base, 30 s max delay, 15 min max retry-after, 60 s budget. Delivery-level retries (5 attempts, then `DeadLetter`) are the publishing engine's, not this crate's; this policy covers retries inside one agent call.
- **Entry point.** `send_with_retry(client, policy, idempotency, rate_limit, make_request) -> Result<Response, HttpError>`, where `make_request: FnMut() -> Result<reqwest::Request, HttpError>` builds a fresh request per attempt. A streaming body is consumed by the first attempt, so the closure is what lets an upload reopen its `MediaStore` stream; a closure that cannot rebuild the body returns an error on the second call and the retry stops there.
- **`Idempotency`**: `Idempotent` or `NonIdempotent` (designer's call: an explicit argument, never inferred from the method, because `POST` uploads to a resumable session are idempotent and some `PUT`s are not).
  - Idempotent: retry on `Transient`, on `RateLimited`, and on a 5xx `Status`.
  - Non-idempotent: retry only when the request provably never reached the server: a connect error (`reqwest::Error::is_connect`), or a 429 or 503 with `Retry-After` (the server refused before acting; matches core's `AgentError::after_send`). A timeout, a reset after connect, a body error, or any other 5xx becomes `OutcomeUnknown` and is returned at once.
- **Delay.** Exponential backoff with full jitter: `sleep = rand(0 ..= min(max_delay, base * 2^(attempt-1)))`. When the server gave a `Retry-After` or an agent reset header, the delay is that value (capped by `max_retry_after`) plus a small jitter of up to 10% (designer's call, to avoid a thundering herd at the reset instant). If the next sleep would push the total past `budget`, the call stops and returns the last error.
- **Agent reset headers.** `RateLimitHeaders` is data the caller supplies: an ordered list of `ResetHeader::RetryAfter`, `ResetHeader::DeltaSeconds(name)`, `ResetHeader::EpochSeconds(name)`, `ResetHeader::EpochMillis(name)` (X's `x-rate-limit-reset` is epoch seconds; others use delta seconds). The first one present wins. Default: `[RetryAfter]`.
- **Determinism.** A `Jitter` value holds a `rand::rngs::StdRng`; production seeds it from the OS, tests seed it with a fixed value (`Jitter::seeded(u64)`). Sleeping uses `tokio::time::sleep`, so tests run under `#[tokio::test(start_paused = true)]` and assert elapsed virtual time. "Now" for HTTP-date parsing comes from a `fn() -> DateTime<Utc>`-like seam (the builder picks between a closure and a small trait; `postit-http` cannot use core's `Clock`). The testkit exposes `seeded_jitter()` and a fixed-now helper.
- **Tracing.** Each attempt gets its own `http_request` span (the existing `execute_traced` shape) with an `attempt` field; the retry decision is logged at `debug` with the classification and the chosen delay, never the URL query or body.

### Rate limiting

- `RateLimiters`: a per-process registry of `governor` keyed limiters. A caller registers a **bucket** once (`BucketSpec { name: String, quota: governor::Quota }`) and then calls `acquire(bucket, key).await` before each request, where `key` is the agent's choice (an app-wide constant, an account ID, an endpoint). Unknown bucket names are a programming error and return `HttpError::Permanent` (designer's call: no panic).
- `send_with_retry` takes an optional `(bucket, key)` and acquires before every attempt, so retries also respect the limit.
- Limits are per process, as the plan says; the docs comment on `RateLimiters` says cross-process limits use the quota tables (B2).
- **Determinism.** The registry is generic over `governor`'s clock. Tests use `governor::clock::FakeRelativeClock` and `check_key` to assert admission and rejection without sleeping; one test runs `acquire` under the real clock with a fast quota to show it waits rather than fails.
- `RateLimiters` is cheap to clone (`Arc` inside) and lives in the composition root once, shared by every agent; the builder adds it to whatever `postit-server` already passes to agents (B5 wires it into `AgentContext`).

### Streaming request bodies

- `stream_body(stream: ByteStreamLike, length: Option<u64>) -> reqwest::Body` wraps any `Stream<Item = Result<Bytes, io::Error>> + Send + 'static` with `reqwest::Body::wrap_stream`. `postit-http` cannot name core's `ByteStream` type alias, but it is exactly `BoxStream<'static, io::Result<Bytes>>`, so it fits. When `length` is known, the caller sets `Content-Length`; chunked transfer otherwise.
- Test: a 64 MiB generated stream is uploaded to wiremock (or a minimal tokio TCP sink if wiremock buffers) while a counting wrapper asserts that no more than a few chunks are alive at once. The builder picks the measurement; the point is to prove the body is not collected into one buffer.

### Redaction

- The existing static patterns stay. B1 adds a **`Redactor`**: the static rules plus caller-registered `SensitiveUrl` patterns. Each pattern is a host (exact or suffix) plus an optional path prefix, and a matching URL is logged as `scheme://host/[redacted]` with no path or query (designer's call: whole-path redaction, because webhook and push secrets live in the path).
- Built-in sensitive URLs (designer's call): `hooks.slack.com`, `discord.com/api/webhooks`, `discordapp.com/api/webhooks`, and the deployment's own media public route (`{media.public_base_url}/public/media/` and the email tracking routes `{email.tracking_base_url}/public/`), added by the composition root because only it knows those bases. Push endpoints are always fully redacted: the push sender (D1) passes `Sensitivity::Opaque` for the request, which redacts the whole URL to `[redacted-url]`.
- `execute_traced` and `send_with_retry` use the redactor for the span URL. A process-wide redactor is set once at startup through a `OnceLock` (designer's call), so the free function `redact_secrets` keeps working everywhere with the registered patterns. Tests that need custom patterns build a local `Redactor` instead of touching the global.
- `SENSITIVE_QUERY_PARAMS` gains `client_secret`, `refresh_token`, `id_token`, `exp`, and `state` (OAuth state is a CSRF token).
- The P10 no-secret-leak harness (`testkit::leak`) covers the new paths: a retried request with a bearer token, a webhook URL, a signed media URL, and an OAuth code produces spans and errors with none of the fixture secrets.

### Configuration

- No new config section is required. `RetryPolicy` defaults live in code; agents override per call. **Designer's call:** `[http]` gains nothing in B1; if the builder finds a need for an operator knob (for example a global retry budget), it adds it under `[http]` with the existing validation style and documents it in `docs/configuration.md`.

## Testing

All without real network: wiremock for HTTP, a tokio TCP listener where wiremock cannot express the case (connection reset after the request body is read), and paused tokio time.

- **Retry:** idempotent GET retried on 503 and succeeding on the third try; budget exhausted stops early with the last error; `Retry-After` delta and HTTP-date honored (virtual time asserted); `Retry-After` above the cap surfaces `RateLimited` without sleeping; an agent epoch-seconds reset header honored; full-jitter delays reproduce exactly for a fixed seed; a streaming-body closure is called once per attempt.
- **Non-idempotent:** POST with a server that reads the body then resets the connection returns `OutcomeUnknown` after one request (mock asserts exactly one); POST timing out after send returns `OutcomeUnknown`; POST refused at connect (closed port) is retried; POST with 429 and `Retry-After` is retried; POST with 500 returns `OutcomeUnknown` without retry.
- **Timeouts:** connect timeout classified `Transient`; request timeout on an idempotent call retried, on a non-idempotent call `OutcomeUnknown`.
- **Rate limiting:** a bucket of N per period admits N and rejects N+1 under the fake clock; separate keys do not share capacity; separate buckets do not share capacity; `send_with_retry` acquires before each retry.
- **Address policy:** a table-driven unit test over every forbidden range and a few public addresses, under both flag values; IPv4-mapped and NAT64 forms; a resolver test with a stub resolver returning a mix of public and private addresses (only the public ones are kept) and only private ones (error); an IP-literal URL rejected with zero requests received; a redirect returned, not followed; `HTTP_PROXY` set in the test process does not affect the public client (the builder decides whether this test is practical; if not, a unit assertion on the builder configuration plus a comment suffice); `resolve_checked` for SMTP under both flag values.
- **Redaction:** webhook, signed media, push, and OAuth fixtures redacted in spans (captured with the existing tracing-subscriber test layer) and in `HttpError` messages; `body_excerpt` redacted.
- **Tooling:** the `clippy.toml` rule is proven by a short note in the plan's verification step (run clippy on a scratch change that calls `reqwest::Client::new()` in another crate, see it fail, revert). No permanent failing fixture.

## Left to the builder

- Module layout inside `postit-http` (suggested: `retry.rs`, `rate_limit.rs`, `policy.rs`, `body.rs`, extending `redact.rs` and `error.rs`).
- Whether `Status` is a new variant or extra fields on the existing ones, as long as B5 gets status, delay, and a redacted excerpt.
- The "now" seam's exact form.
- How the bounded-memory streaming test measures chunks.
- Exact names, as long as they read like the existing crate.
