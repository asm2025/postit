# D0c Branded Identity Emails Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Zitadel's own emails (initialization, email verification by link or code, password reset, password changed, passwordless registration, invite, domain claimed) are rendered and sent by postit in its layout and voice, in English and Arabic.

**Architecture:** Zitadel's HTTP email provider POSTs each message as signed JSON to `POST /internal/identity-mail` on a new internal listener of the API role. `postit-api` checks the `ZITADEL-Signature` HMAC, `postit-mail` renders the payload into one shared askama layout (which the approval emails also move onto), and the handler sends synchronously through the existing `Mailer`, answering 2xx only after the SMTP server accepted. `cargo xtask zitadel-bootstrap` applies postit's message texts, creates and activates the HTTP provider (writing its signing key into the development vault file), and sets Zitadel's branding.

**Tech Stack:** Rust 2024, axum, askama 0.16, lettre, hmac 0.12 + sha2 0.10 (already workspace dependencies), `insta` (new dev-dependency, snapshots), `serde_norway` (new xtask dependency, YAML), Zitadel admin REST API.

**Spec:** `!ref/plans/03. publishing roadmap.md`, "Phase D0c — Branded identity emails". Design reference: `!ref/design/Zitadel Template Overview.md` (colors, card layout, `.btn` and `.code-box`). Read both.

## Decisions this plan makes where the spec leaves a choice

- **Authentication is Zitadel's signature, not a URL token.** Zitadel returns a `signingKey` when an HTTP email provider is created (`POST /admin/v1/email/http`) and rotates it on `PUT /admin/v1/email/http/{id}` with `expirationSigningKey: "0s"`. Requests carry `ZITADEL-Signature: t=<unix seconds>,v1=<hex HMAC-SHA256 of "<t>.<raw body>">`. The webhook requires a valid signature within `identity_mail.signature_tolerance`. The spec's shared `IDENTITY_MAIL_TOKEN` is replaced by that signing key, stored in the vault as `POSTIT__IDENTITY_MAIL__SIGNING_KEY` in `postit.env`. No secret goes in the URL. Task 1 confirms the header against the running Zitadel. If Zitadel does not send it, stop and bring that back to the user. Do not fall back silently.
- **`stack doctor` stays offline.** It is documented as "no Docker, no network". So it checks that the development `postit.env` has the signing key. Whether the postit webhook is Zitadel's *active* provider is checked by `cargo xtask zitadel-bootstrap --check`, which is read-only.
- **The email header uses the text wordmark, never an image.** Gmail and Outlook block SVG and remote images, and postit's logos are SVG. `templateData.logoURL` and Zitadel's colors are ignored. The design reference's palette is fixed in the layout. The SVG logo still goes into Zitadel's branding for its hosted login.
- **Dev wiring:** the internal listener binds `0.0.0.0:44312` (in the 443xx range, unused). Zitadel reaches it at `http://host.docker.internal:44312/internal/identity-mail`, both for a native `cargo run` and for the containerized `postit-server`, which publishes 44312 in development only. Nginx never proxies 44312. The bootstrap reads `ZITADEL_IDENTITY_MAIL_URL` from `docker/.env` to override that URL.
- **qa and production** keep `identity_mail.enabled = false` until G1. `docs/deployment.md` gains the manual steps.
- The internal listener is always plain HTTP, even when `server.tls.enabled`, because it is not reachable from outside the host or network.

## Global Constraints

- Quality gates: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace` (from `server/`). Use a separate `CARGO_TARGET_DIR` and the DB URL as in memory "Test env gotchas".
- Rust stable, `rust-version = "1.98"`, edition 2024. Dependencies are declared in `[workspace.dependencies]` and used with `.workspace = true`. Use `[workspace.lints]`, and no `unwrap`/`expect` outside tests (tests use `unwrap_or_else(|e| unreachable!("{e}"))` as the existing code does).
- Never log or store a verification link, code, or recipient address. Secrets use `RedactedSecret` (`postit-config`) and are never in `Debug` output.
- The extracted vault (`!ref/vault/`) is git-ignored. Never commit it, and never re-pack `vault.7z`. Tell the user to re-pack after the bootstrap writes the key.
- American English in code, comments, docs, and commits. The Arabic copy is natural Modern Standard Arabic, not a word-for-word translation.
- No dependency with an Arabic or Arabic-derived name. Prefer maintained libraries over hand-rolled code.
- Brand values (design reference): pink `#df528a`, text `#1f1e24`, body text `#334155`, muted `#64748b`/`#94a3b8`, outer `#f8fafc`, card `#ffffff`, border `#e2e8f0`, code box `#fdf2f6` with border `#f9d8e5`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (or the model that did the work, per memory "Commit trailer model").

## Review Focus

