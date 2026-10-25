# D0: Localization Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every existing web screen, mobile screen, server email, and API error translatable and right-to-left capable, ship `en-US` and `ar` (Modern Standard Arabic, Arabic-Indic digits), store the user's language in `user_preferences.locale`, and fail CI when a catalog misses a key.

**Architecture:** Server: a new leaf crate `postit-i18n` (Project Fluent) owns locale negotiation and server catalogs; `postit-data` stores `locale`; `postit-identity` sets it at first sign-in from the client's `Accept-Language`; `postit-mail` renders the two plan 02 emails per recipient locale; `postit-api` adds `GET/PUT /me/preferences` (locale only, pending users allowed). Web: i18next + react-i18next with bundled JSON catalogs, `lang`/`dir` on `<html>`, logical CSS, and a formatting hook forcing `ar-u-nu-arab`. Mobile: gen-l10n ARB catalogs, locale from a Riverpod provider, Arabic-Indic digits, Noto Sans Arabic fallback. Both clients send `Accept-Language` with the language on screen and `ui_locales` at sign-in.

**Tech Stack:** Rust (`fluent-bundle`, `fluent-langneg`, `unic-langid`, askama, sqlx, axum, utoipa), React 19 (`i18next`, `react-i18next`, `i18next-browser-languagedetector`, `@fontsource-variable/noto-sans-arabic`, Vitest, Playwright), Flutter 3.47.6 (`flutter_localizations`, `intl`, gen-l10n, Riverpod).

**Spec:** `docs/superpowers/specs/2026-10-18-d0-localization-design.md` (from `!ref/plans/03. publishing roadmap.md`, phase D0 and "Localization"). Read both.

## Global Constraints

- Supported locales, exactly: `en-US` (default, reference catalog) and `ar`. The server stores and accepts only these two tags; clients map `en`/`en-*` to `en-US` and `ar`/`ar-*` to `ar`.
- Arabic UI uses Arabic-Indic digits for every formatted quantity (counts, dates, times, durations, pager numbers, chart axes). Verbatim identifiers and technical values (job, request, and user IDs, cron expressions, HTTP status codes, error codes, raw JSON payloads) stay as-is in Western digits, isolated with `dir="ltr"` (web: `<bdi dir="ltr">` or `<code dir="ltr">`; Flutter: `Directionality(textDirection: TextDirection.ltr)`). No other digit mix.
- User content (display names, emails, later post text) renders with automatic direction (web `dir="auto"`; Flutter direction from the text).
- problem+json `detail` stays English. Swagger UI is not translated.
- Times display in the device's time zone (no time-zone preference in D0).
- JS tooling through pnpm only, never npm; on Windows run pnpm and Flutter through PowerShell (the Git Bash pnpm shim is broken). Exact version pins in `web/package.json`.
- Rust dependencies at workspace level; no dependency with an Arabic or Arabic-derived name.
- One migration file: edit `backend/crates/data/migrations/0001_init.sql` in place, then reset the dev database (`./stack.ps1` per the `stack` skill) and refresh the sqlx offline cache (`cargo sqlx prepare --workspace -- --all-targets` from `backend/`).
- After any API change run `cargo xtask openapi` from `backend/` (regenerates `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`).
- Gates per area: Rust from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`. Web from `web/`: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`. App from `mobile/`: `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test`. Contract: `cargo xtask openapi --check`.
- Arabic text is written in plain, short Modern Standard Arabic; brand name `postit` stays Latin.
- Commits: Conventional Commits (scopes `i18n`, `data`, `identity`, `mail`, `api`, `web`, `app`, `ci`, `docs`), ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A user switches to Arabic on the sign-in screen while their browser says English, then signs in for the first time**: the stored locale must be `ar` and the UI must stay Arabic after login. Task 6 (web) and Task 10 (app) test that the header carries the on-screen language; Task 3 tests provisioning stores it.
2. **`Accept-Language: fr-CA, ar;q=0.5`, an empty header, or garbage**: negotiate to `ar`, `en-US`, and `en-US` respectively, never fail the request. Task 1 tests these.
3. **Mixed-direction strings in Arabic** (an English display name or an email inside an Arabic sentence, a job ID in a row): must not reorder punctuation or digits. Tasks 7–9 test `dir="auto"`/`dir="ltr"` on those elements; Task 11 for the app.
4. **A pending user changes language**: `PUT /me/preferences` must succeed for `pending` and fail for `disabled`; the later `user_approved` email uses the new locale. Task 5 and Task 4 test both.
5. **An error code the client has no translation for** (a future server code): falls back to the generic translated message, never shows the raw code or English `detail` in Arabic. Task 6 and Task 10 test it.

