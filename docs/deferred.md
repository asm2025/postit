# Deferred work

Items that reviews found and the team chose not to fix in the phase that raised them. Each has a source phase, where it should close, and a status. When you close one, move it to "Closed" with the commit or phase.

Last updated: 2026-10-19 (after the B1 merge).

## Open: B1 (HTTP resilience, merged 2026-10-19)

Source: final and per-task reviews of `docs/superpowers/plans/2026-10-19-b1-http-resilience.md`.

### Should close in B5 (agent-kit) or the first agent that needs it

- **`Status.retry_after` on non-429/503 4xx.** `with_reset` in `backend/crates/http/src/retry.rs` also sets `retry_after` from a reset header on a 4xx such as X's 403. That wait is the rate-limit window, not a back-off request. B5's error mapping must not read it as a rate-limit signal on a 4xx. Either apply `with_reset` only for 429/503 or document it on `Status.retry_after`.
- **`RateLimiters::acquire` waits without a limit** and outside the retry budget. A long-window bucket (for example 300 per 3 hours) can park a worker. B5 should call `try_acquire` first or wrap `acquire` in a timeout.
- **Keyed limiter never evicts keys** (`rate_limit.rs`). One entry per distinct key per bucket. Bounded by account count today; call governor's `retain_recent` and `shrink_to_fit` periodically if cardinality grows.
- **Redaction matches raw URL text.** Paths such as `//api//webhooks/...`, `/API/webhooks`, or `/api/%77ebhooks/...` miss the pattern. Agents that store webhook URLs (Discord, Slack) must parse a pasted URL into its parts and rebuild the canonical URL before sending.
- **Other secret-in-path hosts** need a pattern added through `compose::redactor` in `backend/crates/server/src/compose.rs` before the redactor is installed (the install is a `OnceLock`, set at startup before the agent registry exists). Slack, Discord, and Telegram are defaults.
- **Pooled keep-alive reset on a POST** produces a spurious `OutcomeUnknown`. Correct, but noisy. B5 and the reconciliation job must absorb it.
- **A 3xx from the public client** is returned, never followed. An agent that follows `Location` by hand must send the next request through the same `PublicClient`.

### Hardening, any time

- **Unparsable or JSON-escaped URLs in free text** pass through `redact_secrets` unredacted (for example `https://hooks.slack.com:99999/...` or `https:\/\/hooks.slack.com\/...`). Redact the whole match when its host substring matches a pattern, and unescape `\/` before scanning.
- **Startup redactor wiring is untested.** Deleting the `install_http_redactor` call in `backend/crates/server/src/lib.rs` fails no test. Add a test binary that runs `prepare` with a config that fails at the database step and asserts `postit_http::redactor()` redacts a `/public/media/` URL.
- **`log_effective_config` runs before the redactor is installed** (`backend/crates/server/src/lib.rs`). It sends no HTTP and has its own redaction, so nothing leaks today. Move the install earlier if logging ever relies on `redact_secrets`.
- **Not filtered by the address policy:** 6to4 (`2002::/16`), Teredo, local-use NAT64 (`64:ff9b:1::/48`), and documentation ranges. The spec does not list them and the risk is low.
- **`clippy.toml` cannot ban `Client::default()`** or `ClientBuilder::default()` because clippy cannot name trait-impl paths. Review catches them.
- **Trusted client with a user-supplied host.** `build_client` honors proxies and redirects. Only review and docs keep callers from using it for user-typed hosts.
- **`leak.rs` real-time waits.** Backoff sleeps of 1 to 5 ms and a real 300 ms client timeout. Margins are large, but the plan says no real-time sleeps. Run the backoff-only tests under `start_paused`.
- **Connect-refused retry test** takes about 2 seconds on Windows (SYN retries). `closed_port()` binds then drops a port, so another process could take it before use.
- **Test-only reqwest client with `#[allow(clippy::disallowed_methods)]`** in `tests/retry.rs`. Move it to a `testkit::untimed_client()` helper so the exception lives in one place.
- **Docs.** `docs/superpowers/specs/2026-10-19-b1-http-resilience-design.md` and the B1 plan still show the old `execute_public(client, request, policy)` and `address_policy` shapes. The implemented API uses `PublicClient` and `ClientRef`. Update them or leave as history.