1. **HTML or markup inside Zitadel's text fields** (`text`, `greeting`, a display name like `<b>Ada</b>`): it must render escaped, never as markup. That is tested in Task 4.
2. **A Zitadel retry after the SMTP server accepted but the 2xx was lost** sends a second identical email. This is accepted, because a duplicate is better than a missing code. It must never cause a 5xx loop on a permanent failure: a permanent SMTP rejection answers 502, and Zitadel's retry budget ends it. Tested in Task 6.
3. **A recipient address that is not a valid mailbox** (empty, display-name form, CRLF injection `a@b.c\r\nBcc: x@y.z`) answers 400 and sends nothing. Tested in Task 6.
4. **An Arabic display name inside English text, or a Latin code or URL inside Arabic text,** keeps its own direction. Names and codes sit in `<bdi>`, and the code box and URL are `dir="ltr"`. Tested in Task 4.
5. **A signature header with several `v1=` values** (Zitadel rotating keys), extra whitespace, or a timestamp in the future beyond the tolerance: any matching `v1` is accepted, and a timestamp out of tolerance in either direction is refused. Tested in Task 5.

---

### Task 1: Phase-start check against the running Zitadel

Nothing in this task ships code except the pinned image tag and the payload fixtures. Every later task depends on what it confirms.

**Files:**
- Modify: `docker/docker-compose.development.yml` (pin `ghcr.io/zitadel/zitadel:<version>`)
- Create: `server/crates/mail/tests/fixtures/zitadel/verify_email_code.json`, `server/crates/mail/tests/fixtures/zitadel/password_reset_link.json`
- Modify: this plan (append a "Phase-start findings" section at the end)

- [ ] **Step 1:** Start the dev stack (`./stack.ps1 up`) and read the running Zitadel version (`docker image inspect ghcr.io/zitadel/zitadel:latest --format '{{index .Config.Labels "org.opencontainers.image.version"}}'`, else `docker exec postit-zitadel /app/zitadel --version`). Replace `:latest` with that exact tag in `docker-compose.development.yml`, and add a comment saying D0c's payload fixtures were captured against it.
- [ ] **Step 2:** Run a throwaway capture listener on the host port 44312 that logs headers and the raw body. Use a PowerShell `System.Net.HttpListener` script in the scratchpad and answer 200. Create an HTTP email provider with the bootstrap's machine token (`POST /admin/v1/email/http {"endpoint":"http://host.docker.internal:44312/internal/identity-mail","description":"d0c capture"}`), record the returned `signingKey`, and activate it.
- [ ] **Step 3:** Trigger an email verification by code (`member@postit.com`: `POST /v2/users/{id}/email/_resend` with `{"returnCode":null,"sendCode":{}}`) and a password reset by link (`POST /v2/users/{id}/password_reset` with `{"sendLink":{}}`). Capture both requests.
- [ ] **Step 4:** Confirm and record in "Phase-start findings":
  - the signature header's name and format, and that HMAC-SHA256(signingKey, `"{t}.{body}"`) reproduces the captured `v1` (check it with a few lines of Rust or `openssl dgst`);
  - the JSON field names under `contextInfo`, `templateData`, and `args`, and where the language is carried, if anywhere;
  - whether `templateData.text` contains HTML or newlines;
  - the message-text endpoint for each type (`PUT /admin/v1/text/message/{type}/{language}` with `init`, `verifyemail`, `verifyemailotp`, `passwordreset`, `passwordchange`, `passwordless_registration`, `invite_user`, `domainclaimed`), and what Zitadel answers when a PUT changes nothing;
  - the label-policy endpoints (`PUT /admin/v1/policies/label`, `POST /admin/v1/policies/label/_activate`, `POST /assets/v1/instance/policy/label/logo`, `.../font`) and which font file types the font upload accepts.
- [ ] **Step 5:** Save both payloads as fixtures, with the code replaced by `"123456"`, the URL by `"https://postit.local:44300/ui/login/mail/verification?userID=1&code=ABCDEF"`, the address by `"ada@example.test"`, and IDs by fixed strings. Delete the capture provider (`DELETE /admin/v1/email/{id}`) and re-activate the SMTP provider.
- [ ] **Step 6:** If any finding contradicts a name in Tasks 4–9, update those tasks in this plan before continuing. Commit:

```bash
git add docker/docker-compose.development.yml server/crates/mail/tests/fixtures docs/superpowers/plans/2026-10-24-d0c-branded-identity-emails.md
git commit -m "chore(zitadel): pin the dev image and capture identity mail payloads"
```

---

### Task 2: `[identity_mail]` settings

**Files:**
- Modify: `server/crates/config/src/settings.rs` (new struct, field on `Settings`, validation)
- Modify: `server/config/default.toml`, `server/config/development.toml`
- Modify: `docker/ci/docker-compose.smoke.yml` (`POSTIT__IDENTITY_MAIL__ENABLED: "false"`, because the smoke image runs the baked development config with no vault)
- Modify: `docs/configuration.md` (new `### identity_mail` section under Reference, and the secret listed under "Secrets")
- Test: `server/crates/config/src/loader.rs` tests module

**Interfaces:**
- Produces:

```rust
/// Zitadel's HTTP email provider webhook (plan 03 D0c).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMailSettings {
    pub enabled: bool,
    #[serde(default, deserialize_with = "blank_as_none")]
    pub bind: Option<String>,                 // socket address of the internal listener
    #[serde(default)]
    pub signing_key: Option<RedactedSecret>,  // POSTIT__IDENTITY_MAIL__SIGNING_KEY
    #[serde(with = "crate::duration")]
    pub signature_tolerance: Duration,
    pub max_body_bytes: usize,
}
// Settings gains: pub identity_mail: IdentityMailSettings,
impl IdentityMailSettings {
    /// The bind address, when enabled; validation guarantees it parses.
    pub fn bind_addr(&self) -> Option<std::net::SocketAddr>;
}
```

