# B1: HTTP Resilience and Outbound Address Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every outbound call through `postit-http` safe to retry, rate limited per agent bucket, streamed, redacted, and unable to reach private addresses that a user supplied, with all of it proven without network access.

**Architecture:**
- `postit-http` keeps `build_client` as the trusted client and adds `build_public_client` with a filtering DNS resolver, no redirects, and no proxy.
- `send_with_retry` drives attempts from a request-building closure, using an explicit `Idempotency`, a `RetryPolicy`, full-jitter backoff, server and agent reset headers, and optional `governor` buckets.
- A process-wide `Redactor` extends the static redaction rules with sensitive URL patterns.
- `server/clippy.toml` forbids building `reqwest` clients outside this crate.

**Tech Stack:** Rust 2024, reqwest (rustls), `governor` 0.10, `ipnet` 2, `rand`, `chrono`, `tokio` (paused time in tests), wiremock.

**Spec:** `docs/superpowers/specs/2026-10-19-b1-http-resilience-design.md`, which argues from `!ref/plans/03. publishing roadmap.md` phase B1. Read both.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`, and `unwrap_used`, `expect_used`, `panic` set to deny outside tests).
- `postit-http` depends on `postit-config` only. It does not depend on `postit-core`.
- New dependencies of `postit-http` come from `[workspace.dependencies]`: `governor`, `ipnet`, `rand`, `chrono`, `bytes`, `futures`, `tokio` (non-dev; it needs `net` and `time`). No new workspace entries are expected. If one is needed, it gets an exact major version and no Arabic or Arabic-derived name.
- `reqwest` keeps its feature set: no `blocking`, `cookies`, or `socks`.
- Existing public functions keep their signatures. The only changes in callers (`postit-identity`, `postit-server`) are match arms for new `HttpError` variants.
- No test makes a real network call or sleeps in real time. Use wiremock, local tokio listeners, `#[tokio::test(start_paused = true)]`, and `governor`'s `FakeRelativeClock`.
- No URL path or query, header value, or body from a fixture secret appears in a span, an error `Display`, or a `Debug`.
- After every task, run from `server/`:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test -p postit-http`

  The last task runs the full gates from CLAUDE.md.
- Commits use Conventional Commits with scope `http` (or the crate touched) and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Writing: American English.

## Review Focus

1. **A URL whose host is an IP in a non-dotted form**, such as `http://2130706433/`, `http://0x7f.1/`, or `http://[::ffff:7f00:1]/`. The `url` crate normalizes these to an IP literal, and each must be rejected like `127.0.0.1`. Task 2 tests this.
2. **A DNS answer that mixes public and private addresses.** Only the public addresses may be dialed, never a fallback to a private one. Task 2 tests this with a stub resolver.
3. **A garbage `Retry-After`** (`"soon"`, a negative number, a date in 1999, or a value of a year). Garbage is ignored and backoff is used. A past date means zero. A value over the cap surfaces as `RateLimited` without sleeping. Task 1 tests this.
4. **A non-idempotent POST answered with a plain 503 and no `Retry-After`.** It becomes `OutcomeUnknown` and is not retried, because the server may have acted. Task 3 tests this.
5. **A webhook host written in mixed case, or with a trailing dot** (`HOOKS.Slack.com.`). It must still be redacted. Task 5 tests this.

---

### Task 1: Error variants and `Retry-After` parsing

**Files:**
- Modify: `server/crates/http/src/error.rs`, `server/crates/http/src/lib.rs`, every exhaustive `match` on `HttpError` in `server/crates/identity/src/` and `server/crates/server/src/` (find them with `rg "HttpError::" server/crates`)
- Test: `server/crates/http/tests/errors.rs` (new)

**Interfaces:**
- Produces:
  - `HttpError::OutcomeUnknown(String)`
  - `HttpError::AddressRejected { host: String }`
  - `HttpError::Status { status: u16, retry_after: Option<Duration>, body_excerpt: Option<String> }`
  - `HttpError::is_retryable()` is unchanged for the old variants. It returns `false` for `OutcomeUnknown` and `AddressRejected`. For `Status` it returns `true` when `status == 429 || status >= 500`.
  - `pub fn retry_after_at(headers: &HeaderMap, now: DateTime<Utc>) -> Option<Duration>`, which parses delta-seconds or an IMF-fixdate. A past date gives `Duration::ZERO`. Anything unparsable gives `None`.
  - `retry_after_from_headers(headers)` keeps its signature and delegates with `Utc::now()`.
  - `pub async fn status_error(response: reqwest::Response, now: DateTime<Utc>) -> HttpError`. It reads at most 512 bytes of the body, runs `redact_secrets` on them, and builds `Status`.