## Open: A2 (vault, platforms, configuration, merged to stage)

Source: A2 final whole-branch review and task reviews, recorded from the A2 session's hand-off.

- **Cross-check the sealed-column format test vector** against an independent XChaCha20-Poly1305 and HKDF implementation before the first production deploy (plan 03 G1).
- **`OsRng.fill_bytes` panics on a `getrandom` failure.** Use `try_fill_bytes` and return an error.
- **Duplicated testkit wrappers** in `backend/crates/data/src/reseal.rs` under `cfg` gates. Deduplicate.
- **Key trimming and length inconsistency.** Keys with leading or trailing whitespace are rejected and length is checked on raw bytes; the minor inconsistency between that and trimmed handling elsewhere is unresolved.
- **`settings.rs` is over 900 lines** (`backend/crates/config`). Split it when next touched.
- ~~**`vapid_subject` scheme is not enforced** (should be `mailto:` or `https:`).~~ Resolved in D1 Task 20: `postit-config` refuses any other form.
- **`Services.vault` uses `allow(dead_code)`** where `expect(dead_code)` is better, so the lint fails when the field becomes used.
- **No route enqueues `vault_rotate` yet.** `POST /admin/vault/rotate` arrives in plan 03 D3; until then nothing can run a rotation. `docs/deployment.md` says so.
- **Maintainer action:** generated vault and signing keys were appended to the extracted `!ref/vault/qa/postit.env` and `!ref/vault/production/postit.env` (not staged). Re-pack `!ref/vault.7z` when ready.

## Open: P4 (data and identity), from the 2026-09-27 rulings

Status of items 1, 3, and 4 is unknown: they were pinned to P6, which has since shipped. Check the code before treating them as open.

1. **`HttpJwksSource` status classification** (`backend/crates/identity`). A non-2xx JWKS response can be classified permanent instead of transient. Pinned to P6. Verify.
2. **Last-admin guard** (`backend/crates/identity/src/admin.rs`). Now known to deadlock, not only race. Scheduled in the P10 plan, Task 6. Verify it closed.
3. **`PrincipalCache` stale re-insert race** (`backend/crates/identity/src/cache.rs`). Needs an epoch or generation counter. Pinned to P6. Verify.
4. **Claims-to-principal composition function.** Pinned to P6. Probably closed by the API shell. Verify.
5. **Test coverage gaps** in `postit-data` and `postit-identity`, and the no-secret-leak harness. The harness was scheduled in P10 Task 7 and exists as `testkit::leak`. The other gaps (concurrent bootstrap through the real transform path, untested repo functions, rejected-transition combinations) have no owner.
6. **`IdempotencyRepo` takes raw `owner_id` and `actor_id`, not `&OwnerScope`** (`backend/crates/data`). Close in plan 03 B2, the first phase with real callers.

## Open: P5 (jobs and mail), from the 2026-09-27 rulings

- **Unknown job type is killed once pushed to apalis** (`backend/crates/jobs/src/dispatch.rs`). Safe only while all workers run the same release. Plan 03 D1 (scheduled publishes, rolling deploys): re-queue unknown types as `Pending` with `run_at` +60 seconds, no attempt consumed.
- **Audit writes do not lock the users they reference** (`AuditLog::record`, `backend/crates/data/src/audit.rs`). Pinned to before P6 routes landed with a `FOR KEY SHARE` fix. Verify whether it was done.
- **Dead queue supervisor stays dead** (`backend/crates/jobs/src/backend.rs`). Pinned to P6 `/ready`. Verify.
- **Accepted, no fix planned:** approval-email marker race. A pending user whose provisioning commits after an admin's coalesced email can be omitted from it.

## Environment

- **`postit-data --test pool` fails against the shared dev Postgres** with "migration 1 was previously applied but was modified". The dev database holds an older checksum of the single migration (A2 edited it under the single-migration policy). Reset the dev database to clear it.

## Closed

- P5: defer clock skew churn, config duration bounds, stale `apalis.workers` rows, and the P8 console job-state items (verified closed 2026-10-10).
- P4: the vacuous cache test was rewritten.