- default.toml: `[identity_mail] enabled = false`, `signature_tolerance = "5m"`, `max_body_bytes = 65536`.
- development.toml: `[identity_mail] enabled = true`, `bind = "0.0.0.0:44312"`.

- [ ] **Step 1: Write failing tests** in the loader tests module, using the existing `write`/`env_load` helpers:
  - `identity_mail_defaults_hold`: baseline load gives `enabled == false`, `signature_tolerance == 5 min`, `max_body_bytes == 65536`.
  - `identity_mail_enabled_requires_bind`: enabled with no bind gives `ConfigError::Validation` naming `identity_mail.bind`.
  - `identity_mail_bind_must_be_a_socket_address`: `bind = "nope"` names `identity_mail.bind`.
  - `identity_mail_enabled_requires_signing_key`: the error names `identity_mail.signing_key`, mentions `POSTIT__IDENTITY_MAIL__SIGNING_KEY`, and says to run `cargo xtask zitadel-bootstrap`.
  - `identity_mail_tolerance_bounds`: `0s` and `2h` are refused, `1h` is accepted.
  - `identity_mail_body_limit_bounds`: `512` and `2097152` are refused, `1024` and `1048576` are accepted.
  - `identity_mail_signing_key_is_redacted`: `redacted_dump()` does not contain the key value.
- [ ] **Step 2:** Run `cargo test -p postit-config identity_mail`. Expected: FAIL (no field `identity_mail`).
- [ ] **Step 3: Implement** the struct, the `Settings` field, `bind_addr`, and the checks in `Settings::validate`: enabled requires a parseable bind and a non-empty signing key; tolerance is 1 s to 1 h; the body limit is 1024 to 1 048 576 bytes. Then add the TOML sections, the smoke-compose variable, and the docs section (keys, defaults, the secret, and "development only until G1").
- [ ] **Step 4:** Run `cargo test -p postit-config`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(config): add identity_mail settings for the Zitadel email webhook`.

---

### Task 3: One shared email layout, with the approval emails on it

**Files:**
- Create: `server/crates/mail/templates/layout.html`
- Modify: `server/crates/mail/templates/user_approved.html`, `server/crates/mail/templates/user_pending_approval.html` (`{% extends "layout.html" %}`)
- Modify: `server/crates/mail/src/templates.rs` (template structs gain `title`, `pre_header`, `footer`)
- Modify: `server/Cargo.toml` (`insta = "1"` in `[workspace.dependencies]`), `server/crates/mail/Cargo.toml` (`insta.workspace = true` under dev-dependencies)
- Test: `server/crates/mail/tests/templates.rs` plus `server/crates/mail/tests/snapshots/`

**Interfaces:**
- Produces the layout's contract, which Task 4 uses: an askama base template with the variables `lang: &str`, `dir: &str`, `title: &str`, `pre_header: &str`, `footer: &[String]` (one entry per paragraph, empty means no footer row), and `{% block content %}`. It has `.btn` and `.code-box` classes plus `.link-fallback`. A child template provides the block.

- [ ] **Step 1: Write failing tests** in `tests/templates.rs`:
  - `approved_uses_the_shared_layout`: in both `Locale::EnUs` and `Locale::Ar`, the HTML contains `class="main-card"`, the wordmark `post<span style="color:#df528a">it</span>` inside `<bdi dir="ltr">`, `<title>` equal to the subject, and no `class="footer"` (the approved email has no footer text).
  - `pending_footer_is_the_messages_own`: the HTML footer contains `t(locale, "pending-footer")` and does **not** contain "confirm your email address".
  - `approval_text_parts_are_unchanged`: assert the exact plain-text bodies, copied from the current output before the change (record them first by running the current renderer).
  - `layout_mirrors_in_arabic`: the Arabic HTML has `dir="rtl"`, and the layout CSS uses only logical properties (`padding-inline`, `text-align: start`). Assert that the HTML contains no `text-align: left`, `padding-left`, or `padding-right`.
  - `insta::assert_snapshot!` for the approved and pending HTML in both locales (4 snapshots).
- [ ] **Step 2:** Run `cargo test -p postit-mail --test templates`. Expected: FAIL.
- [ ] **Step 3: Implement `layout.html`** from the design reference's base template, with these changes: `lang`/`dir` from variables; `<title>{{ title }}</title>`; a hidden pre-header `<div style="display:none;max-height:0;overflow:hidden">{{ pre_header }}</div>` when it is not empty; the header is always the text wordmark wrapped in `<bdi dir="ltr">`; physical CSS is converted to logical (`padding-inline`, `padding-block`, `text-align: start`, `margin-inline`); the font stack is `'Onest', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Noto Sans Arabic', Helvetica, Arial, sans-serif`; the footer row renders each `footer` entry as a `<p>` and is left out when `footer` is empty. Move the two approval templates onto it, keeping their current content inside `{% block content %}`, and pass `title = subject`, `pre_header = ""`, and `footer = vec![footer]` for pending or `vec![]` for approved.
- [ ] **Step 4:** Run `cargo insta test -p postit-mail --review` (or `INSTA_UPDATE=always`, then review the `.snap` files by eye), then `cargo test -p postit-mail`. Expected: PASS, with snapshots committed.
- [ ] **Step 5:** Commit: `feat(mail): render every email in one branded layout`.

