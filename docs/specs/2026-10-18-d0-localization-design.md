# D0 — Localization foundation: design

Date: 2026-10-18. Source: plan 03 (`docs/contracts/03. publishing roadmap.md`), phase D0 and the "Localization" section. This document records intent, the decisions the plan left to phase start, and what is left to the builder.

## Intent

postit is for non-technical users, and later audiences will want their own language, some of them written right to left. Localizing after a dozen feature screens exist is far more expensive than now, while the UI is still the plan 02 shell (sign-in, home, profile, users, audit, jobs). D0 makes every existing screen, email, and error translatable and right-to-left capable, so every later phase adds strings to catalogs instead of retrofitting, and a new language, right-to-left or not, is a catalog plus one registration line.

## Goals

- Every existing web screen, mobile screen, server email, and API error takes its text from a catalog; no user-facing string is hard-coded.
- English (US, default and reference) catalogs on web, mobile, and server. Nothing in the design is specific to one language; another language is a new catalog file plus registering it, with its writing direction.
- Layouts are direction-neutral: `lang` and `dir` follow the active locale's direction, so a right-to-left language mirrors the UI with no code change.
- The server sends codes, not sentences; clients translate error codes.
- A CI check that every key exists in every catalog, on web, mobile, and server.

## Non-goals

- Languages beyond `en-US` (more come later, after R1).
- Translating user content, problem+json `detail` (stays English for developers), audit/job technical payloads, or developer pages (Swagger UI).
- Non-Gregorian calendars.
- The `/me/preferences` time zone, push, and email switches, and a time-zone picker (D1).

## Anti-goals

- Hard-coded `left`/`right` layout that breaks in right-to-left.
- English strings concatenated into sentences, or plurals done with `count === 1 ? … : …` (other languages have more plural forms).
- A runtime-loaded catalog that can fail and leave raw keys on screen.

## Constraints

- pnpm only for JS tooling (on Windows run through PowerShell; the Git Bash pnpm shim is broken). Flutter commands through PowerShell on Windows.
- Exact version pins in `web/package.json` (existing convention); workspace dependencies in Rust.
- Web CSP is sent by the server (P10): fonts and catalogs must be bundled, no external font or catalog hosts.
- One migration file until G1: `locale` is added to `backend/crates/data/migrations/0001_init.sql` in place; dev database reset.
- No dependency with an Arabic or Arabic-derived name.
- Existing tests and public signatures keep working; `cargo xtask openapi` regenerates the TS types and Dart client after the API change.

## Decisions

All library choices below are the designer's calls, made on the user's instruction to pick the most popular, stable, flexible, supported, and cheapest to maintain.

### Web (`web/`)

