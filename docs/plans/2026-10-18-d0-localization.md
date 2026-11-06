# D0: Localization Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every existing web screen, mobile screen, server email, and API error translatable and right-to-left capable, ship `en-US`, store the user's language in `user_preferences.locale`, and fail CI when a catalog misses a key, so a later language (right-to-left or not) is a catalog plus one registration.

**Architecture:** Server: a new leaf crate `postit-i18n` (Project Fluent) owns locale negotiation and server catalogs; `postit-data` stores `locale`; `postit-identity` sets it at first sign-in from the client's `Accept-Language`; `postit-mail` renders the two plan 02 emails per recipient locale; `postit-api` adds `GET/PUT /me/preferences` (locale only, pending users allowed). Web: i18next + react-i18next with bundled JSON catalogs, one locale table naming each language's direction and formatting locale, `lang`/`dir` on `<html>`, logical CSS, and a formatting hook. Mobile: gen-l10n ARB catalogs, locale from a Riverpod provider, directional layouts. Both clients send `Accept-Language` with the language on screen and `ui_locales` at sign-in.

**Tech Stack:** Rust (`fluent-bundle`, `fluent-langneg`, `unic-langid`, askama, sqlx, axum, utoipa), React 19 (`i18next`, `react-i18next`, `i18next-browser-languagedetector`, Vitest, Playwright), Flutter 3.47.6 (`flutter_localizations`, `intl`, gen-l10n, Riverpod).

**Spec:** `docs/specs/2026-10-18-d0-localization-design.md` (from `docs/contracts/03. publishing roadmap.md`, phase D0 and "Localization"). Read both.

## Global Constraints