---

### Task 4: Render Zitadel identity messages

**Files:**
- Create: `server/crates/mail/src/identity.rs`, `server/crates/mail/templates/identity.html`, `server/crates/mail/templates/identity.txt`
- Modify: `server/crates/mail/src/lib.rs` (module and re-exports)
- Modify: `server/crates/i18n/locales/en-US/mail.ftl`, `server/crates/i18n/locales/ar/mail.ftl` (key `identity-link-fallback`)
- Test: `server/crates/mail/tests/identity.rs` (uses Task 1's fixtures and `insta`)

**Interfaces:**
- Consumes: the layout from Task 3, and `Rendered` and `MailError` from `templates.rs`/`error.rs`.
- Produces (re-exported from `postit_mail`):

```rust
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityMessage {
    pub context_info: IdentityContext,
    #[serde(default)] pub template_data: IdentityTemplate,
    #[serde(default)] pub args: IdentityArgs,
}
#[derive(Clone, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct IdentityContext { pub event_type: String, pub recipient_email_address: String }
#[derive(Clone, Default, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct IdentityTemplate {
    pub title: Option<String>, pub pre_header: Option<String>, pub subject: Option<String>,
    pub greeting: Option<String>, pub text: Option<String>, pub url: Option<String>,
    pub button_text: Option<String>, pub footer_text: Option<String>,
}
#[derive(Clone, Default, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct IdentityArgs { pub code: Option<String>, pub preferred_language: Option<String> }
// Unknown fields are ignored (no deny_unknown_fields): Zitadel adds fields across versions.
impl fmt::Debug for IdentityMessage  // prints only event_type; never the address, code, URL, or texts
pub fn identity_locale(message: &IdentityMessage) -> Locale;
pub fn render_identity(message: &IdentityMessage) -> Result<Rendered, MailError>;
```

Rules the tests pin:
- **Locale:** `args.preferred_language` (or whatever field Task 1 found) with a primary subtag of `ar` gives `Ar`, and `en` gives `EnUs`. Otherwise `Ar` if more than half of the alphabetic characters in `subject`, `greeting`, and `text` are Arabic script (U+0600–06FF, U+0750–077F, U+08A0–08FF, U+FB50–FDFF, U+FE70–FEFF), else `EnUs`.
- **Subject:** `subject`, else `title`, else `"postit"`. CR and LF are stripped.
- **Title** (H2): `title`, else the subject.
- **Body:** the greeting, then each non-empty line of `text` as its own paragraph. Everything is escaped. If Task 1 found `<br />` in Zitadel's texts, treat `<br>`, `<br/>`, and `<br />` as line breaks before escaping; no other markup survives.
- **Button** when `url` is set and parses as an `http`/`https` URL: `.btn` with `button_text` (else the title), followed by the `identity-link-fallback` line and the raw URL in `dir="ltr"`. Any other scheme (for example `javascript:`) drops the button and the URL entirely.
- **Code box** when `args.code` is set: `<div class="code-box" dir="ltr">`. Both appear when both are set.
- **Footer:** the lines of `footer_text`. If it is missing, there is no footer.
- **Plain text:** title, blank line, greeting, paragraphs, then `"{button_text}: {url}"` and `code` on their own lines, then the footer.

- [ ] **Step 1: Write failing tests** in `tests/identity.rs`:
  - `parses_the_captured_code_payload` / `parses_the_captured_link_payload`: deserialize the fixtures. The event type and recipient match, and `format!("{msg:?}")` contains neither `123456`, nor the URL, nor `ada@example.test`.
  - `link_variant_has_button_and_fallback`: url set, no code. One `class="btn"` with `href` equal to the URL, the fallback text, no `code-box`.
  - `code_variant_has_code_box_only`: code set, no url. `code-box` containing `123456`, no `btn`.
  - `both_variant_has_both`, and `unknown_event_type_renders_generically` (`eventType = "user.human.something.new"` renders title, text, and button).
  - `javascript_url_is_dropped`: `url = "javascript:alert(1)"` gives no `href`, and the string does not appear in the HTML or text.
  - `markup_in_texts_is_escaped`: `greeting = "Hi <b>Ada</b>"` renders as `&lt;b&gt;`, and `text` containing `<script>` renders escaped.
  - `arabic_by_language_and_by_script`: `preferredLanguage = "ar-EG"` gives `dir="rtl"`; no language with Arabic texts gives `dir="rtl"`; no language with English texts gives `dir="ltr"`.
  - `subject_strips_newlines`: `"a\r\nBcc: x"` becomes `"aBcc: x"`.
  - `insta` snapshots: link, code, and both, in English and in Arabic (6). Unknown type in English (1).
- [ ] **Step 2:** Run `cargo test -p postit-mail --test identity`. Expected: FAIL (unresolved import).
- [ ] **Step 3: Implement** `identity.rs` and the two templates (`identity.html` extends `layout.html`). Add `identity-link-fallback = If the button does not work, copy this link into your browser:` and its Arabic equivalent to both `.ftl` files. The existing CI catalog check requires the key in both.
- [ ] **Step 4:** Accept and review the snapshots, then run `cargo test -p postit-mail` and `cargo test -p postit-i18n`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(mail): render Zitadel identity messages in the postit layout`.

---

### Task 5: Verify `ZITADEL-Signature`

**Files:**
- Create: `server/crates/api/src/zitadel_signature.rs`
- Modify: `server/crates/api/src/lib.rs` (`pub mod zitadel_signature;`), `server/Cargo.toml` (`hex = "0.4"` in `[workspace.dependencies]`), `server/crates/api/Cargo.toml` (`hmac`, `sha2`, `hex` with `.workspace = true`)

**Interfaces:**

```rust
pub const HEADER: &str = "zitadel-signature";
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError { Missing, Malformed, Expired, Mismatch }  // each with a short #[error] text
/// Accepts `t=<unix seconds>,v1=<hex>[,v1=<hex>...]`, whitespace around parts allowed.
/// Valid when |now - t| <= tolerance and any v1 equals HMAC-SHA256(key, "{t}.{body}"),
/// compared in constant time (`Mac::verify_slice`).
pub fn verify(header: Option<&str>, body: &[u8], key: &[u8], now: DateTime<Utc>, tolerance: Duration)
    -> Result<(), SignatureError>;
```

- [ ] **Step 1: Write failing unit tests** in the module (sign with a test helper that builds the header from key, timestamp, and body):
  - `accepts_a_valid_signature`;
  - `missing_header` gives `Missing`; `"garbage"`, `"t=abc,v1=00"`, and `"t=1"` (no v1) give `Malformed`;
  - `wrong_key` and `tampered_body` give `Mismatch`;
  - `stale_and_future_timestamps`: `now - 301 s` and `now + 301 s` with a 300 s tolerance give `Expired`, and `now ± 300 s` is accepted;
  - `any_of_several_v1_matches`: `t=..,v1=<wrong>,v1=<right>` is accepted;
  - `tolerates_spaces`: `"t=..., v1=..."` is accepted;
  - `odd_length_or_non_hex_v1` gives `Malformed`.
- [ ] **Step 2:** Run `cargo test -p postit-api --lib zitadel_signature`. Expected: FAIL.
- [ ] **Step 3: Implement** `verify`.
- [ ] **Step 4:** Run the same command. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(api): verify Zitadel webhook signatures`.

---

### Task 6: The `POST /internal/identity-mail` webhook

**Files:**
- Create: `server/crates/api/src/internal.rs`
- Modify: `server/crates/api/src/lib.rs` (`pub mod internal; pub use internal::{IdentityMailState, internal_router};`)
- Test: `server/crates/api/tests/identity_mail.rs`, `server/crates/api/tests/identity_mail_leak.rs` (its own binary, because it captures logs)

**Interfaces:**
- Consumes: `zitadel_signature::verify` (Task 5), `postit_mail::{IdentityMessage, render_identity, Mailer, RenderedMessage, MailError}` (Task 4), `postit_core::Clock`.
- Produces:

```rust
#[derive(Clone)]
pub struct IdentityMailState {
    pub mailer: Arc<dyn Mailer>,
    pub signing_key: RedactedSecret,
    pub clock: Arc<dyn Clock>,
    pub tolerance: Duration,
    pub max_body_bytes: usize,
}
/// Only `POST /internal/identity-mail`. No CORS, no auth extractor, no TraceLayer (the
/// request is never logged); the signature header is marked sensitive.
pub fn internal_router(state: IdentityMailState) -> Router;
```

Handler order and answers, with empty bodies throughout: read the body up to `max_body_bytes` (over the limit gives **413**). A bad or missing signature gives **401**. JSON that does not parse gives **400**. A recipient that does not parse as a `lettre::message::Mailbox`, or that contains a display name, CR, or LF, gives **400**. Rendering fails with **500**. Then `mailer.send(...)`: `Ok` gives **204**, `MailError::Transient` gives **503**, and `MailError::Permanent` gives **502**. Each outcome logs one `tracing::info!`/`warn!` with `event_type` and the status only.

- [ ] **Step 1: Write failing tests** in `tests/identity_mail.rs`, driving `internal_router(...)` with `tower::ServiceExt::oneshot`, a `TestClock`, and `MemoryMailer`. Add a small `FailingMailer(MailError kind)` in the test file:
  - `signed_code_payload_sends_and_answers_204`: the Task 1 fixture, signed. One message is sent to `ada@example.test`, with a subject from the fixture and `123456` in both its HTML and its text.
  - `missing_wrong_and_stale_signatures_answer_401_and_send_nothing`.
  - `oversized_body_answers_413`: `max_body_bytes + 1` bytes.
  - `malformed_json_answers_400`, `missing_recipient_answers_400`.
  - `crlf_or_display_name_recipient_answers_400`: `"a@b.c\r\nBcc: x@y.z"` and `"Ada <a@b.c>"`.
  - `transient_smtp_failure_answers_503` and `permanent_smtp_failure_answers_502`.
  - `get_is_405_and_other_paths_404`.
  - `public_api_router_does_not_serve_it`: `api_router(testkit state)` answers 404 to a signed `POST /internal/identity-mail`. Use the existing testkit builder in `postit_api::testkit`.
- [ ] **Step 2:** Write `tests/identity_mail_leak.rs`: install `LogCapture`, send a signed fixture through a `FailingMailer(Permanent)` and through `MemoryMailer`, then `assert_no_leak(&["123456", "ada@example.test", "ABCDEF", <signing key>], &[("logs", &logs)])`. Also assert that the logs contain the event type, which shows the capture works.
- [ ] **Step 3:** Run `cargo test -p postit-api --test identity_mail --test identity_mail_leak`. Expected: FAIL.
- [ ] **Step 4: Implement** `internal.rs`. Read the body with `axum::body::to_bytes(body, max_body_bytes)`, which maps the length error to 413. Wrap the router in `SetSensitiveRequestHeadersLayer` for `zitadel-signature`.
- [ ] **Step 5:** Run the tests. Expected: PASS. Confirm that `cargo xtask openapi --check` is unchanged, because the route is not in the OpenAPI document.
- [ ] **Step 6:** Commit: `feat(api): accept Zitadel identity mail on an internal webhook`.

---

### Task 7: Serve the internal listener from the API role

**Files:**
- Modify: `server/crates/server/src/lib.rs` (`StartOptions.internal_listener: Option<TcpListener>`, `RunningServer.internal_addr: Option<SocketAddr>`, bind in `prepare` when `role.runs_api() && identity_mail.enabled`, serve plain HTTP in `start`)
- Modify: `server/crates/server/src/compose.rs` (`pub fn mailer(settings: &Settings) -> anyhow::Result<Arc<dyn Mailer>>`, used by both `registry` (replacing the inline `SmtpMailer::new` at line ~222) and the internal router)
- Modify: every `StartOptions { .. }` literal (12 sites: `grep -rn "StartOptions {" server/`) to add `internal_listener: None`
- Modify: `docker/docker-compose.development.yml` (`postit-server`: `POSTIT__IDENTITY_MAIL__BIND: "0.0.0.0:44312"`, and `ports: ["44312:44312"]` with a comment saying Zitadel reaches it through `host.docker.internal` and nginx does not proxy it)
- Test: `server/crates/server/tests/smoke.rs` (new test)

**Interfaces:**
- Consumes: `IdentityMailSettings::bind_addr` (Task 2), and `internal_router`/`IdentityMailState` (Task 6).

- [ ] **Step 1: Write the failing test** `identity_mail_listener_is_separate_from_the_api`. Use the smoke helper `config_dir(&issuer, extra)` with `extra = "[identity_mail]\nenabled = true\nbind = \"127.0.0.1:0\""` and `POSTIT__IDENTITY_MAIL__SIGNING_KEY` in the `load_with` vars, and pass `internal_listener: Some(TcpListener::bind("127.0.0.1:0"))`. Assert that `internal_addr` is `Some`, that a signed POST of the fixture to `internal_addr` does **not** answer 401 (any of 204/503 is fine, because there is no SMTP), and that the same POST to `api_addr` answers 404. Add a second test, `identity_mail_disabled_binds_nothing`, where `internal_addr` is `None`.
- [ ] **Step 2:** Run `cargo test -p postit-server --test smoke identity_mail`. Expected: FAIL.
- [ ] **Step 3: Implement.** Mirror the existing `web_listener` handling: bind failures name `identity_mail.bind`, and `spawn_listeners` serves it with `tls: None`. In `run_from_env`, include `internal = ?running.internal_addr` in the "listening" log.
- [ ] **Step 4:** Run `cargo test -p postit-server`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(server): serve the identity mail webhook on its own listener`.

---

### Task 8: postit's message texts for Zitadel

**Files:**
- Create: `docker/zitadel/messages/en.yaml`, `docker/zitadel/messages/ar.yaml`
- Create: `server/xtask/src/zitadel_messages.rs` (loader and request builder, no network)
- Modify: `server/xtask/src/main.rs` (`mod zitadel_messages;`), `server/Cargo.toml` (`serde_norway = "0.9"` and `regex` if absent, in `[workspace.dependencies]`), `server/xtask/Cargo.toml` (both with `.workspace = true`)

**Interfaces:**

```rust
/// Zitadel message types, in the path spelling Task 1 confirmed.
pub const MESSAGE_TYPES: [&str; 8] = ["init", "verifyemail", "verifyemailotp", "passwordreset",
    "passwordchange", "passwordless_registration", "invite_user", "domainclaimed"];
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageText { pub title: String, pub pre_header: String, pub subject: String,
    pub greeting: String, pub text: String, pub button_text: String, pub footer_text: String }
/// `{type -> MessageText}` for one language file.
pub fn load(path: &Path) -> anyhow::Result<BTreeMap<String, MessageText>>;
/// (path, body) for each PUT: `/admin/v1/text/message/{type}/{language}`.
pub fn requests(language: &str, texts: &BTreeMap<String, MessageText>) -> Vec<(String, serde_json::Value)>;
```

YAML shape: a top-level key per message type, then the seven fields. Zitadel variables are written as Go templates: `{{.DisplayName}}`, `{{.PreferredLoginName}}`, `{{.Code}}`, `{{.URL}}` (only those four).

English copy, in postit's voice (plain, warm, short, with no "Dear", no exclamation marks, and "you" addressed). The `text` field is written by the implementer in the same voice and is two sentences at most:

| type | subject | buttonText |
|---|---|---|
| init | Finish setting up your postit account | Set up my account |
| verifyemail | Confirm your email for postit | Confirm email |
| verifyemailotp | Your postit code: {{.Code}} | (empty) |
| passwordreset | Reset your postit password | Reset password |
| passwordchange | Your postit password was changed | Sign in |
| passwordless_registration | Add a passkey to postit | Add passkey |
| invite_user | You're invited to postit | Accept invite |
| domainclaimed | Your postit sign-in name is changing | Sign in |

Every `greeting` is `Hello {{.DisplayName}},` (Arabic: `مرحبًا {{.DisplayName}}،`). Every `footerText` says the email was sent by postit because of an action on this address, and that it can be ignored if that action was not theirs. The password-change footer instead says to reset the password at once if the change was not theirs.

- [ ] **Step 1: Write failing tests** in `zitadel_messages.rs`:
  - `both_languages_cover_every_type`: `load` of `../docker/zitadel/messages/{en,ar}.yaml` has exactly `MESSAGE_TYPES` as keys.
  - `only_known_variables`: every field of every text uses only `{{.DisplayName}}`, `{{.PreferredLoginName}}`, `{{.Code}}`, and `{{.URL}}`. A regex `\{\{[^}]*\}\}` finds the rest; the test fails on any other.
  - `arabic_texts_are_arabic`: every non-empty `ar` field has Arabic-script letters, and no `ar` field equals its `en` counterpart.
  - `requests_build_admin_paths`: `requests("ar", ..)` has the path `/admin/v1/text/message/verifyemail/ar` and a camelCase body with `buttonText`.
  - `unknown_field_is_rejected`: a temp YAML with `buton: x` fails `load`.
- [ ] **Step 2:** Run `cargo test -p xtask zitadel_messages`. Expected: FAIL.
- [ ] **Step 3: Write** both YAML files and implement the module.
- [ ] **Step 4:** Run the same command. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(zitadel): add postit's English and Arabic identity message texts`.