- **i18next + react-i18next.** Most used React localization stack, MIT, actively maintained. Catalogs in i18next's native JSON format, **no ICU plugin**: i18next's built-in plural handling uses `Intl.PluralRules`, which covers every CLDR plural form a later language needs. One fewer dependency, simpler files to translate.
- Catalogs are **bundled** (imported JSON), one namespace per feature area or a single namespace (builder's call), one folder per language (`en-US` now). Keys are type-checked from the English catalog through i18next's TypeScript resource declaration, so a missing or misspelled key fails `tsc`.
- **Locales:** one table in `web/src/i18n/locales.ts` names each supported language with its primary language subtag, its `Intl` formatting locale, and its direction. Any browser or account tag maps to a supported locale by its primary subtag, else `en-US`.
- **Language selection:** before sign-in, `i18next-browser-languagedetector` (official plugin) from the browser language, cached in `localStorage`. After sign-in, the profile's `locale` from `/me/preferences` wins, and changing it in the profile updates both the server and the UI immediately. *Designer's call:* a compact language switch also appears on the sign-in screen and the pending/disabled status screens (both clients), so the language can be chosen before the account exists or is approved. The switch, and the profile's language row, show only while more than one language ships.
- **Direction:** `lang` and `dir` on `<html>` follow the active language's direction. CSS uses logical properties: Tailwind `ms-`/`me-`/`ps-`/`pe-`/`start-`/`end-`/`text-start`/`text-end` and `rtl:` variants where logical utilities do not exist. Direction-sensitive icons (arrows, chevrons, "back", pager next/previous) carry `rtl:` mirroring; symmetric or meaning-bearing icons do not. Base UI components are checked for right-to-left behavior (menus, dialogs, popovers).
- **Formatting:** every `Intl` formatter (`DateTimeFormat`, `NumberFormat`, `RelativeTimeFormat`, including `web/src/lib/format.ts`, which is hard-coded to `'en'` today) takes the active locale's formatting locale. Times stay in the device's time zone in D0 (a saved time-zone preference arrives with D1).
- **Identifiers:** verbatim identifiers and technical values (job, request, and user IDs, cron expressions, HTTP status codes, error codes, raw JSON payloads) are developer details, isolated with `dir="ltr"` (`<bdi>`/`<code>`) so they never reorder inside right-to-left text. The admin Jobs console and Audit screens are in scope: their chrome, labels, states, and formatted quantities are translated; their identifiers and payloads follow this rule. Mobile follows the same rule.
- **User content:** text that can be in any script (display names, emails, later post text) renders with `dir="auto"`.
- **Error codes:** the existing error handling (`web/src/api/errors.ts`) maps every `ErrorCode` to a catalog key; unknown codes fall back to a generic translated message.

### Mobile (`mobile/`)

- `flutter_localizations` + gen-l10n with ARB files (`app_en.arb` now, one per language), generated `AppLocalizations`. Every existing string in `lib/ui` extracted, including `lib/ui/errors/messages.dart` error-code messages.
- `Directionality` follows the locale (MaterialApp does this once `supportedLocales`/`locale` are set), so a right-to-left language mirrors the app. Locale before sign-in from the device; after sign-in from `/me/preferences`; the profile screen gets the language switch, shown while more than one language ships. Insets and alignments are directional (`EdgeInsetsDirectional`, `AlignmentDirectional`).
- Direction-sensitive icons mirrored (Flutter's `Icons` with `matchTextDirection` where needed).
- Numbers and dates through `intl` with the active locale.
- User content with direction detection (`TextDirection` from the text, or `Bidi` helpers in `intl`).

### Server (`backend/`)

- **New leaf crate `postit-i18n`** (no internal dependencies) using **Project Fluent** (`fluent-bundle`, maintained by the Fluent project; plus `unic-langid` for language tags). Fluent handles every CLDR plural form natively and is the plan's default candidate. `.ftl` catalogs per language are embedded at compile time (`include_str!`), parsed once, and exposed through a small API: resolve a locale (with fallback to `en-US`), format a message with arguments, and the locale's direction for `dir`. A message missing in a later language falls back to `en-US` at runtime and is caught by the CI check. Whether to wrap with `fluent-templates` or use `fluent-bundle` directly is the builder's call; direct use is preferred if the wrapper adds little.
- **`postit-mail`:** the two plan 02 emails (`user_pending_approval`, `user_approved`, HTML and text) take their strings from `postit-i18n` in the recipient's locale; HTML templates set `lang` and `dir` and use logical properties only. Each crate keeps its own askama templates; only strings come from catalogs. The pending-approval email goes to admins, so it uses each admin's locale.
- **`user_preferences.locale`:** new `TEXT NOT NULL DEFAULT 'en-US'` column in `0001_init.sql`, constrained to supported locales at the API layer. Set at first sign-in from the request's `Accept-Language` (best supported match, else `en-US`). *Designer's call:* both clients send `Accept-Language` explicitly on every API request with the **language the UI is showing** (not the browser or device default; Dart's `HttpClient` sends none by default), so a language chosen before sign-in is the one stored. User provisioning happens in the `Auth` extractor through `postit-identity`'s claims path, which does not see headers today; the extractor passes the resolved locale into provisioning, and it is used only when the user and preferences rows are first created.
- **`GET/PUT /me/preferences`** (self access, behind `Auth` like `GET /me`, so **pending users can read and change it**; *designer's call*, so a pending user can switch language and their `user_approved` email uses it). Added in D0 because plan 03 places it in D1, but D0 needs a way to change the language. *Designer's call:* D0 exposes **`locale` only**; `timezone` and the push and email switches come with D1, which owns their validation and UI. Unsupported locale is a 422 `validation_failed`. OpenAPI, TS types, and Dart client regenerated.
- **Error codes audit:** all 18 existing `ErrorCode` values already have stable `snake_case` codes (`backend/crates/api/src/error.rs`); the audit confirms every API error path uses one (no free-text-only errors) and records the list both clients translate.

### Sign-in

- The web (`oidc-client-ts` extra query param) and mobile (`flutter_appauth` additional parameters) clients send `ui_locales` with the active language, so Zitadel's hosted login follows it for every language it supports.

### CI

- A check per catalog set that every key exists in every language and no catalog has keys the reference (`en-US`/`en`) lacks: web JSON, Flutter ARB, server `.ftl`. Runs in the existing `web`, `app`, and Rust CI jobs (builder decides script vs test; a Rust test in `postit-i18n` and a vitest test are natural fits; Flutter gen-l10n's untranslated-messages file can back the ARB check).

## Left to the builder

- Catalog key naming and namespace split.
- Exact component-by-component RTL fixes, and which icons count as direction-sensitive.
- Wrapping helpers (a `useFormat` hook, a Dart extension for formatters).
- `fluent-templates` versus direct `fluent-bundle`.
- Work order across web, mobile, and server; they are independent after the API change.

## Tests

- Web: vitest for `dir`/`lang` on `<html>` from the locale's direction, tag mapping, formatting through the locale, error-code translation; the catalog-completeness check; existing Playwright e2e keeps passing.
- Mobile: widget tests for direction, translated text, and formatting; catalog completeness; user content with right-to-left script reading right to left inside an English screen.
- Server: `postit-i18n` locale resolution and fallback, plural forms, direction; mail rendering with logical properties only; `/me/preferences` GET/PUT including invalid locale and the four-caller rule where applicable (self-only route: the owner and an unrelated user); first-sign-in locale from `Accept-Language`.
- Quality gates: Rust (`fmt`, clippy `-D warnings`, `check`, `test`), web (lint, typecheck, test, format check per `web/CLAUDE.md`), app (`dart format`, `flutter analyze`, `flutter test`), and `cargo xtask openapi --check`.

## Exit criteria

Every existing web and mobile screen, server email, and API error renders from catalogs, `lang` and `dir` follow the active locale's direction, and the catalog-completeness checks pass on every client and the server.