- Supported locales, exactly: `en-US` (default, reference catalog). The server stores and accepts only supported tags; clients map any tag to a supported locale by its primary language subtag, else `en-US`. Every locale names its writing direction in one place per codebase (`postit_i18n::Locale::is_rtl`, `web/src/i18n/locales.ts`, Flutter's localizations), and everything that sets `dir` reads it from there.
- Every formatted quantity (counts, dates, times, durations, pager numbers, chart axes) goes through the locale's formatter. Verbatim identifiers and technical values (job, request, and user IDs, cron expressions, HTTP status codes, error codes, raw JSON payloads) stay as-is, isolated with `dir="ltr"` (web: `<bdi dir="ltr">` or `<code dir="ltr">`; Flutter: `Directionality(textDirection: TextDirection.ltr)`) so they never reorder inside right-to-left text.
- User content (display names, emails, later post text) renders with automatic direction (web `dir="auto"`; Flutter direction from the text).
- problem+json `detail` stays English. Swagger UI is not translated.
- Times display in the device's time zone (no time-zone preference in D0).
- JS tooling through pnpm only, never npm; on Windows run pnpm and Flutter through PowerShell (the Git Bash pnpm shim is broken). Exact version pins in `web/package.json`.
- Rust dependencies at workspace level; no dependency with an Arabic or Arabic-derived name.
- One migration file: edit `backend/crates/data/migrations/0001_init.sql` in place, then reset the dev database (`./stack.ps1` per the `stack` skill) and refresh the sqlx offline cache (`cargo sqlx prepare --workspace -- --all-targets` from `backend/`).
- After any API change run `cargo xtask openapi` from `backend/` (regenerates `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`).
- Gates per area: Rust from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`. Web from `web/`: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`. App from `mobile/`: `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test`. Contract: `cargo xtask openapi --check`.
- Catalog text is plain and short; brand name `postit` stays Latin.
- Commits: Conventional Commits (scopes `i18n`, `data`, `identity`, `mail`, `api`, `web`, `app`, `ci`, `docs`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **The language on screen reaches the server**: both clients send `Accept-Language` with the on-screen language on every request, so the locale stored at first sign-in is the one the user saw. Task 6 (web) and Task 10 (app) test the header; Task 3 tests provisioning stores it.
2. **`Accept-Language: fr-CA, en;q=0.5`, an empty header, or garbage**: negotiate to `en-US`, never fail the request. Task 1 tests these.
3. **Mixed-direction strings** (a right-to-left display name or email inside an English sentence, a job ID in a row): must not reorder punctuation or digits. Tasks 7–9 test `dir="auto"`/`dir="ltr"` on those elements; Task 11 for the app.
4. **A pending user changes language**: `PUT /me/preferences` must succeed for `pending` and fail for `disabled`; the later `user_approved` email uses the stored locale. Task 5 and Task 4 test both.
5. **An error code the client has no translation for** (a future server code): falls back to the generic translated message, never shows the raw code or the English `detail`. Task 6 and Task 10 test it.

---

### Task 1: `postit-i18n` crate

**Files:**
- Create: `backend/crates/i18n/Cargo.toml`, `backend/crates/i18n/src/lib.rs`, `backend/crates/i18n/src/locale.rs`, `backend/crates/i18n/src/catalog.rs`, `backend/crates/i18n/locales/en-US/mail.ftl`
- Modify: `backend/Cargo.toml` (members, workspace deps `fluent-bundle = "0.16"`, `fluent-langneg = "0.14"`, `unic-langid = "0.9"`, `postit-i18n = { path = "crates/i18n" }`; check crates.io for the current minor at phase start and pin that)

**Interfaces:**
- Produces:
  - `enum Locale { EnUs }` (`Copy`, `Default = EnUs`), `Locale::ALL: [Locale; 1]`, `tag(self) -> &'static str` (`"en-US"`), `language(self) -> &'static str` (`"en"`, the subtag negotiation matches), `from_tag(&str) -> Option<Locale>` (exact tags only), `is_rtl(self) -> bool` (each locale states its direction; `EnUs` is `false`), `dir(self) -> &'static str` (`"ltr"`/`"rtl"` from `is_rtl`), `negotiate(accept_language: Option<&str>) -> Locale`.
  - `struct Args` builder: `Args::new()`, `.str(name: &'static str, value: impl Into<String>)`, `.num(name: &'static str, value: i64)`.
  - `fn t(locale: Locale, id: &str, args: &Args) -> String` (falls back to `en-US` when the id is missing in `locale`, then to the id itself; never panics).
  - `fn message_ids(locale: Locale) -> BTreeSet<String>` (for the completeness test and other crates' tests).

**Approach:** `fluent_bundle::concurrent::FluentBundle` per locale in a `std::sync::LazyLock`, resources embedded with `include_str!`. Keep Unicode isolation on (`use_isolating = true`) so mixed-direction arguments render correctly. `negotiate` parses the header and picks the most preferred entry whose language subtag is a supported locale's `language()`, else `EnUs`.

**Mail messages** (the catalog defines exactly these ids): `pending-subject` (`$count`, plural), `pending-intro` (`$count`), `pending-more` (`$more`), `pending-review` (link text), `pending-footer`, `approved-subject`, `approved-greeting` (`$name`), `approved-body`, `approved-sign-in` (link text). Base the English text on the current templates in `backend/crates/mail/templates/` and `templates.rs` (`"{total} postit account(s) waiting for approval"` becomes a proper plural).

- [ ] **Step 1: Write failing tests** in `lib.rs` tests:

```rust
#[test] fn negotiate_table() {
    for (header, want) in [(Some("en-GB"), Locale::EnUs), (Some("fr-CA, en;q=0.5"), Locale::EnUs), (Some("en;q=0"), Locale::EnUs),
                           (Some(""), Locale::EnUs), (Some("%%%"), Locale::EnUs), (None, Locale::EnUs)] {
        assert_eq!(negotiate(header), want, "{header:?}");
    }
}
#[test] fn catalogs_have_identical_ids() { for l in Locale::ALL { assert_eq!(message_ids(l), message_ids(Locale::EnUs)); } }
#[test] fn english_plural() { assert_eq!(strip_isolation(&t(Locale::EnUs, "pending-subject", &Args::new().num("count", 1))), "1 postit account is waiting for approval"); }
#[test] fn missing_id_falls_back() { assert_eq!(t(Locale::EnUs, "no-such-id", &Args::new()), "no-such-id"); }
#[test] fn tags_round_trip() { for l in Locale::ALL { assert_eq!(Locale::from_tag(l.tag()), Some(l)); } assert_eq!(Locale::from_tag("en"), None); }
#[test] fn dir_follows_the_direction() { for l in Locale::ALL { assert_eq!(l.dir(), if l.is_rtl() { "rtl" } else { "ltr" }); } }
```

  (`strip_isolation` removes U+2068/U+2069; define it in the test module. Also expose it as `pub fn strip_isolation(&str) -> String` under `#[cfg(any(test, feature = "testkit"))]` with a `testkit` feature, for Task 4's tests.)
- [ ] **Step 2: Run** `cargo test -p postit-i18n` from `backend/`. Expected: compile failure.
- [ ] **Step 3: Implement** the crate and the `.ftl` file.
- [ ] **Step 4: Run** tests, clippy, and `cargo deny check` (licenses of the new crates are MIT/Apache). Expected: PASS.
- [ ] **Step 5: Commit** `feat(i18n): add the postit-i18n crate with the en-US catalog`.

---

### Task 2: `user_preferences.locale` in the data layer

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql:44-50`, `backend/crates/data/src/preferences.rs`, `backend/crates/data/tests/preferences.rs`, `backend/.sqlx/` (regenerated)

**Interfaces:**
- Produces: `UserPreferences.locale: String`; `UserPreferencesRepo::create_default(conn, user_id: UserId, timezone: &str, locale: &str) -> Result<UserPreferences, DataError>` (new `locale` parameter); `UserPreferencesRepo::set_locale(conn, user_id: UserId, locale: &str) -> Result<UserPreferences, DataError>` (returns `DataError::NotFound` or the crate's existing not-found variant when no row).

- [ ] **Step 1: Write failing tests** in `data/tests/preferences.rs`: `create_default_stores_locale` (create with `"fr"`; `get` returns `locale == "fr"`), `set_locale_updates_only_locale` (timezone unchanged), `set_locale_missing_user` (not-found error). Update existing `create_default` calls to pass `"en-US"`.
- [ ] **Step 2: Run** `cargo test -p postit-data --test preferences` (needs the dev Postgres). Expected: compile failure.
- [ ] **Step 3: Implement.** Column: `locale TEXT NOT NULL DEFAULT 'en-US'`. Reset the dev database, then refresh the sqlx offline cache.
- [ ] **Step 4: Run** the tests. Expected: PASS. Fix every other `create_default` caller (`identity/src/claims.rs:207`, `identity/tests/approval_emails.rs:40`) to pass `"en-US"` for now; Task 3 replaces the identity one.
- [ ] **Step 5: Commit** `feat(data): store the user's locale in user_preferences`.

---

### Task 3: Locale at first sign-in

**Files:**
- Modify: `backend/crates/identity/Cargo.toml` (+ `postit-i18n`), `backend/crates/identity/src/auth.rs:73` (trait and impl), `backend/crates/identity/src/claims.rs` (`transform`, `on_created`), `backend/crates/api/src/extract.rs:89-140` (`Auth`), every `Authenticate` test fake (find with `grep -rn "impl Authenticate" server`)
- Test: `backend/crates/identity/tests/` (the existing provisioning test file), `backend/crates/api/tests/` (existing auth extractor tests)

**Interfaces:**
- Consumes: `postit_i18n::{Locale, negotiate}`; `create_default(.., locale)` (Task 2).
- Produces: `Authenticate::authenticate(&self, bearer: &SecretString, gate: &dyn ProvisionGate, ctx: AuditContext, locale: Locale) -> Result<Principal, AuthError>`; `ClaimsTransformer::transform(&self, verified, bearer_token, ctx, locale: Locale)`. `locale` is used only when the user row is created; an existing user's stored locale never changes on sign-in. The `Auth` extractor computes `negotiate(parts.headers.get(ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()))`.

- [ ] **Step 1: Write failing tests**: identity `first_sign_in_stores_locale` (transform a new subject with `Locale::EnUs`; preferences row has `"en-US"`), `later_sign_in_keeps_stored_locale` (a locale stored since the first sign-in survives a second transform); api `first_request_stores_negotiated_locale` (the api testkit runs the real `Authenticator` over the test database with its `TestIssuer`: `GET /api/v1/me` with a token for a new subject and `Accept-Language: en-GB`, then read `user_preferences.locale` from `TestApp.pool` and assert `"en-US"`; a second new subject without the header gets `"en-US"`).
- [ ] **Step 2: Run** `cargo test -p postit-identity -p postit-api`. Expected: compile failure.
- [ ] **Step 3: Implement** and update all fakes and call sites.
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(identity): set the locale from Accept-Language at first sign-in`.

---

### Task 4: Localized approval emails

**Files:**
- Modify: `backend/crates/mail/Cargo.toml` (+ `postit-i18n`, dev `postit-i18n/testkit`), `backend/crates/mail/src/templates.rs`, `backend/crates/mail/src/send.rs:169-190`, `backend/crates/mail/templates/*.html`, `*.txt`
- Test: `backend/crates/mail/src/templates.rs` tests, `backend/crates/identity/tests/approval_emails.rs`

**Interfaces:**
- Consumes: `postit_i18n::{t, Args, Locale}`; `UserPreferencesRepo::get`.
- Produces: `render(content: &MailContent, app_url: &Url, locale: Locale) -> Result<Rendered, MailError>`. Templates receive only pre-translated strings plus `lang` (`locale.tag()`) and `dir` (`locale.dir()`); HTML templates set `<html lang="{{ lang }}" dir="{{ dir }}">`; user-supplied values (display names, emails) are wrapped in `<bdi>` in HTML. `send.rs` loads the recipient's preferences in the same transaction with `UserPreferencesRepo::get` and uses `Locale::from_tag(&prefs.locale)`, falling back to `Locale::EnUs` when the row is missing or the tag is unknown (never an error: the email still goes out).

- [ ] **Step 1: Write failing tests**: `renders_pending_in_english_with_plural` (2 users + `more: 3` gives subject `"5 postit accounts are waiting for approval"`), `display_name_is_isolated` (HTML contains `<bdi>Ada</bdi>`), `layout_uses_logical_properties` (for every locale, `dir` is the locale's and no physical `left`/`right` property appears); in `approval_emails.rs`: `approved_email_uses_recipient_locale` (recipient prefs `"en-US"`, captured mail subject is that catalog's), `missing_preferences_sends_english` (recipient with no preferences row: the email is sent in `en-US`).
- [ ] **Step 2: Run** `cargo test -p postit-mail -p postit-identity`. Expected: compile failure.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(mail): render approval emails in the recipient's language`.

---

### Task 5: `GET/PUT /me/preferences`

**Files:**
- Modify: `backend/crates/api/src/routes/me.rs`, `backend/crates/api/src/dto.rs`, `backend/crates/api/src/router.rs`, `backend/crates/api/src/openapi.rs`, `backend/crates/api/Cargo.toml` (+ `postit-i18n`)
- Regenerate: `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/` (`cargo xtask openapi`)
- Test: `backend/crates/api/tests/` (new file `preferences.rs`, following the existing `me` route tests)

**Interfaces:**
- Consumes: `Auth` extractor (pending allowed, disabled rejected), `UserPreferencesRepo::{get, set_locale}`, `Locale::from_tag`.
- Produces: `GET /api/v1/me/preferences` returns `PreferencesDto { locale: LocaleDto }`; `PUT /api/v1/me/preferences` with `UpdatePreferences { locale: String }` returns the updated `PreferencesDto`. `LocaleDto` is a utoipa string enum with the value `"en-US"` (so the generated TS and Dart types are closed; a later language adds a value). An unsupported value is 422 `validation_failed`. Tag `me`, security `oidc`. No act-as (self route).

- [ ] **Step 1: Write failing tests**: `get_returns_default_locale` (`"en-US"`), `put_sets_locale_and_get_reflects_it`, `put_rejects_unsupported_locale` (`"fr"` and `"en"` give 422 with code `validation_failed`), `pending_user_can_read_and_change` (pending principal: 200 on both), `disabled_user_is_rejected` (403 `account_disabled`), `users_only_see_their_own` (user A's change never reaches user B's row), `act_as_header_does_not_reach_another_user` (with `X-Postit-Act-As` naming another user, the route behaves exactly as `GET /me` does with that header today: check `routes/me.rs` and mirror it; it never reads or writes the other user's row), `unauthenticated_is_401`.
- [ ] **Step 2: Run** `cargo test -p postit-api --test preferences`. Expected: FAIL (404s).
- [ ] **Step 3: Implement** the handlers and register them in the router and OpenAPI document.
- [ ] **Step 4: Run** tests, clippy, then `cargo xtask openapi` and `cargo xtask openapi --check`. Expected: PASS; regenerated files committed.
- [ ] **Step 5: Error-code audit.** Run `grep -rn "ApiError::new\|ErrorCode::" backend/crates/api/src` and confirm every error response is built from an `ErrorCode` (no free-text-only errors). Record the result in the commit body (list of the 18 codes). Expected: no code changes needed; fix any path that is not.
- [ ] **Step 6: Commit** `feat(api): add GET/PUT /me/preferences with the locale`.

---

### Task 6: Web i18n foundation

**Files:**
- Modify: `web/package.json` (+ `i18next`, `react-i18next`, `i18next-browser-languagedetector`, exact current versions via `pnpm add -E`), `web/src/main.tsx`, `web/src/index.css:4-14`, `web/src/test/setup.ts`, `web/src/api/authFetch.ts`, `web/src/auth/SessionProvider.tsx:53,99`, `web/src/lib/format.ts`
- Create: `web/src/i18n/index.ts`, `web/src/i18n/locales.ts`, `web/src/i18n/i18next.d.ts`, `web/src/i18n/useFormat.ts`, `web/src/i18n/LocaleSync.tsx`, `web/src/i18n/LanguageSwitch.tsx`, `web/src/i18n/en-US/common.json`
- Test: `web/src/i18n/i18n.test.tsx`, `web/src/i18n/catalogs.test.ts`, `web/src/lib/format.test.ts`

**Interfaces:**
- Produces:
  - `locales.ts`: one table of supported languages, each with its primary language subtag, `Intl` formatting locale, and direction; `type AppLocale` (its keys, `'en-US'`); `SUPPORTED: readonly AppLocale[]`; `MULTILINGUAL` (more than one language ships); `toAppLocale(tag: string | null | undefined): AppLocale` (by primary subtag, else `en-US`); `formatLocale(l: AppLocale): string`; `dirOf(l: AppLocale): 'rtl' | 'ltr'`.
  - `i18n/index.ts`: `initI18n(): Promise<void>`, awaited in `main.tsx` before the first render and setting `<html lang dir>` itself, so a right-to-left visitor never sees a left-to-right flash (no inline script: the CSP forbids it). It configures i18next with `react-i18next`, the browser detector configured `order: ['localStorage', 'navigator']`, `caches: ['localStorage']`, `fallbackLng: 'en-US'`, `supportedLngs: SUPPORTED`, `nonExplicitSupportedLngs: false`, `load: 'currentOnly'`, `interpolation.escapeValue: false`, resources imported statically); `currentLocale(): AppLocale`; `setLocale(l: AppLocale): Promise<void>`.
  - `i18next.d.ts`: `CustomTypeOptions` with `resources` typed from the `en-US` JSON files, so unknown keys fail `tsc`.
  - `LocaleSync`: renders nothing; sets `document.documentElement.lang`/`.dir` on language change; when signed in, fetches `/me/preferences` once and calls `setLocale` with the stored locale.
  - `LanguageSwitch`: a compact select built on the existing `components/ui/select.tsx`, each language labeled in its own language (`English`); it renders nothing unless `MULTILINGUAL`. It calls `setLocale`, and with `persist` also `PUT /me/preferences`. If the save fails, it switches back to the previous language and shows the translated error, so the screen never disagrees with the stored value. Props: `{ persist?: boolean }`.
  - `useFormat()`: returns `{ number(n: number): string; date(iso: string, opts?: Intl.DateTimeFormatOptions): string; dateTime(iso: string): string; relative(iso: string, now?: Date): string }` using `formatLocale(currentLocale())`. `format.ts` functions `relativeTime`, `greeting`, `dateLabel` take a `locale: AppLocale` parameter (or move behind `useFormat`; keep `initials` as is) and get their strings from the catalog.
  - `authFetch`: sets `Accept-Language: currentLocale()` on every request.
  - Sign-in redirects pass `extraQueryParams: { ui_locales: currentLocale() }`.
  - Error messages: `messageFor` in `web/src/api/errors.ts` uses `i18next.t('errors.<code>')`, falling back to `errors.generic` (never `detail`) when the key is missing.
  - Test setup: call `await initI18n()` and `setLocale('en-US')` in `beforeAll`, so existing English assertions keep passing.

- [ ] **Step 1: Write failing tests**: `catalogs.test.ts`: `ships one catalog folder per supported language`, `every key exists in every catalog` (flatten nested keys of each language's `*.json`; assert the `en-US` key set and no empty strings), `every ErrorCode has a translation` (read `../../backend/openapi.json` via `node:fs`, take `components.schemas.ErrorCode.enum`, assert `errors.<code>` exists in every catalog), and `every plural key supplies all CLDR forms of its language`. `i18n.test.tsx`: `setting a language sets lang and dir on html from its direction`, `maps any tag to a supported language by its primary subtag`, `numbers use the language format` (`useFormat().number(1234)` is `1,234`), `authFetch sends the on-screen language` (MSW handler asserts `accept-language: en-US`), `unknown error code falls back to the generic message` (`messageFor(new ApiError(500, 'future_code', 'English detail'))` equals `errors.generic` and does not contain `English detail`).
- [ ] **Step 2: Run** `pnpm test` from `web/`. Expected: FAIL.
- [ ] **Step 3: Implement** the files above, with `common.json` holding shell-wide keys (`errors.*`, `language.*`, `actions.*`).
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): add i18next, the locale table, locale sync, and formatting`.

---

### Task 7: Web shell, sign-in, status, and profile screens

**Files:**
- Modify: `web/src/components/*.tsx` (`Shell`, `ConfirmDialog`, `Pager`, `Panel`, `Logo` alt text), `web/src/components/ui/*.tsx` (RTL check of the Base UI wrappers: `sheet.tsx` opens from the inline-start side, `dropdown-menu.tsx`/`select.tsx` align to inline start and mirror their chevrons, `dialog.tsx`/`alert-dialog.tsx` close buttons use `end-`, `table.tsx` cells use `text-start`), `web/src/StartupError.tsx`, `web/src/theme/ThemeToggle.tsx`, `web/src/features/me/*.tsx`, `web/src/features/home/*.tsx`
- Create: `web/src/i18n/en-US/me.json`, `en-US/home.json` (register in `i18n/index.ts` and `i18next.d.ts`)
- Test: the existing tests beside each file, plus `web/src/features/me/LoginPage.test.tsx`, `ProfilePage.test.tsx`

**Interfaces:**
- Consumes: `useTranslation`, `useFormat`, `LanguageSwitch`, `LocaleSync` (Task 6).
- Produces: every visible string, `aria-label`, `title`, and `alt` in these files comes from the catalog; `LanguageSwitch` on `LoginPage` and on the pending/disabled screens in `StatusScreens.tsx` (`persist` only when signed in), and a "Language" row on `ProfilePage` (`persist`), shown only when `MULTILINGUAL`. `LocaleSync` mounted once in `App.tsx`.

**RTL conversion rule (this and Tasks 8–9):** replace physical utilities with logical ones (`ml-`/`mr-` with `ms-`/`me-`, `pl-`/`pr-` with `ps-`/`pe-`, `left-`/`right-` with `start-`/`end-`, `text-left`/`text-right` with `text-start`/`text-end`, `rounded-l-`/`rounded-r-` with `rounded-s-`/`rounded-e-`, `border-l`/`border-r` with `border-s`/`border-e`); find them with `grep -rnE "\b(ml|mr|pl|pr|left|right|rounded-[lr]|border-[lr]|text-left|text-right)-?" web/src --include=*.tsx`. Direction-sensitive icons (`ChevronLeft`/`ChevronRight`, `ArrowLeft`/`ArrowRight`, back and next) get `rtl:-scale-x-100`. User-content elements get `dir="auto"`.

- [ ] **Step 1: Write failing tests**: `LoginPage.test.tsx`: `shows no language switch while one language ships`; `ProfilePage.test.tsx`: `shows no language row while one language ships`, `display name uses automatic direction` (`dir="auto"` on the name element); `Gate.test.tsx`: `status screens show no language switch while one language ships`; `Shell.test.tsx`: every shell label comes from the catalog.
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Extract strings, convert to logical utilities, mirror icons** in the listed files.
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the shell, sign-in, status, home, and profile screens`.

---

### Task 8: Web users and audit screens

**Files:**
- Modify: `web/src/features/users/*.tsx`, `web/src/features/audit/*.tsx` and `kinds.ts`
- Create: `web/src/i18n/en-US/users.json`, `en-US/audit.json`

**Interfaces:**
- Consumes: Task 6–7 infrastructure. Audit event kind labels in `kinds.ts` become catalog keys `audit.kinds.<kind>`; the catalog test from Task 6 also asserts every kind in `kinds.ts` has a key.

- [ ] **Step 1: Write failing tests** in `UsersPage.test.tsx` and `AuditPage.test.tsx`: `renders from the catalog` (column headers and status labels from the catalog; counts through the formatter), `identifiers stay isolated` (a request ID or user ID cell is inside an element with `dir="ltr"` and contains ASCII digits), `emails and names use automatic direction`.
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Implement** per the RTL conversion rule in Task 7 and the identifier rule in Global Constraints.
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the users and audit screens`.

---

### Task 9: Web jobs console and the direction smoke test

**Files:**
- Modify: `web/src/features/jobs/*.tsx`, `web/src/features/jobs/labels.ts`, `web/src/features/jobs/ThroughputChart.tsx`, `web/e2e/smoke.spec.ts`
- Create: `web/src/i18n/en-US/jobs.json`

**Interfaces:**
- Consumes: Tasks 6–7. `labels.ts` state and action labels become catalog keys; `labels.test.ts` asserts keys instead of English strings. Chart axis ticks use `useFormat().number`/`date`; recharts axes mirror in right-to-left (`reversed` on the X axis and `orientation="right"` on the Y axis when `dirOf(locale) === 'rtl'`). Cron expressions, job IDs, queue names, attempt error payloads stay inside `dir="ltr"`.

- [ ] **Step 1: Write failing tests**: `JobsDashboard.test.tsx` `renders from the catalog` (headings and state labels from the catalog; counts through the formatter), `RecurringPage.test.tsx` `cron expression stays isolated`, `JobDetailPage.test.tsx` `attempt payload stays LTR`. In `smoke.spec.ts`, add `sign-in page is in English, left to right` (open `/login`, expect `html[dir="ltr"]` and `html[lang="en-US"]`); it runs only against the dev stack like the existing smoke test.
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the web gates; run `pnpm e2e` against a running dev stack if available (report if not run). Then search for leftover literals: `grep -rnE ">[^<{]*[A-Za-z]{3,}[^<{]*<" web/src --include=*.tsx | grep -v test` should list only brand names, code samples, or user content. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the jobs console and add a direction smoke test`.

---

### Task 10: App l10n foundation

**Files:**
- Modify: `mobile/pubspec.yaml` (`flutter_localizations: {sdk: flutter}`, `flutter: generate: true`), `mobile/ios/Runner/Info.plist` (`CFBundleLocalizations` array with `en`, one entry per shipped language; iOS otherwise reports only the development language to Flutter, as the Flutter internationalization guide notes), `mobile/lib/ui/app.dart`, `mobile/lib/data/http/dio_factory.dart`, `mobile/lib/data/auth/app_auth_client.dart:38`, `mobile/lib/ui/theme/app_theme.dart`, `mobile/.gitignore` (`lib/l10n/gen/`), `.github/workflows/ci.yml` (no change expected: `flutter pub get` runs gen-l10n; confirm)
- Create: `mobile/l10n.yaml`, `mobile/lib/l10n/app_en.arb`, `mobile/lib/state/locale.dart`, `mobile/lib/ui/format.dart`, `mobile/lib/ui/widgets/language_switch.dart`
- Test: `mobile/test/ui/l10n_test.dart`, `mobile/test/state/locale_test.dart`

**Interfaces:**
- Produces:
  - `l10n.yaml`: `arb-dir: lib/l10n`, `template-arb-file: app_en.arb`, `output-dir: lib/l10n/gen`, `output-localization-file: app_localizations.dart`, `nullable-getter: false`, `untranslated-messages-file: build/l10n_untranslated.json`.
  - `state/locale.dart`: `enum AppLocale { en }` with `Locale get flutterLocale`, `String get nativeName`, `String get apiTag` (`'en-US'`), `static bool get multilingual`, `static AppLocale fromTag(String? tag)` (by primary subtag, else `en`); `final localeProvider = NotifierProvider<LocaleController, AppLocale>(...)` initialized from `shared_preferences` key `locale`, else the device locale; `LocaleController.set(AppLocale l, {bool persist = false})` stores locally and, when `persist`, calls `PUT /me/preferences`; after sign-in the controller loads `GET /me/preferences` and applies it (follow the pattern of `state/theme.dart`).
  - `MaterialApp.router` gets `locale: ref.watch(localeProvider).flutterLocale`, `supportedLocales: AppLocalizations.supportedLocales`, `localizationsDelegates: AppLocalizations.localizationsDelegates`.
  - `dio_factory.dart`: an interceptor sets `Accept-Language` to the current `apiTag`.
  - `app_auth_client.dart`: `AuthorizationTokenRequest(..., additionalParameters: {'ui_locales': apiTag})`.
  - `ui/format.dart`: `String formatNumber(BuildContext, num)`, `String formatDate(BuildContext, DateTime, {String? pattern})`, `String formatRelative(BuildContext, DateTime, {DateTime? now})`, using `intl` with the active locale.
  - `LanguageSwitch` widget: `LanguageSwitch({bool persist = false})`, empty unless `AppLocale.multilingual`.
  - `ui/errors/messages.dart`: `messageFor(BuildContext, Object)` reads `AppLocalizations` (keys `error_<code>`, fallback `error_generic`, never `detail`).

- [ ] **Step 1: Write failing tests**: `l10n_test.dart`: `a catalog for each supported language, with identical keys` (parse every `app_<language>.arb` with `dart:convert`, ignore `@`-metadata keys, assert one file per `AppLocale`, equal key sets, and non-empty values), `every ErrorCode has a translation` (read `../backend/openapi.json`, `components.schemas.ErrorCode.enum`, assert `error_<code>` exists in every ARB), `English digits stay Western` (`formatNumber` gives `1,234`), `unknown error code falls back` (the generic message, not the detail). `locale_test.dart`: `fromTag maps a tag by its primary subtag, anything else to en`, `set persists to the API when asked` (fake API client records `PUT` with `en-US`), `dio sends the on-screen language` (request headers contain `accept-language: en-US`).
- [ ] **Step 2: Run** `flutter test` from `mobile/` (PowerShell). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the app gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): add gen-l10n, locale state, and formatting`.

---

### Task 11: App screens

**Files:**
- Modify: `mobile/lib/ui/screens/*.dart`, `mobile/lib/ui/widgets/*.dart`, `mobile/lib/ui/router.dart` (page titles), `mobile/lib/ui/errors/messages.dart`, `mobile/lib/l10n/app_en.arb`
- Test: existing screen tests under `mobile/test/ui/`

**Interfaces:**
- Consumes: Task 10.
- Produces: every visible string, tooltip, and semantics label from `AppLocalizations`; `LanguageSwitch()` on `sign_in_screen.dart` and the pending/disabled screens in `status_screens.dart`; `LanguageSwitch(persist: true)` row on `profile_screen.dart`, shown only when `AppLocale.multilingual`. `LabelText` drops capitals and letter spacing in right-to-left, where scripts often have no capitals and spacing breaks joined letters. Direction-sensitive icons use `Icons.arrow_back`/`chevron_*` with automatic mirroring (`Icon(..., textDirection: Directionality.of(context))` or `matchTextDirection` on images). User names and emails render with `Directionality` derived from the text (`intl`'s `Bidi.detectRtlDirectionality`). IDs stay in an LTR `Directionality`.

- [ ] **Step 1: Write failing tests**: `sign_in_screen_test.dart` `the sign-in screen is left to right with no language switch`, `profile_screen_test.dart` `shows no language switch while one language ships`, `status_screens_test.dart` `status screens show no language switch while one language ships`, `user_detail_screen_test.dart` `email uses its own direction` and `a right-to-left name and email read right to left inside an English screen`, `shell_screen_test.dart` `a right-to-left name in an English sentence is isolated`.
- [ ] **Step 2: Run** `flutter test`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the app gates, then `flutter run --flavor development --dart-define-from-file=config/development.json` on the emulator (`./tool/android-dev.ps1 help`) and visit every screen; report what was checked or that it was not run. Expected: gates PASS.
- [ ] **Step 5: Commit** `feat(app): translate every screen and add the language switch`.

---

### Task 12: Sign-in language check, plan sync, final gates

**Files:**
- Modify: `docs/contracts/03. publishing roadmap.md` (REST table: Preferences row phase becomes "D0 (locale), D1 (switches)"; D0 bullets: "`GET/PUT /me/preferences` with `locale`", and the web bullet names the chosen stack: i18next without ICU), `CLAUDE.md` (add `i18n` to the crate list in "Project state")

- [ ] **Step 1: Check Zitadel.** With the dev stack up, start a web sign-in and confirm the request carries `ui_locales=en-US`.
- [ ] **Step 2: Update plan 03** as listed above.
- [ ] **Step 3: Run all gates**: Rust, web, app, and `cargo xtask openapi --check`. Expected: all PASS.
- [ ] **Step 4: Exit check.** In the running web app and on the emulator: sign in and visit every screen (home, profile, users, user detail, audit, jobs dashboard, jobs list, job detail, recurring), confirm every label comes from the catalog and identifiers sit in `dir="ltr"`. Report the result screen by screen.
- [ ] **Step 5: Commit** `docs: record D0 preferences scope and sign-in language support`.