---

### Task 9: Bootstrap the HTTP provider, texts, and branding

**Files:**
- Modify: `server/xtask/src/zitadel_bootstrap.rs`
- Create: `docker/zitadel/branding/` with the font Task 1 found Zitadel accepts. Onest from `github.com/google/fonts/tree/main/ofl/onest` (the static Regular `.ttf`) plus its `OFL.txt`, with the source URL in a `README.md`. The logo is read from `web/public/postit.svg` and not copied.

**Interfaces:**
- Consumes: `zitadel_messages::{load, requests}` (Task 8).
- Produces (pure functions, unit-tested):

```rust
const IDENTITY_MAIL_DESCRIPTION: &str = "postit: identity mail webhook";
const DEFAULT_IDENTITY_MAIL_URL: &str = "http://host.docker.internal:44312/internal/identity-mail";
const VAULT_POSTIT_ENV: &str = "../!ref/vault/development/postit.env";
const SIGNING_KEY_VAR: &str = "POSTIT__IDENTITY_MAIL__SIGNING_KEY";

#[derive(Debug, PartialEq, Eq)]
enum ProviderStep {
    Create,                       // POST /admin/v1/email/http -> signingKey
    UpdateEndpoint(String),       // PUT /admin/v1/email/http/{id}, no rotation
    RotateKey(String),            // PUT ... with "expirationSigningKey": "0s" -> signingKey
    Activate(String),             // POST /admin/v1/email/{id}/_activate
    KeepOperatorProvider,         // an active provider that is neither of ours: change nothing, warn
}
/// Steps in order, given the `_search` result, whether the vault already has the key, and the endpoint.
fn provider_plan(list: &Value, vault_has_key: bool, endpoint: &str) -> Vec<ProviderStep>;
/// Replaces or appends `KEY=value` (single-quoted) in a dotenv file; other lines and their order kept.
fn upsert_env_value(path: &Path, key: &str, value: &str) -> anyhow::Result<()>;
fn label_policy_body() -> Value;  // primaryColor #df528a, backgroundColor #f8fafc, fontColor #1f1e24, warnColor #b91c1c, plus the dark-mode fields
```