- [ ] **Step 1: Write failing tests.**

```rust
#[test] fn retry_after_delta_seconds() // "120" -> Some(120s)
#[test] fn retry_after_http_date_relative_to_now() // now = 2015-10-21T07:27:00Z, "Wed, 21 Oct 2015 07:28:00 GMT" -> Some(60s)
#[test] fn retry_after_past_date_is_zero() // "Fri, 01 Jan 1999 00:00:00 GMT" -> Some(ZERO)
#[test] fn retry_after_garbage_is_none() // "soon", "-5", "" -> None
#[tokio::test] async fn status_error_redacts_and_truncates_excerpt() // wiremock 400 body "access_token=SECRET123 " + 2000 x 'a' -> Status{status:400}, excerpt.len() <= 512 + redaction slack, !contains("SECRET123")
#[test] fn outcome_unknown_and_address_rejected_are_not_retryable()
#[test] fn address_rejected_display_has_host_only() // AddressRejected{host:"internal.example"}.to_string() contains host, no "/" path
```

- [ ] **Step 2: Run** `cargo test -p postit-http --test errors`. Expected: the compile fails on the missing items.

- [ ] **Step 3: Implement.** Use chrono's `DateTime::parse_from_rfc2822` for the date form. Add the match arms the compiler asks for in `identity` and `server`, mapping the new variants the same way `Permanent` is mapped there today.