---

### Task 1: `postit-i18n` crate

**Files:**
- Create: `backend/crates/i18n/Cargo.toml`, `backend/crates/i18n/src/lib.rs`, `backend/crates/i18n/src/locale.rs`, `backend/crates/i18n/src/catalog.rs`, `backend/crates/i18n/locales/en-US/mail.ftl`, `backend/crates/i18n/locales/ar/mail.ftl`
- Modify: `backend/Cargo.toml` (members, workspace deps `fluent-bundle = "0.16"`, `fluent-langneg = "0.14"`, `unic-langid = "0.9"`, `postit-i18n = { path = "crates/i18n" }`; check crates.io for the current minor at phase start and pin that)

**Interfaces:**
- Produces:
  - `enum Locale { EnUs, Ar }` (`Copy`, `Default = EnUs`), `Locale::ALL: [Locale; 2]`, `tag(self) -> &'static str` (`"en-US"`, `"ar"`), `from_tag(&str) -> Option<Locale>` (exact tags only), `is_rtl(self) -> bool`, `dir(self) -> &'static str` (`"ltr"`/`"rtl"`), `negotiate(accept_language: Option<&str>) -> Locale`.
  - `struct Args` builder: `Args::new()`, `.str(name: &'static str, value: impl Into<String>)`, `.num(name: &'static str, value: i64)`.
  - `fn t(locale: Locale, id: &str, args: &Args) -> String` (falls back to `en-US` when the id is missing in `locale`, then to the id itself; never panics).
  - `fn message_ids(locale: Locale) -> BTreeSet<String>` (for the completeness test and other crates' tests).

**Approach:** `fluent_bundle::concurrent::FluentBundle` per locale in a `std::sync::LazyLock`, resources embedded with `include_str!`. A bundle formatter (`set_formatter`) renders `FluentValue::Number` with Arabic-Indic digits (U+0660–U+0669) for `ar` and plain digits for `en-US`; plural selection still uses the number. Keep Unicode isolation on (`use_isolating = true`) so mixed-direction arguments render correctly. `negotiate` parses with `fluent_langneg::accepted_languages::parse` and picks the first entry whose language subtag is `ar` or `en`, else `EnUs`.

**Mail messages** (both files define exactly these ids; Arabic translations written by the implementer): `pending-subject` (`$count`, plural), `pending-intro` (`$count`), `pending-more` (`$more`), `pending-review` (link text), `pending-footer`, `approved-subject`, `approved-greeting` (`$name`), `approved-body`, `approved-sign-in` (link text). Base the English text on the current templates in `backend/crates/mail/templates/` and `templates.rs` (`"{total} postit account(s) waiting for approval"` becomes a proper plural).

- [ ] **Step 1: Write failing tests** in `lib.rs` tests:

```rust
#[test] fn negotiate_table() {
    for (header, want) in [(Some("ar"), Locale::Ar), (Some("ar-SA,en;q=0.8"), Locale::Ar), (Some("fr-CA, ar;q=0.5"), Locale::Ar),
                           (Some("en-GB"), Locale::EnUs), (Some(""), Locale::EnUs), (Some("%%%"), Locale::EnUs), (None, Locale::EnUs)] {
        assert_eq!(negotiate(header), want, "{header:?}");
    }
}
#[test] fn catalogs_have_identical_ids() { assert_eq!(message_ids(Locale::EnUs), message_ids(Locale::Ar)); }
#[test] fn arabic_numbers_use_arabic_indic_digits() { let s = strip_isolation(&t(Locale::Ar, "pending-subject", &Args::new().num("count", 12))); assert!(s.contains("١٢") && !s.chars().any(|c| c.is_ascii_digit())); }
#[test] fn english_plural() { assert_eq!(strip_isolation(&t(Locale::EnUs, "pending-subject", &Args::new().num("count", 1))), "1 postit account is waiting for approval"); }
#[test] fn arabic_plural_forms_differ() { let f = |n| t(Locale::Ar, "pending-subject", &Args::new().num("count", n)); assert_ne!(f(1), f(2)); assert_ne!(f(2), f(3)); assert_ne!(f(3), f(11)); }
#[test] fn missing_id_falls_back() { assert_eq!(t(Locale::Ar, "no-such-id", &Args::new()), "no-such-id"); }
#[test] fn tags_round_trip() { for l in Locale::ALL { assert_eq!(Locale::from_tag(l.tag()), Some(l)); } assert_eq!(Locale::from_tag("en"), None); }
```

  (`strip_isolation` removes U+2068/U+2069; define it in the test module. Also expose it as `pub fn strip_isolation(&str) -> String` under `#[cfg(any(test, feature = "testkit"))]` with a `testkit` feature, for Task 4's tests.)
- [ ] **Step 2: Run** `cargo test -p postit-i18n` from `backend/`. Expected: compile failure.
- [ ] **Step 3: Implement** the crate and both `.ftl` files.
- [ ] **Step 4: Run** tests, clippy, and `cargo deny check` (licenses of the new crates are MIT/Apache). Expected: PASS.
- [ ] **Step 5: Commit** `feat(i18n): add the postit-i18n crate with en-US and ar catalogs`.

---

### Task 2: `user_preferences.locale` in the data layer

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql:44-50`, `backend/crates/data/src/preferences.rs`, `backend/crates/data/tests/preferences.rs`, `backend/.sqlx/` (regenerated)

**Interfaces:**
- Produces: `UserPreferences.locale: String`; `UserPreferencesRepo::create_default(conn, user_id: UserId, timezone: &str, locale: &str) -> Result<UserPreferences, DataError>` (new `locale` parameter); `UserPreferencesRepo::set_locale(conn, user_id: UserId, locale: &str) -> Result<UserPreferences, DataError>` (returns `DataError::NotFound` or the crate's existing not-found variant when no row).

- [ ] **Step 1: Write failing tests** in `data/tests/preferences.rs`: `create_default_stores_locale` (create with `"ar"`; `get` returns `locale == "ar"`), `set_locale_updates_only_locale` (timezone unchanged), `set_locale_missing_user` (not-found error). Update existing `create_default` calls to pass `"en-US"`.
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

- [ ] **Step 1: Write failing tests**: identity `first_sign_in_stores_locale` (transform a new subject with `Locale::Ar`; preferences row has `"ar"`), `later_sign_in_keeps_stored_locale` (second transform with `Locale::EnUs` leaves `"ar"`); api `first_request_stores_negotiated_locale` (the api testkit runs the real `Authenticator` over the test database with its `TestIssuer`: `GET /api/v1/me` with a token for a new subject and `Accept-Language: ar-EG`, then read `user_preferences.locale` from `TestApp.pool` and assert `"ar"`; a second new subject without the header gets `"en-US"`).
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

- [ ] **Step 1: Write failing tests**: `renders_approved_in_arabic` (`lang="ar"`, `dir="rtl"`, subject equals `t(Ar, "approved-subject")`, no ASCII digits in subject), `renders_pending_in_english_with_plural` (2 users + `more: 3` gives subject `"5 postit accounts are waiting for approval"`), `display_name_is_isolated` (HTML contains `<bdi>Ada</bdi>`); in `approval_emails.rs`: `approved_email_uses_recipient_locale` (recipient prefs `"ar"`, captured mail subject is the Arabic one), `missing_preferences_sends_english` (recipient with no preferences row: the email is sent in `en-US`).
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
- Produces: `GET /api/v1/me/preferences` returns `PreferencesDto { locale: LocaleDto }`; `PUT /api/v1/me/preferences` with `UpdatePreferences { locale: String }` returns the updated `PreferencesDto`. `LocaleDto` is a utoipa string enum with values `"en-US"`, `"ar"` (so the generated TS and Dart types are closed). An unsupported value is 422 `validation_failed`. Tag `me`, security `oidc`. No act-as (self route).

- [ ] **Step 1: Write failing tests**: `get_returns_default_locale` (`"en-US"`), `put_sets_locale_and_get_reflects_it`, `put_rejects_unsupported_locale` (`"fr"` and `"en"` give 422 with code `validation_failed`), `pending_user_can_read_and_change` (pending principal: 200 on both), `disabled_user_is_rejected` (403 `account_disabled`), `users_only_see_their_own` (user A sets `ar`; user B still gets `en-US`), `act_as_header_does_not_reach_another_user` (with `X-Postit-Act-As` naming another user, the route behaves exactly as `GET /me` does with that header today: check `routes/me.rs` and mirror it; it never reads or writes the other user's row), `unauthenticated_is_401`.
- [ ] **Step 2: Run** `cargo test -p postit-api --test preferences`. Expected: FAIL (404s).
- [ ] **Step 3: Implement** the handlers and register them in the router and OpenAPI document.
- [ ] **Step 4: Run** tests, clippy, then `cargo xtask openapi` and `cargo xtask openapi --check`. Expected: PASS; regenerated files committed.
- [ ] **Step 5: Error-code audit.** Run `grep -rn "ApiError::new\|ErrorCode::" backend/crates/api/src` and confirm every error response is built from an `ErrorCode` (no free-text-only errors). Record the result in the commit body (list of the 18 codes). Expected: no code changes needed; fix any path that is not.
- [ ] **Step 6: Commit** `feat(api): add GET/PUT /me/preferences with the locale`.

---

### Task 6: Web i18n foundation

**Files:**
- Modify: `web/package.json` (+ `i18next`, `react-i18next`, `i18next-browser-languagedetector`, `@fontsource-variable/noto-sans-arabic`, exact current versions via `pnpm add -E`), `web/src/main.tsx`, `web/src/index.css:4-14`, `web/src/test/setup.ts`, `web/src/api/authFetch.ts`, `web/src/auth/SessionProvider.tsx:53,99`, `web/src/lib/format.ts`
- Create: `web/src/i18n/index.ts`, `web/src/i18n/locales.ts`, `web/src/i18n/i18next.d.ts`, `web/src/i18n/useFormat.ts`, `web/src/i18n/LocaleSync.tsx`, `web/src/i18n/LanguageSwitch.tsx`, `web/src/i18n/en-US/common.json`, `web/src/i18n/ar/common.json`
- Test: `web/src/i18n/i18n.test.tsx`, `web/src/i18n/catalogs.test.ts`, `web/src/lib/format.test.ts`

**Interfaces:**
- Produces:
  - `locales.ts`: `type AppLocale = 'en-US' | 'ar'`; `SUPPORTED: readonly AppLocale[]`; `toAppLocale(tag: string | null | undefined): AppLocale` (`ar*` gives `ar`, everything else `en-US`); `formatLocale(l: AppLocale): string` (`'ar-u-nu-arab'` or `'en-US'`); `dirOf(l: AppLocale): 'rtl' | 'ltr'`.
  - `i18n/index.ts`: `initI18n(): Promise<void>`, awaited in `main.tsx` before the first render and setting `<html lang dir>` itself, so an Arabic visitor never sees a left-to-right flash (no inline script: the CSP forbids it). It configures i18next with `react-i18next`, the browser detector configured `order: ['localStorage', 'navigator']`, `caches: ['localStorage']`, `fallbackLng: 'en-US'`, `supportedLngs: SUPPORTED`, `nonExplicitSupportedLngs: false`, `load: 'currentOnly'`, `interpolation.escapeValue: false`, resources imported statically); `currentLocale(): AppLocale`; `setLocale(l: AppLocale): Promise<void>`.
  - `i18next.d.ts`: `CustomTypeOptions` with `resources` typed from the `en-US` JSON files, so unknown keys fail `tsc`.
  - `LocaleSync`: renders nothing; sets `document.documentElement.lang`/`.dir` on language change; when signed in, fetches `/me/preferences` once and calls `setLocale` with the stored locale.
  - `LanguageSwitch`: a compact select built on the existing `components/ui/select.tsx`, with the two languages labeled in their own language (`English`, `العربية`); calls `setLocale`, and with `persist` also `PUT /me/preferences`. If the save fails, it switches back to the previous language and shows the translated error, so the screen never disagrees with the stored value. Props: `{ persist?: boolean }`.
  - `useFormat()`: returns `{ number(n: number): string; date(iso: string, opts?: Intl.DateTimeFormatOptions): string; dateTime(iso: string): string; relative(iso: string, now?: Date): string }` using `formatLocale(currentLocale())`. `format.ts` functions `relativeTime`, `greeting`, `dateLabel` take a `locale: AppLocale` parameter (or move behind `useFormat`; keep `initials` as is) and get their strings from the catalog.
  - `authFetch`: sets `Accept-Language: currentLocale()` on every request.
  - Sign-in redirects pass `extraQueryParams: { ui_locales: currentLocale() }`.
  - Error messages: `messageFor` in `web/src/api/errors.ts` uses `i18next.t('errors.<code>')`, falling back to `errors.generic` (never `detail`) when the key is missing.
  - CSS: `@import '@fontsource-variable/noto-sans-arabic';` and append `'Noto Sans Arabic Variable'` after the Latin faces in `--font-heading`, `--font-display`, `--font-sans`.
  - Test setup: call `await initI18n()` and `setLocale('en-US')` in `beforeAll`, so existing English assertions keep passing.

- [ ] **Step 1: Write failing tests**: `catalogs.test.ts`: `every key exists in both catalogs` (flatten nested keys of each `en-US/*.json` and `ar/*.json`; assert equal sets and no empty strings) and `every ErrorCode has a translation` (read `../../backend/openapi.json` via `node:fs`, take `components.schemas.ErrorCode.enum`, assert `errors.<code>` exists in both catalogs). `i18n.test.tsx`: `switching to Arabic sets lang and dir on html`, `Arabic numbers use Arabic-Indic digits` (`useFormat().number(1234)` contains `١٬٢٣٤` and no `[0-9]`), `Arabic dates use Arabic-Indic digits`, `authFetch sends the on-screen language` (switch to `ar`, MSW handler asserts `accept-language: ar`), `unknown error code falls back to the generic message` (in `ar`, `messageFor(new ApiError(500, 'future_code', 'English detail'))` equals the Arabic `errors.generic` and does not contain `English detail`).
- [ ] **Step 2: Run** `pnpm test` from `web/`. Expected: FAIL.
- [ ] **Step 3: Implement** the files above, with `common.json` holding shell-wide keys (`errors.*`, `language.*`, `actions.*`).
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): add i18next, Arabic font, locale sync, and formatting`.

---

### Task 7: Web shell, sign-in, status, and profile screens

**Files:**
- Modify: `web/src/components/*.tsx` (`Shell`, `ConfirmDialog`, `Pager`, `Panel`, `Logo` alt text), `web/src/components/ui/*.tsx` (RTL check of the Base UI wrappers: `sheet.tsx` opens from the inline-start side, `dropdown-menu.tsx`/`select.tsx` align to inline start and mirror their chevrons, `dialog.tsx`/`alert-dialog.tsx` close buttons use `end-`, `table.tsx` cells use `text-start`), `web/src/StartupError.tsx`, `web/src/theme/ThemeToggle.tsx`, `web/src/features/me/*.tsx`, `web/src/features/home/*.tsx`
- Create: `web/src/i18n/en-US/me.json`, `ar/me.json`, `en-US/home.json`, `ar/home.json` (register in `i18n/index.ts` and `i18next.d.ts`)
- Test: the existing tests beside each file, plus `web/src/features/me/LoginPage.test.tsx`, `ProfilePage.test.tsx`

**Interfaces:**
- Consumes: `useTranslation`, `useFormat`, `LanguageSwitch`, `LocaleSync` (Task 6).
- Produces: every visible string, `aria-label`, `title`, and `alt` in these files comes from the catalog; `LanguageSwitch` on `LoginPage` and on the pending/disabled screens in `StatusScreens.tsx` (`persist` only when signed in), and a "Language" row on `ProfilePage` (`persist`). `LocaleSync` mounted once in `App.tsx`.

**RTL conversion rule (this and Tasks 8–9):** replace physical utilities with logical ones (`ml-`/`mr-` with `ms-`/`me-`, `pl-`/`pr-` with `ps-`/`pe-`, `left-`/`right-` with `start-`/`end-`, `text-left`/`text-right` with `text-start`/`text-end`, `rounded-l-`/`rounded-r-` with `rounded-s-`/`rounded-e-`, `border-l`/`border-r` with `border-s`/`border-e`); find them with `grep -rnE "\b(ml|mr|pl|pr|left|right|rounded-[lr]|border-[lr]|text-left|text-right)-?" web/src --include=*.tsx`. Direction-sensitive icons (`ChevronLeft`/`ChevronRight`, `ArrowLeft`/`ArrowRight`, back and next) get `rtl:-scale-x-100`. User-content elements get `dir="auto"`.

- [ ] **Step 1: Write failing tests**: `LoginPage.test.tsx`: `language switch on the sign-in page turns the page Arabic` (select `العربية`; heading text equals the `ar` catalog value; `document.documentElement.dir === 'rtl'`; no request to `/me/preferences`); `ProfilePage.test.tsx`: `changing language saves it` (MSW asserts `PUT /api/v1/me/preferences` body `{ locale: 'ar' }`; UI switches), `failed save reverts the language` (MSW returns 500; `html[dir]` back to `ltr`, translated error shown), `display name uses automatic direction` (`dir="auto"` on the name element); `Shell.test.tsx`: `renders in Arabic without English chrome` (render in `ar`, assert none of the `en-US` catalog's values for the shell keys appear).
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Extract strings, convert to logical utilities, mirror icons** in the listed files; write the Arabic catalog values.
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the shell, sign-in, status, home, and profile screens`.

---

### Task 8: Web users and audit screens

**Files:**
- Modify: `web/src/features/users/*.tsx`, `web/src/features/audit/*.tsx` and `kinds.ts`
- Create: `web/src/i18n/{en-US,ar}/users.json`, `{en-US,ar}/audit.json`

**Interfaces:**
- Consumes: Task 6–7 infrastructure. Audit event kind labels in `kinds.ts` become catalog keys `audit.kinds.<kind>`; the catalog test from Task 6 also asserts every kind in `kinds.ts` has a key.

- [ ] **Step 1: Write failing tests** in `UsersPage.test.tsx` and `AuditPage.test.tsx`: `renders in Arabic` (column headers and status labels from the `ar` catalog; counts in Arabic-Indic digits), `identifiers stay Western and isolated` (a request ID or user ID cell is inside an element with `dir="ltr"` and contains ASCII digits), `emails and names use automatic direction`.
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Implement** per the RTL conversion rule in Task 7 and the digits rule in Global Constraints.
- [ ] **Step 4: Run** the web gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the users and audit screens`.

---

### Task 9: Web jobs console and RTL smoke test

**Files:**
- Modify: `web/src/features/jobs/*.tsx`, `web/src/features/jobs/labels.ts`, `web/src/features/jobs/ThroughputChart.tsx`, `web/e2e/smoke.spec.ts`
- Create: `web/src/i18n/{en-US,ar}/jobs.json`

**Interfaces:**
- Consumes: Tasks 6–7. `labels.ts` state and action labels become catalog keys; `labels.test.ts` asserts keys instead of English strings. Chart axis ticks use `useFormat().number`/`date` (Arabic-Indic in `ar`); recharts axes mirror in RTL (`reversed` on the X axis and `orientation="right"` on the Y axis when `dir === 'rtl'`). Cron expressions, job IDs, queue names, attempt error payloads stay Western inside `dir="ltr"`.

- [ ] **Step 1: Write failing tests**: `JobsDashboard.test.tsx` `renders in Arabic` (headings and state labels from `ar`; counts Arabic-Indic), `RecurringPage.test.tsx` `cron expression stays Western and isolated`, `JobDetailPage.test.tsx` `attempt payload stays LTR`. In `smoke.spec.ts`, add `sign-in page switches to Arabic` (open `/login`, choose `العربية`, expect `html[dir="rtl"]` and `html[lang="ar"]`); it runs only against the dev stack like the existing smoke test.
- [ ] **Step 2: Run** `pnpm test`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the web gates; run `pnpm e2e` against a running dev stack if available (report if not run). Then search for leftover literals: `grep -rnE ">[^<{]*[A-Za-z]{3,}[^<{]*<" web/src --include=*.tsx | grep -v test` should list only brand names, code samples, or user content. Expected: PASS.
- [ ] **Step 5: Commit** `feat(web): translate the jobs console and add an RTL smoke test`.

---

### Task 10: App l10n foundation

**Files:**
- Modify: `mobile/pubspec.yaml` (`flutter_localizations: {sdk: flutter}`, `flutter: generate: true`, Noto Sans Arabic font family), `mobile/ios/Runner/Info.plist` (`CFBundleLocalizations` array with `en` and `ar`; iOS otherwise reports only English to Flutter, as the Flutter internationalization guide notes), `mobile/lib/ui/app.dart`, `mobile/lib/data/http/dio_factory.dart`, `mobile/lib/data/auth/app_auth_client.dart:38`, `mobile/lib/ui/theme/app_theme.dart`, `mobile/.gitignore` (`lib/l10n/gen/`), `.github/workflows/ci.yml` (no change expected: `flutter pub get` runs gen-l10n; confirm)
- Create: `mobile/l10n.yaml`, `mobile/lib/l10n/app_en.arb`, `mobile/lib/l10n/app_ar.arb`, `mobile/assets/fonts/NotoSansArabic-Variable.ttf` (from Google Fonts' `notofonts` release, OFL; add the license file beside the other font licenses if the repo keeps them), `mobile/lib/state/locale.dart`, `mobile/lib/ui/format.dart`, `mobile/lib/ui/widgets/language_switch.dart`
- Test: `mobile/test/ui/l10n_test.dart`, `mobile/test/state/locale_test.dart`

**Interfaces:**
- Produces:
  - `l10n.yaml`: `arb-dir: lib/l10n`, `template-arb-file: app_en.arb`, `output-dir: lib/l10n/gen`, `output-localization-file: app_localizations.dart`, `nullable-getter: false`, `untranslated-messages-file: build/l10n_untranslated.json`.
  - `state/locale.dart`: `enum AppLocale { en, ar }` with `Locale get flutterLocale`, `String get apiTag` (`'en-US'`, `'ar'`), `static AppLocale fromTag(String? tag)`; `final localeProvider = NotifierProvider<LocaleController, AppLocale>(...)` initialized from `shared_preferences` key `locale`, else the device locale; `LocaleController.set(AppLocale l, {bool persist = false})` stores locally and, when `persist`, calls `PUT /me/preferences`; after sign-in the controller loads `GET /me/preferences` and applies it (follow the pattern of `state/theme.dart`).
  - `MaterialApp.router` gets `locale: ref.watch(localeProvider).flutterLocale`, `supportedLocales: AppLocalizations.supportedLocales`, `localizationsDelegates: AppLocalizations.localizationsDelegates`.
  - `dio_factory.dart`: an interceptor sets `Accept-Language` to the current `apiTag`.
  - `app_auth_client.dart`: `AuthorizationTokenRequest(..., additionalParameters: {'ui_locales': apiTag})`.
  - `app_theme.dart`: `fontFamilyFallback: ['NotoSansArabic']` on the text themes.
  - `ui/format.dart`: `String formatNumber(BuildContext, num)`, `String formatDate(BuildContext, DateTime, {String? pattern})`, `String formatRelative(BuildContext, DateTime, {DateTime? now})`, using `intl` with the active locale and Arabic-Indic digits for `ar` (if `intl`'s `ar` symbols give Western digits, map U+0030–U+0039 to U+0660–U+0669 after formatting).
  - `LanguageSwitch` widget: `LanguageSwitch({bool persist = false})`.
  - `ui/errors/messages.dart`: `messageFor(BuildContext, Object)` reads `AppLocalizations` (keys `error_<code>`, fallback `error_generic`, never `detail`).

- [ ] **Step 1: Write failing tests**: `l10n_test.dart`: `arb files have identical keys` (parse both ARB files with `dart:convert`, ignore `@`-metadata keys, assert equal key sets and non-empty values), `every ErrorCode has a translation` (read `../backend/openapi.json`, `components.schemas.ErrorCode.enum`, assert `error_<code>` exists in both ARBs), `Arabic digits` (`formatNumber` under an `ar` `Localizations` gives `١٬٢٣٤`), `unknown error code falls back` (Arabic generic message, not the detail). `locale_test.dart`: `fromTag maps ar-SA to ar and fr to en`, `set persists to the API when asked` (fake API client records `PUT` with `ar`), `dio sends the on-screen language` (switch to `ar`; request headers contain `accept-language: ar`).
- [ ] **Step 2: Run** `flutter test` from `mobile/` (PowerShell). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the app gates. Expected: PASS.
- [ ] **Step 5: Commit** `feat(app): add gen-l10n, locale state, Arabic font, and formatting`.

---

### Task 11: App screens

**Files:**
- Modify: `mobile/lib/ui/screens/*.dart`, `mobile/lib/ui/widgets/*.dart`, `mobile/lib/ui/router.dart` (page titles), `mobile/lib/ui/errors/messages.dart`, `mobile/lib/l10n/app_en.arb`, `mobile/lib/l10n/app_ar.arb`
- Test: existing screen tests under `mobile/test/ui/`

**Interfaces:**
- Consumes: Task 10.
- Produces: every visible string, tooltip, and semantics label from `AppLocalizations`; `LanguageSwitch()` on `sign_in_screen.dart` and the pending/disabled screens in `status_screens.dart`; `LanguageSwitch(persist: true)` row on `profile_screen.dart`. Direction-sensitive icons use `Icons.arrow_back`/`chevron_*` with automatic mirroring (`Icon(..., textDirection: Directionality.of(context))` or `matchTextDirection` on images). User names and emails render with `Directionality` derived from the text (`intl`'s `Bidi.detectRtlDirectionality`). IDs stay in an LTR `Directionality`.

- [ ] **Step 1: Write failing tests**: `sign_in_screen_test.dart` `switching to Arabic turns the screen RTL` (tap the switch, choose `العربية`; `Directionality.of` is `rtl`; title text equals the Arabic ARB value), `profile_screen_test.dart` `changing language saves it` (fake API receives `PUT`), `users_screen_test.dart` `renders in Arabic with Arabic-Indic counts`, `user_detail_screen_test.dart` `email uses its own direction`. Pump each screen in `ar` and assert no `app_en.arb` value for that screen's keys appears.
- [ ] **Step 2: Run** `flutter test`. Expected: FAIL.
- [ ] **Step 3: Implement**, writing the Arabic ARB values.
- [ ] **Step 4: Run** the app gates, then `flutter run --flavor development --dart-define-from-file=config/development.json` on the emulator (`./tool/android-dev.ps1 help`) and switch the profile to Arabic and back; report what was checked or that it was not run. Expected: gates PASS.
- [ ] **Step 5: Commit** `feat(app): translate every screen and add the language switch`.

---

### Task 12: Sign-in language check, plan sync, final gates

**Files:**
- Modify: `!ref/plans/03. publishing roadmap.md` (REST table: Preferences row phase becomes "D0 (locale), D1 (switches)"; D0 bullets: "`GET/PUT /me/preferences` with `locale`", and the web bullet names the chosen stack: i18next without ICU, Noto Sans Arabic), `CLAUDE.md` (add `i18n` to the crate list in "Project state"), `docs/deployment.md` (only if Zitadel's hosted login lacks Arabic: one short paragraph under the identity provider section)

- [ ] **Step 1: Check Zitadel.** With the dev stack up, open the Zitadel login with `ui_locales=ar` (start a web sign-in in Arabic). Record whether the hosted login renders Arabic. If not, add the paragraph to `docs/deployment.md` stating that the login page falls back to English and that `ui_locales` is already sent, so a future Zitadel release with Arabic needs no postit change.
- [ ] **Step 2: Update plan 03** as listed above.
- [ ] **Step 3: Run all gates**: Rust, web, app, and `cargo xtask openapi --check`. Expected: all PASS.
- [ ] **Step 4: Exit check.** In the running web app and on the emulator: sign in, switch the profile language to Arabic, visit every screen (home, profile, users, user detail, audit, jobs dashboard, jobs list, job detail, recurring), confirm right-to-left layout, Arabic text, Arabic-Indic digits, and no English except user content and developer details; switch back to English. Report the result screen by screen.
- [ ] **Step 5: Commit** `docs: record D0 preferences scope and sign-in language support`.