`ensure_smtp_provider` changes: the SMTP provider is created if missing but **no longer activated**. It stays as the inactive fallback, and `smtp_step`'s `Activate` branch goes away. Run order in `run()`: `ensure_smtp_provider`, then `ensure_message_texts` (en, ar), then `ensure_identity_mail_provider`, then `ensure_branding`. A new `--check` argument (parsed in `main.rs`) only lists the providers and texts and exits non-zero, with a one-line reason, when ours is not the active provider.

- [ ] **Step 1: Write failing unit tests**:
  - `provider_plan` (the caller always activates the id that `Create` returns, so `Create` is never followed by `Activate` in the plan): no providers gives `[Create]`; ours inactive, key in vault, same endpoint gives `[Activate(id)]`; ours active, key in vault, same endpoint gives `[]`; ours active with a different endpoint gives `[UpdateEndpoint(id)]`; ours active, key missing from the vault gives `[RotateKey(id)]`; our SMTP active gives the steps for ours plus `Activate`; an operator provider active gives `[KeepOperatorProvider]`.
  - `upsert_env_value`: it replaces an existing line, appends to a file with no trailing newline, keeps comments and order, creates nothing when the file is missing (error naming the path and "extract !ref/vault.7z"), and quotes the value.
  - `label_policy_body_uses_brand_colors`.
  - Update `smtp_step` tests for the no-activation rule.
