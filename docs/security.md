# Security

This document is postit's threat model for the foundation: what can go wrong, what stops it, which test proves it, and which risks are accepted on purpose. For how to deploy securely (TLS, proxies, the identity provider), see the [deployment guide](deployment.md).

Contents:

1. [Scope and method](#scope-and-method)
2. [Findings register](#findings-register)
3. [Accepted risks](#accepted-risks)
4. [Reporting a vulnerability](#reporting-a-vulnerability)

## Scope and method

This review covers the foundation surface as of plan 02 P10 (2026-10-15): bearer-token verification, the signing-key cache, the principal cache, rate limits and client addresses, CORS, the act-as extension point, the admin API and job console, secrets in logs and errors, the web app's token handling and security headers, and dependency advisories.

It was done by reading the code paths named in each row and checking that a test pins each control. Publishing, delegation, uploads and outbound requests to social platforms do not exist yet. Plan 03 phase G3 extends this file for them.

Status values:

- **fixed in P10**: the hardening pass added the control or its test.
- **in place**: the control and its test existed before P10; the review confirmed them.
- **accepted**: a known gap, explained under [Accepted risks](#accepted-risks).
- **deferred to B6/G3**: the feature does not exist yet; its review belongs to that plan 03 phase.

## Findings register

Test paths are relative to `backend/crates/` unless they start with `web/`.

### Token validation

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| Algorithm confusion (an HMAC token signed with the public key) | Only the asymmetric algorithms in `auth.oidc.algorithms` are accepted; HS* never is. | `identity/tests/verifier.rs` | in place |
| `alg: none` | The header fails to decode, so the token is refused before any key lookup. | `identity/tests/verifier.rs` | in place |
| Missing or wrong `aud` or `iss`, expired, not yet valid | `aud` must include a configured audience, `iss` must equal the issuer, `exp` and `nbf` are required and checked with `auth.oidc.leeway`. | `identity/tests/verifier.rs` | in place |
| A token issued to another client of the same IdP project | The client claim (`client_id`, falling back to `azp`) must be in `auth.oidc.allowed_clients`. | `token_from_another_client_is_refused` | fixed in P10 |
| Oversized bearer used to burn CPU | Bearers over 8 KiB are refused with the same `invalid_token` problem as any other bad token. | `an_oversized_bearer_is_the_uniform_invalid_token` | fixed in P10 |
| Parser panics on malformed input, or error details that tell an attacker which check failed | Property tests feed malformed tokens through the parsing path: no panic, one constant error shape. | `identity/tests/verifier_props.rs`, `api/tests/malformed_tokens.rs` | fixed in P10 |

### Signing keys

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A flood of tokens with unknown `kid` values forces JWKS refetches | At most one refetch per 60 seconds per process, shared by concurrent callers. | `identity/tests/discovery.rs` | in place |
| Identity provider outage | The last good key set is kept; already-issued tokens keep verifying until they expire. | `identity/tests/discovery.rs` | in place, see [accepted risk 4](#accepted-risks) |

### Principal cache

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A disabled user keeps using a cached principal | Postgres `LISTEN` eviction across processes, a generation counter against stale refills, and a TTL (`auth.principal_cache_ttl`). | `identity/tests/cache.rs` | in place, see [accepted risk 3](#accepted-risks) |

### Rate limits and client address

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| Unauthenticated or bad-token request floods | A per-IP bucket for unauthenticated requests and failed tokens, a stricter gate for requests that provision a user, and a per-user bucket for authenticated requests. | `api/tests/middleware.rs` | in place |
| A client spoofs its address with `X-Forwarded-For` | The header is honored only from `server.trusted_proxies`, read right to left. | `forwarded_for_is_honored_only_from_a_trusted_proxy`, `api/src/client_ip.rs` unit tests | in place |

### CORS

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| Another site calls the API with the user's browser | An origin allowlist (wildcards are a config error), no credentials, fixed method and header lists. | `cors_preflight_allows_exactly_the_listed_headers`, `a_wildcard_cors_origin_is_a_config_error` | in place |

### Act-as

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A caller sends `X-Postit-Act-As` before delegation exists and something honors it | Every request carrying the header gets 403 `forbidden` until plan 03 B6. | `act_as_is_forbidden_everywhere_until_delegation_exists` | fixed in P10 |
| Delegation resolution, access levels, re-delegation | Not built yet. | — | deferred to B6/G3 |

### Admin surface and job console

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A member reaches an admin operation | Every `/users` and `/admin/*` operation refuses a member; the test sweeps every such operation in the OpenAPI document, so a new route is covered automatically. | `every_admin_operation_refuses_a_member` | fixed in P10 |
| The job console leaks a job's secrets | The console shows payload summaries and error kind and code only, never raw payloads or error text. | `api/tests/jobs.rs`, including `the_console_never_leaks_a_send_email_failure` | in place |
| Two admins disable each other at once and leave no active admin | Admin-set changes are serialized with a Postgres advisory lock. | `concurrent_mutual_disable_leaves_one_admin` | fixed in P10 |

### Outbound requests

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A user-supplied host (AI endpoint, push endpoint, SMTP host, Mastodon instance, Bluesky PDS) reaches loopback, private, link-local or cloud-metadata addresses (SSRF), including through DNS rebinding, an IP literal, an IPv4-mapped IPv6 address, a redirect or `HTTP_PROXY` | The public client (`PublicClient`) resolves through a filtering resolver that keeps only addresses `AddressPolicy` allows, follows no redirects and uses no proxy. It carries its policy and exposes no direct send, so every send (`execute_public`, or `send_with_retry` through `ClientRef::Public`) checks IP-literal hosts first. `allow_private` (the `ai`, `push` and `email` flags) lifts only the private ranges, never unspecified, multicast, broadcast or reserved ones. Mastodon and Bluesky hosts are always strict. SMTP uses `resolve_checked`. | `http/tests/policy.rs` | in place (callers wire it in as each feature lands) |

### Identity mail webhook

Zitadel's HTTP email provider calls `POST /internal/identity-mail` on postit, which renders and sends each identity email (verification codes, reset links) itself.

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| The internal listener is reached from outside Zitadel's network, and anyone can make postit send mail | The listener (`identity_mail.bind`, port 44312 in development) is separate from the public one and serves only this route, so the public router does not contain it. It must be reachable from Zitadel's network only, and is never proxied by nginx or any public reverse proxy. Production never publishes it. Development publishes 44312 on the host so Zitadel reaches it through `host.docker.internal`; there the signature is the only gate. | `api/tests/identity_mail.rs` | in place |
| A forged or tampered call | Every call must carry a valid `Zitadel-Signature: t=<unix>,v1=<hex>`: HMAC-SHA-256 with the provider's signing key over `{t}.{raw body bytes}`. A missing, malformed, or wrong signature answers 401 and sends nothing. | `api/tests/identity_mail.rs`, unit tests in `api/src/zitadel_signature.rs` | in place |
| A captured call is replayed | The timestamp `t` must be within `identity_mail.signature_tolerance` (default 5 minutes) of now. The window bounds a replay; it does not prevent one inside the window. | `stale_and_future_timestamps` in `api/src/zitadel_signature.rs`, and the stale case in `missing_wrong_and_stale_signatures_answer_401_and_send_nothing` (`api/tests/identity_mail.rs`) | in place |
| An oversized or malformed body ties up memory or renders junk | The body is read up to `identity_mail.max_body_bytes` (default 64 KiB) and refused with 413 beyond it; unreadable JSON and an invalid recipient address answer 400. Everything rendered into HTML is escaped, and only `http` and `https` links become buttons. | `api/tests/identity_mail.rs`, `mail/tests/identity.rs` | in place |
| Links, codes, or recipient addresses are stored or logged | The webhook sends synchronously and answers 2xx only after the mail server accepted the message, so nothing goes to the job ledger or the database. Logs carry the event type and status only; the request is not traced and the signature header is marked sensitive. | `api/tests/identity_mail_leak.rs` | in place |
| A duplicate email on a Zitadel retry | Accepted. A non-2xx answer (503 for a transient SMTP failure) makes Zitadel retry the same event, and a retry after an unseen success can send a second mail. A duplicate code or link is harmless, and dropped mail would be worse. | | accepted |
| An identity email lost while postit is down | Accepted. Zitadel retries a failed delivery for only about a minute (10 failures), then drops the event; the user asks for a new code or link. Restarts stay short, and planned downtime switches to the SMTP provider ([deployment](deployment.md), identity emails). | | accepted |
| Zitadel is allowed to call internal addresses | Zitadel's outbound denylist (`ZITADEL_HTTPCLIENT_DENYLIST`) blocks private and loopback addresses by default. A deployment that reaches postit over a private network relaxes it only as far as needed (development blocks only `169.254.0.0/16`). | | documented |

### Secrets in output

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| Bearer tokens, the SMTP password, the database password or the audit pseudonym key appear in logs, errors, `Debug` output or problem bodies | `secrecy` types and redaction; a shared `assert_no_leak` harness checks each output channel. | `http/tests/redaction.rs`, `api/tests/redaction.rs`, `mail/tests/smtp_leak.rs`, `backend/tests/startup_leak.rs` | fixed in P10 |
| A webhook token, signed media URL or push endpoint in a URL path appears in a span, log or error | The `Redactor` logs a URL matching a registered `SensitiveUrl` (host plus path prefix) as `scheme://host/[redacted]`. Slack and Discord webhooks and Telegram Bot API paths (`api.telegram.org/bot<token>`, `/file/bot<token>`) are built in, and the server registers the media and email tracking base URLs at startup. A push endpoint is logged `Opaque`. The redactor is installed once at startup, before any agent exists, so any other host that carries a secret in the path is added as a `SensitiveUrl` in `compose::redactor` before it is installed. Query parameters such as `code`, `token` and `signature` are always redacted, and error status excerpts are scrubbed. | `http/tests/redaction.rs`, `http/tests/leak.rs` | in place |

### Media

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| An upload is not what its name or `Content-Type` says (SVG or HTML with script, a renamed executable) | Only JPEG, PNG, GIF, WebP, MP4 and QuickTime are accepted, decided by sniffing the first 8 KiB; the stored type is the sniffed one, never the client's. The kind implied by the extension must match the sniffed kind. Everything else, SVG, HTML and HEIC included, is `415`. | `media/tests/intake.rs` (`spoofed_mime_refused`, `svg_and_html_refused`), `api/tests/media.rs` (`svg_is_415`, `content_headers`) | in place |
| A stored file runs script in the app or media origin | Media is served as the sniffed type with `X-Content-Type-Options: nosniff` and `Content-Security-Policy: sandbox`; thumbnails are re-encoded JPEG. | `api/tests/media.rs` (`content_headers`, `public_route_serves_signed_url`) | in place |
| A file name reaches the file system (`../../etc/passwd`, a NUL, a very long name) | Paths come only from generated storage keys (`{owner}/{yyyy}/{mm}/{asset}`); `StorageKey::parse` accepts only that grammar, and the resolved path is checked to stay under the canonical root. The original name is metadata: control characters or more than 255 characters are `422`. | `media/src/storage.rs` (tests), `media/tests/intake.rs` (`hostile_filenames`) | in place |
| A signed URL is forged, extended or reused for another file | The signature is an HMAC-SHA256 over the target (`{id}` or `{id}/thumbnail`) and the expiry, verified in constant time before any query. A wrong, missing or expired signature is `403` with one body; a content signature does not work on the thumbnail. | `media/src/signing.rs` (tests), `api/tests/media.rs` (`public_refuses_expired_and_tampered`, `public_thumbnail_needs_its_own_signature`) | in place |
| A signed URL or signature appears in a log or problem body | The route logs the matched route only, never the query; the redactor covers the media base URL. | `api/tests/media_public_leak.rs` | in place |
| Guessing signed URLs, or a page of thumbnails, trips the rate limit | A valid signature counts as authenticated for the per-IP bucket; a forged or expired one is charged to it. | `api/tests/media.rs` (`valid_public_urls_are_not_charged_to_the_ip_bucket`) | in place |
| A large upload exhausts memory, disk or time | Bodies are streamed with a counted limit (`Content-Length` is required and must match); no function holds a whole file. Per-kind size limits and the per-user quota apply before any byte is stored, and the quota reservation locks the owner's limits row, so concurrent uploads cannot pass it. Idle timeouts replace an overall timeout on streaming routes, and a chunk streams into its own file with no database connection or row lock held, so a slow client cannot exhaust the pool. Probing and thumbnails run outside tools under one deadline (`media.complete_timeout`) whose expiry kills the child process and refuses the file. A garbage collector removes failed, expired and orphan files. | `media/tests/bounded_memory.rs`, `data/tests/media.rs` (`concurrent_reservations_never_exceed_quota`), `media/tests/intake.rs` (`append_holds_no_row_lock_while_the_body_streams`, `a_probe_that_never_ends_refuses_the_file`), `media/tests/gc.rs` | in place |
| A hostile image or video crashes the thumbnailer | Images are decoded with an allocation cap and skipped past `media.thumbnail_max_pixels`; a thumbnail failure never fails the upload. `ffprobe` and `ffmpeg` run as child processes with `kill_on_drop` and the file path as an argument, never through a shell. | `media/tests/probe.rs` | in place |

### Web app

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| Script injection steals tokens from web storage | Tokens live in memory only. Sign-in state and the PKCE verifier live in `sessionStorage` (this tab, no token). | `web/src/auth/userManager.test.ts` | fixed in P10 |
| A stolen refresh token is replayed | Refresh tokens rotate (reuse is refused by the IdP) and are revoked on sign-out. | `web/src/auth/userManager.test.ts`, `web/src/auth/tokenSource.test.ts` | fixed in P10 |
| Clickjacking, or the app's sign-in running in a frame | The app never uses an iframe: renewal is by refresh token, and a reload restores the session with one top-level `prompt=none` redirect, at most once per tab. The CSP forbids framing in both directions. | `web/src/auth/SessionProvider.test.tsx`, `web/src/auth/tokenSource.test.ts` | fixed in P10 |
| Missing or proxy-dependent security headers | The server sends the web app's CSP, `X-Content-Type-Options`, `X-Frame-Options` and `Referrer-Policy` itself. | `backend/tests/web.rs` | fixed in P10 |
| API responses cached or content-sniffed | Every API response is `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`. | `api/tests/hardening.rs` | fixed in P10 |

### Mobile app

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| The refresh token is read from the device | It lives in the Android Keystore / iOS Keychain via secure storage, never in preferences or logs. Mitigations: revocation on sign-out, rotation at the IdP, `allowBackup=false`, and data extraction rules that exclude everything from cloud backup and Android 12+ device transfer (which ignores `allowBackup`). | `mobile/test/data/auth/session_manager_test.dart` | accepted risk |
| The dev CA is trusted in a shipped build | Debug-only trust in two parts: Android `network_security_config` for native code, and a development-flavor Flutter asset loaded into a dio `SecurityContext` (Dart's `HttpClient` ignores `network_security_config`). The CI `app` job asserts the qa and production APKs carry neither. | `mobile/test/data/http/dev_trust_test.dart`, CI `app` job | controlled |
| An unreachable IdP signs the user out | The stored refresh token is kept, and only an OAuth rejection clears it. | `mobile/test/data/auth/session_manager_test.dart`, `mobile/test/data/http/auth_interceptor_test.dart` | controlled |
| One client's token is used on another platform | Web, Android, and iOS use separate client IDs, all listed in `allowed_clients`. | server config, verifier, and end-to-end tests | controlled |

### Sealed secrets

Secrets that postit must read back later, such as platform tokens and pasted credentials, are stored sealed by the vault (`postit-vault`). Each value gets its own random data key, which encrypts the value with XChaCha20-Poly1305. A master key from `vault.keys` then wraps that data key, and the wrapped key and the master key version are stored beside the ciphertext in one `bytea`.

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A database dump, backup or SQL injection reveals stored tokens | Sealed columns hold only ciphertext. The master keys live outside the database, in the vault or the deployment's secret mounts, and a backup of the database alone cannot be opened. | unit tests in `crates/vault/src` | in place |
| A sealed value is copied to another row, column or table to make postit use it for the wrong owner or account | The associated data authenticated with every value is the domain tag `postit.vault.v1`, the table name, the column name and the 16 bytes of the row ID. It is never stored, so a value moved anywhere else fails to open. | unit tests in `crates/vault/src` | in place |
| A secret leaks through `Debug`, errors or logs | `Sealed` has a redacting `Debug`, `open` returns the plain value to the one caller that asked for it, and vault errors never include the value or the key. | unit tests in `crates/vault/src` | in place |
| A weak, reused or development master key protects production | Every vault key and signing key is at least 32 bytes (checked on the raw value, and a vault key with leading or trailing whitespace is refused). Outside development the loader refuses the shipped insecure development keys, and it requires `media.signing_key`, `email.signing_key` and `audit.pseudonym_key` to be three different values. | `backend/crates/config/src/loader.rs` (tests module), `backend/crates/config/tests/production_requirements.rs` | in place |
| A restore or a misconfigured start runs with an older key set | The database remembers the newest key version ever configured (`vault_state`). A process whose newest key version is lower refuses to start, and so does one that lacks a version still used by a sealed value. | `backend/crates/data/tests/vault_state.rs` | in place |

**The operator is trusted.** The vault keys sit on the same host as the database and the server process, so whoever operates the deployment, or has host, database and key access together, can decrypt stored tokens. "Admins cannot read other users' content or secrets" holds at the application level: no API route, job console view, doctor check or notification exposes it. It is not a guarantee against the person running the deployment. Users of a hosted deployment should know this.

### Dependencies

| Threat | Control | Test | Status |
| --- | --- | --- | --- |
| A Rust dependency with a known advisory or an unacceptable license | `cargo deny check` in CI. | CI `server` job | in place |
| A web production dependency with a high or critical advisory | `pnpm audit --prod --audit-level high` in CI. | CI `web` job | fixed in P10 |

## Accepted risks

Each risk says why it is accepted and what would reopen it.

1. **CORS preflights and the static web listener are not rate-limited.** They are cheap, keep no state and return no data. Reopen if either starts doing per-request work (for example server-side rendering) or shows up in a load incident.
2. **An ID token presented as a bearer is accepted.** It names the same user, was issued to the same client, and is short-lived, so it grants nothing the access token would not. Reopen if the IdP is configured to issue ID tokens that outlive access tokens, or if any route starts trusting claims only the access token should carry.
3. **A disabled user keeps access on other processes for up to `auth.principal_cache_ttl`** (default 5 minutes). `LISTEN` eviction usually makes it immediate; the TTL bounds the case where a notification is missed. Reopen if a deployment needs a hard revocation bound, then lower the TTL.
4. **During an identity provider outage, issued tokens stay valid until they expire.** Nobody new can sign in. Reopen if the IdP's access-token lifetime is raised well beyond an hour.
5. **The unknown-`kid` refetch limit is per process.** N API replicas can make N JWKS fetches per minute. Reopen if the deployment runs enough replicas for that to matter to the IdP.
6. **Each page reload costs one visible redirect to the identity provider** to restore the session. It is the price of keeping tokens out of storage and the app out of iframes. Reopen if users find it disruptive; the alternative is a backend-for-frontend session, not storing tokens.
7. **A development-only dependency has an unfixed advisory.** `pnpm audit` (without `--prod`) reports GHSA-vfj7-8cjw-p6xm in `braces` <= 3.0.3, pulled in by the `shadcn` CLI, with no patched release yet. The CLI runs only on a developer's machine when adding components; it is not in the production bundle. Reopen when `braces` publishes a fix (then update), or if the CLI moves into a build or CI step that handles untrusted input.

## Reporting a vulnerability

Report a vulnerability privately through a GitHub security advisory on [`asm2025/postit`](https://github.com/asm2025/postit/security/advisories/new). Do not open a public issue. Include the affected version or commit, the steps to reproduce, and the impact you expect.