- [ ] **Step 4: Run** `cargo test -p postit-http`, then `cargo check --workspace --all-targets`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(http): classify outcome-unknown, address, and status errors with HTTP-date Retry-After`.

---

### Task 2: Address policy, public client, and the client-construction lint

**Files:**
- Create: `server/crates/http/src/policy.rs`, `server/clippy.toml`
- Modify: `server/crates/http/src/client.rs`, `server/crates/http/src/lib.rs`, `server/crates/http/Cargo.toml`
- Test: `server/crates/http/tests/policy.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug)] pub struct AddressPolicy { pub allow_private: bool }` with `AddressPolicy::strict()`.
  - `pub fn is_allowed(ip: IpAddr, policy: AddressPolicy) -> bool`. It covers the full range table from the spec. IPv4-mapped, IPv4-compatible, and NAT64 (`64:ff9b::/96`) addresses are checked as their embedded IPv4. Unspecified, multicast, and broadcast addresses are refused even with `allow_private`.
  - `pub async fn resolve_checked(host: &str, port: u16, policy: AddressPolicy) -> Result<Vec<SocketAddr>, HttpError>` is the SMTP entry point. An IP-literal host is checked directly. It returns `AddressRejected` when no address remains.
  - `pub(crate) trait Lookup: Send + Sync` (`async fn lookup(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>`), with `SystemLookup` built on `tokio::net::lookup_host`. The filtering logic takes `&dyn Lookup`, so tests can inject a stub. Expose it to integration tests through `testkit::StubLookup`.
  - `pub fn build_public_client(settings: &HttpSettings, policy: AddressPolicy) -> Result<reqwest::Client, HttpError>`: same base as `build_client`, plus a `dns_resolver(Arc<PolicyResolver>)`, `redirect(Policy::none())`, and `.no_proxy()`. Factor out the shared builder setup so the two functions do not duplicate it.
  - `pub async fn execute_public(client: &reqwest::Client, request: reqwest::Request, policy: AddressPolicy) -> Result<reqwest::Response, HttpError>` rejects a disallowed IP-literal host before sending, then calls `execute_traced`.
  - `server/clippy.toml`: `disallowed-methods = [{ path = "reqwest::Client::new", reason = "build clients through postit-http" }, { path = "reqwest::Client::builder", reason = "…" }, { path = "reqwest::ClientBuilder::new", reason = "…" }]`. The single builder call in `client.rs` carries `#[allow(clippy::disallowed_methods)] // postit-http is the only client factory`.

- [ ] **Step 1: Write failing tests.**

```rust
#[test] fn forbidden_ranges_table() // for each (addr, strict_allowed, private_allowed): 127.0.0.1 (f,t), 10.1.2.3 (f,t), 172.16.0.1 (f,t), 192.168.1.1 (f,t), 100.64.0.1 (f,t), 169.254.169.254 (f,t), 0.0.0.0 (f,f), 224.0.0.1 (f,f), 255.255.255.255 (f,f), ::1 (f,t), fc00::1 (f,t), fe80::1 (f,t), ff02::1 (f,f), :: (f,f), ::ffff:127.0.0.1 (f,t), 64:ff9b::7f00:1 (f,t), 8.8.8.8 (t,t), 2606:4700::1111 (t,t)
#[tokio::test] async fn stub_lookup_mixed_keeps_only_public() // stub returns [10.0.0.1:443, 93.184.216.34:443] -> Ok([93.184.216.34:443])
#[tokio::test] async fn stub_lookup_only_private_is_rejected() // stub returns [127.0.0.1:443] -> Err(AddressRejected{host})
#[tokio::test] async fn private_allowed_keeps_lan() // allow_private=true, [192.168.1.5:25] -> Ok
#[tokio::test] async fn resolve_checked_ip_literal() // "127.0.0.1", strict -> Err; allow_private -> Ok
#[tokio::test] async fn public_client_refuses_loopback_literal_without_sending() // wiremock on 127.0.0.1: execute_public(GET http://127.0.0.1:{port}/x, strict) -> Err(AddressRejected); server.received_requests() is empty
#[tokio::test] async fn public_client_refuses_obfuscated_literals() // http://2130706433:{port}/ and http://[::ffff:7f00:1]:{port}/ -> Err(AddressRejected), zero requests
#[tokio::test] async fn public_client_with_private_allowed_does_not_follow_redirects() // allow_private=true, wiremock 302 Location: /other -> response status 302, /other never hit
```

- [ ] **Step 2: Run** `cargo test -p postit-http --test policy`. Expected: FAIL.

- [ ] **Step 3: Implement.** Implement `policy.rs` with `ipnet::IpNet` constants and `contains`. Implement the resolver as `reqwest::dns::Resolve` over a `Lookup`, then filter. `PolicyResolver` returns an `io::Error` whose message names only the host. `execute_public` maps that error back to `AddressRejected` through `HttpError::from` (check `err.source()` chain for a marker error type).

- [ ] **Step 4: Run** the tests and clippy. Expected: PASS. Then **prove the lint once**: add `let _ = reqwest::Client::new();` to any function in `postit-identity` and run clippy. Expected: `disallowed_methods` error. Revert the line. Do not commit the probe.

- [ ] **Step 5: Commit** `feat(http): add the outbound address policy, public client, and client-construction lint`.

---

### Task 3: Retry with backoff, jitter, budget, and reset headers

**Files:**
- Create: `server/crates/http/src/retry.rs`
- Modify: `server/crates/http/src/lib.rs`, `server/crates/http/src/testkit/mod.rs`
- Test: `server/crates/http/tests/retry.rs`

**Interfaces:**
- Consumes: Task 1 `HttpError` variants, `retry_after_at`, `status_error`. Task 2 `execute_public` (when the caller passes a policy).
- Produces:
  - `#[derive(Clone, Copy, Debug)] pub enum Idempotency { Idempotent, NonIdempotent }`
  - `#[derive(Clone, Debug)] pub struct RetryPolicy { pub max_attempts: u32, pub base_delay: Duration, pub max_delay: Duration, pub max_retry_after: Duration, pub budget: Duration }`. Its `Default` is 3, 500ms, 30s, 15min, and 60s.
  - `#[derive(Clone, Debug)] pub enum ResetHeader { RetryAfter, DeltaSeconds(HeaderName), EpochSeconds(HeaderName), EpochMillis(HeaderName) }`. `pub struct RateLimitHeaders(pub Vec<ResetHeader>)`. Its `Default` is `[RetryAfter]`.
  - `pub struct Jitter` with `Jitter::from_entropy()` and `Jitter::seeded(u64)`, plus `fn backoff(&mut self, attempt: u32, policy: &RetryPolicy) -> Duration` (full jitter) and `fn spread(&mut self, base: Duration) -> Duration` (adds up to 10%).
  - `pub type Now = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>`.
  - `pub struct RetryRequest<'a> { pub client: &'a reqwest::Client, pub policy: &'a RetryPolicy, pub idempotency: Idempotency, pub headers: &'a RateLimitHeaders, pub address_policy: Option<AddressPolicy>, pub limiter: Option<(&'a RateLimiters, &'a str, &'a str)>, pub now: Now }`. The `RateLimiters` field type arrives in Task 4. Until then, leave the field out and add it in Task 4.
  - `pub async fn send_with_retry<F>(req: RetryRequest<'_>, jitter: &mut Jitter, make: F) -> Result<reqwest::Response, HttpError> where F: FnMut() -> Result<reqwest::Request, HttpError>`. It returns the first 2xx/3xx response. Any other status, after retries, comes back as `Err(status_error(..))`.
  - Testkit: `pub fn seeded_jitter() -> Jitter` (seed `0x5eed`) and `pub fn fixed_now(at: DateTime<Utc>) -> Now`.
- Decision rules, copied from the spec:
  - Idempotent: retry `Transient`, `RateLimited`, and 5xx/429 `Status`.
  - NonIdempotent: retry only connect errors (`reqwest::Error::is_connect`) and 429/503 responses that carry a reset header. A timeout, any other error after connect, or any other 5xx becomes `OutcomeUnknown(redacted text)` at once.
  - Delay: the reset-header value capped at `max_retry_after` plus `spread`, else `backoff`. A reset value over the cap returns `RateLimited { retry_after: Some(full) }` without sleeping.
  - Stop when the next sleep would exceed `budget` in total, or when `max_attempts` is reached. Return the last error.

- [ ] **Step 1: Write failing tests.** All under `#[tokio::test(start_paused = true)]`, with wiremock `up_to_n_times` sequences:

```rust
async fn idempotent_get_retries_503_then_succeeds() // 503,503,200 -> Ok(200); exactly 3 requests
async fn backoff_is_reproducible_for_a_seed() // two runs with seeded_jitter(): recorded elapsed virtual times equal
async fn budget_stops_retries_early() // budget 1s, base 800ms, always 503 -> Err(Status{503}); fewer than max_attempts requests
async fn retry_after_delta_is_honored() // 429 Retry-After: 7, then 200 -> elapsed >= 7s and < 7.7s
async fn retry_after_http_date_is_honored() // fixed_now; 503 with date now+5s, idempotent -> elapsed ~5s
async fn retry_after_over_cap_surfaces_without_sleeping() // 429 Retry-After: 86400 -> Err(RateLimited{Some(86400s)}); elapsed == 0
async fn garbage_retry_after_uses_backoff() // 429 Retry-After: soon -> retried with backoff delay
async fn epoch_seconds_reset_header() // headers [EpochSeconds("x-rate-limit-reset")], value now+3 -> elapsed ~3s
async fn non_idempotent_post_plain_503_is_outcome_unknown() // 503 no header -> Err(OutcomeUnknown); exactly 1 request
async fn non_idempotent_post_500_is_outcome_unknown() // exactly 1 request
async fn non_idempotent_post_429_with_retry_after_is_retried() // 429 RA:1, 200 -> Ok; 2 requests
async fn non_idempotent_post_connect_refused_is_retried() // bind+drop a TcpListener for a dead port, then point at wiremock on attempt 2 via closure switching URLs -> Ok
async fn non_idempotent_post_reset_after_body_is_outcome_unknown() // tokio TcpListener that reads the request then drops the socket -> Err(OutcomeUnknown); listener saw 1 connection
async fn non_idempotent_post_timeout_is_outcome_unknown() // wiremock delay > request_timeout -> Err(OutcomeUnknown); 1 request
async fn make_is_called_once_per_attempt() // counter in closure == requests received
async fn closure_error_on_rebuild_stops() // closure Errs on 2nd call -> that Err returned
```

  Paused time and a real socket can interact badly. If the reset-after-body or timeout cases cannot run under paused time, run those two with real time and a short `request_timeout` of 200ms or less, and add a comment saying why.

- [ ] **Step 2: Run** `cargo test -p postit-http --test retry`. Expected: FAIL.

- [ ] **Step 3: Implement** `retry.rs`. Every attempt goes through `execute_traced`, or through `execute_public` when `address_policy` is set. Record an `attempt` field on the span. Log each retry decision at `debug` with the classification and the delay only.

- [ ] **Step 4: Run** the tests. Expected: PASS.

- [ ] **Step 5: Commit** `feat(http): add send_with_retry with full-jitter backoff, retry budget, and reset headers`.

---

### Task 4: Per-bucket rate limiters and streaming bodies

**Files:**
- Create: `server/crates/http/src/rate_limit.rs`, `server/crates/http/src/body.rs`
- Modify: `server/crates/http/src/retry.rs` (add the limiter field), `server/crates/http/src/lib.rs`
- Test: `server/crates/http/tests/rate_limit.rs`, `server/crates/http/tests/body.rs`

**Interfaces:**
- Produces:
  - `pub struct BucketSpec { pub name: String, pub quota: governor::Quota }`
  - `#[derive(Clone)] pub struct RateLimiters<C = governor::clock::DefaultClock>`, backed by `Arc<HashMap<String, governor::DefaultKeyedRateLimiter<String>>>` (generic over the clock). It has these methods:
    - `fn new(specs: Vec<BucketSpec>) -> Self`
    - `fn with_clock(specs, clock: C) -> Self`
    - `async fn acquire(&self, bucket: &str, key: &str) -> Result<(), HttpError>`, which waits until the bucket admits the request
    - `fn try_acquire(&self, bucket: &str, key: &str) -> Result<bool, HttpError>`

    An unknown bucket returns `HttpError::Permanent("unknown rate-limit bucket {name}")`. The rustdoc states that limits are per process and that cross-process limits use the quota tables.
  - `RetryRequest.limiter: Option<(&RateLimiters, &str, &str)>`. `send_with_retry` calls `acquire` before every attempt.
  - `pub fn stream_body<S>(stream: S) -> reqwest::Body where S: Stream<Item = io::Result<Bytes>> + Send + 'static`.

- [ ] **Step 1: Write failing tests.**

```rust
#[test] fn bucket_admits_quota_then_rejects() // FakeRelativeClock, Quota::per_minute(3): try_acquire x3 true, 4th false; advance 20s -> true
#[test] fn keys_do_not_share_capacity() // "acct-a" exhausted, "acct-b" still true
#[test] fn buckets_do_not_share_capacity()
#[test] fn unknown_bucket_is_an_error()
#[tokio::test] async fn acquire_waits_instead_of_failing() // real clock, Quota::per_second(20) burst 1: two acquires take >= ~40ms, both Ok
#[tokio::test(start_paused = true)] async fn send_with_retry_acquires_before_each_attempt() // limiter wrapper counting acquires; 503,200 -> 2 acquires
#[tokio::test] async fn streamed_body_is_not_buffered() // 64 MiB generated in 64 KiB chunks through a stream that tracks max chunks produced-but-not-yet-polled-by-a-sink; POST to a local tokio listener that reads and discards; assert high-water mark <= 16 chunks and listener read 64 MiB
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.

- [ ] **Step 3: Implement** with `governor::RateLimiter::keyed` and `until_key_ready` for `acquire`. `stream_body` is `reqwest::Body::wrap_stream`. The streaming test measures the high-water mark of chunks generated ahead of the bytes the listener has read, by sharing an `AtomicU64` of bytes read.

- [ ] **Step 4: Run** the tests. Expected: PASS.

- [ ] **Step 5: Commit** `feat(http): add per-bucket rate limiters and streamed request bodies`.

---

### Task 5: Redactor with sensitive URL patterns, and leak coverage

**Files:**
- Modify: `server/crates/http/src/redact.rs`, `server/crates/http/src/client.rs`, `server/crates/http/src/retry.rs`, `server/crates/http/src/lib.rs`, `server/crates/server/src/lib.rs` (register the deployment patterns at startup)
- Test: `server/crates/http/tests/redaction.rs` (extend), `server/crates/http/tests/leak.rs` (new, using `testkit::leak`)

**Interfaces:**
- Produces:
  - `pub struct SensitiveUrl { pub host: String, pub host_suffix: bool, pub path_prefix: Option<String> }`
  - `#[derive(Clone, Default)] pub struct Redactor`, with these methods:
    - `fn with_defaults() -> Self`. The defaults are `hooks.slack.com`, `discord.com` with prefix `/api/webhooks`, and `discordapp.com` with prefix `/api/webhooks`.
    - `fn add(&mut self, url: SensitiveUrl)`
    - `fn redact_url(&self, url: &Url) -> String`. A match gives `scheme://host/[redacted]`. Anything else gets the existing query-param redaction.
  - `pub fn install_redactor(r: Redactor) -> bool`. It is a `OnceLock` set and returns `false` if one was already installed. `pub fn redactor() -> &'static Redactor` falls back to `with_defaults()`.
  - `#[derive(Clone, Copy)] pub enum Sensitivity { Normal, Opaque }`, a field on `RetryRequest` (default `Normal`). `Opaque` logs the URL as `[redacted-url]`.
  - Host matching is ASCII case-insensitive and ignores a trailing dot.
  - `SENSITIVE_QUERY_PARAMS` gains `client_secret`, `refresh_token`, `id_token`, `exp`, and `state`.
  - `postit-server` installs a redactor at startup. It holds the defaults, plus `{media.public_base_url or server.public_url}` with prefix `/public/media/`, plus `{email.tracking_base_url or server.public_url}` with prefix `/public/`.

- [ ] **Step 1: Write failing tests.**

```rust
#[test] fn slack_webhook_path_redacted() // https://hooks.slack.com/services/T0/B0/SECRET -> "https://hooks.slack.com/[redacted]"
#[test] fn mixed_case_and_trailing_dot_host_redacted() // https://HOOKS.Slack.com./services/x/y/SECRET -> no "SECRET"
#[test] fn discord_webhook_redacted_but_other_discord_paths_not() // /api/webhooks/1/tok -> redacted; /api/v10/channels/1 -> unchanged
#[test] fn registered_media_prefix_redacted() // local Redactor + SensitiveUrl{host:"postit.local", path_prefix:"/public/media/"}; /public/media/abc?exp=1&sig=zz -> no "abc", no "zz"
#[test] fn new_query_params_redacted() // ?client_secret=a&refresh_token=b&state=c -> none of a,b,c
#[tokio::test] async fn opaque_request_span_has_no_url() // capture span via testkit tracing layer; send_with_retry with Opaque to wiremock /push/ENDPOINTSECRET -> captured text lacks "ENDPOINTSECRET" and contains "[redacted-url]"
#[tokio::test] async fn retried_request_leaks_nothing() // leak harness fixtures: bearer token header, OAuth code in query, webhook URL; 503 then 200; captured logs + error Display + Debug contain none
#[tokio::test] async fn status_excerpt_leaks_nothing() // body echoes "refresh_token=FIXTURE" -> Status excerpt lacks FIXTURE
```

- [ ] **Step 2: Run** the tests. Expected: FAIL.

- [ ] **Step 3: Implement.** `execute_traced` and `send_with_retry` use `redactor().redact_url`. Also make `redact_secrets` apply the registered sensitive URLs to any URL it finds in free text: match `https?://\S+`, parse each match, and replace it when it matches.

- [ ] **Step 4: Run** `cargo test -p postit-http` and `cargo test -p postit-server`. Expected: PASS.

- [ ] **Step 5: Commit** `feat(http): redact sensitive URLs by host and path, with startup registration`.

---

### Task 6: Docs and full gates

**Files:**
- Modify: `docs/configuration.md` (one paragraph under the `ai`, `push`, and `email` private-address keys explaining the public client, and that Mastodon and Bluesky hosts are always strict), `docs/security.md` (the outbound address policy and redaction in the threat model's SSRF and secret-leak rows), `server/crates/http/src/lib.rs` crate docs (which client to use when)

- [ ] **Step 1:** Write the doc changes in the existing tone.
- [ ] **Step 2: Run** the full gates from CLAUDE.md: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`. Expected: all pass.
- [ ] **Step 3: Commit** `docs: describe the outbound address policy and URL redaction`.