- [ ] **Step 2:** Run `cargo test -p xtask`. Expected: FAIL.
- [ ] **Step 3: Implement** the planner, `ensure_identity_mail_provider` (it writes the key with `upsert_env_value` whenever Create or RotateKey returns one, then prints "Wrote POSTIT__IDENTITY_MAIL__SIGNING_KEY to !ref/vault/development/postit.env; restart postit and re-pack vault.7z"), and `ensure_message_texts`, which treats Zitadel's "not changed" answer as success the same way `ensure_login_policy` does. Implement `ensure_branding` as: PUT the label policy (with the same not-changed handling), upload the logo and font as multipart (`reqwest::multipart`; the workspace already enables the feature), then `_activate`. Also implement `--check`. The endpoint comes from `ZITADEL_IDENTITY_MAIL_URL`, or the default when it is unset.
- [ ] **Step 4:** Run `cargo test -p xtask`. Expected: PASS. Then, against the dev stack, run `cargo xtask zitadel-bootstrap` twice. The second run must print no "Wrote" line and make no create, rotate, or activate call (log each call to stdout so this can be seen). `cargo xtask zitadel-bootstrap --check` must exit 0.
- [ ] **Step 5:** Commit: `feat(xtask): bootstrap Zitadel's postit mail webhook, texts, and branding`.

---

### Task 10: `stack doctor` checks the development signing key

**Files:**
- Modify: `stack.sh` (`doctor`), `stack.ps1` (`Invoke-Doctor`)
- Test: `docker/tests/stack-doctor.sh` (and the PowerShell test, if one exists next to it)

- [ ] **Step 1: Write failing cases** in `stack-doctor.sh` with its `expect` helper:
  - development `postit.env` without `POSTIT__IDENTITY_MAIL__SIGNING_KEY` fails, naming the key and "cargo xtask zitadel-bootstrap";
  - with the key set, it passes;
  - qa without it passes (G1 owns qa and production).
- [ ] **Step 2:** Run `bash docker/tests/stack-doctor.sh`. Expected: FAIL.
- [ ] **Step 3: Implement** the check in both launchers. The value is never printed.
- [ ] **Step 4:** Run the tests in Git Bash and, for `stack.ps1`, `./stack.ps1 doctor` against a temporary vault dir through `POSTIT_VAULT_DIR`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(stack): doctor checks the identity mail signing key in development`.

---

### Task 11: Documentation

**Files:**
- Modify: `docs/deployment.md`, in "Identity provider" under the Zitadel production setup: a numbered step list to create the HTTP email provider at `<internal URL>/internal/identity-mail`, store its `signingKey` as `POSTIT__IDENTITY_MAIL__SIGNING_KEY`, set `identity_mail.enabled`/`bind`, keep an inactive SMTP provider as the fallback, apply `docker/zitadel/messages/*.yaml`, and set the branding. Add a note that the listener must be reachable only from Zitadel's network and must never be proxied publicly. Mark it as the G1 checklist.
- Modify: `docs/security.md` (threat model: the internal listener, signature and replay window, no storage of links or codes, and the duplicate on retry)
- Modify: `CLAUDE.md` ("Project state": D0c built), and `!ref/plans/03. publishing roadmap.md` (D0c: note the two decisions above, signature instead of token and the doctor/`--check` split, so the plan and the code agree)
- Modify: `README.md` dev setup, if it describes the bootstrap output (re-pack the vault after the first bootstrap)

- [ ] **Step 1:** Write the docs.
- [ ] **Step 2:** Commit: `docs: document identity mail in deployment, security, and the plan`.

---

### Task 12: Exit-criteria run in development

- [ ] **Step 1:** Run the quality gates from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, and `cargo test --workspace`. Also run `cargo xtask openapi --check`. Expected: all pass.
- [ ] **Step 2:** `./stack.ps1 up`, then `cargo xtask zitadel-bootstrap`, then start postit (`cargo run` from `server/`), with Papercut running. In the browser (Claude in Chrome), on `https://postit.local:44315`: sign up a new user with an English profile, confirm the email **by link**, then by **code** (the resend-code path), and reset the password. Repeat with the Zitadel profile language set to Arabic (or `ui_locales=ar`). Every email arrives in Papercut in the postit layout. The Arabic ones are right to left with an LTR code and link.
- [ ] **Step 3:** Re-run `cargo xtask zitadel-bootstrap`: no changes. Then `cargo xtask zitadel-bootstrap --check`: exit 0.
- [ ] **Step 4:** Check that postit stored and logged nothing: grep the server's stdout for the code and link seen in Papercut (no match), and run `SELECT count(*) FROM jobs WHERE created_at > <run start>` to confirm no mail job was enqueued for these sends.
- [ ] **Step 5:** Stop postit, trigger a code email, and confirm Zitadel reports the delivery failure and retries, then delivers after postit is back. This proves that non-2xx answers lead to a retry.
- [ ] **Step 6:** Fix every issue found, minor ones included. Anything truly blocked goes into `docs/superpowers/plans/2026-10-24-d0c-branded-identity-emails-deferred.md` (issue, why, trigger) and is committed.

## Phase-start findings

(Task 1 fills this in.)
