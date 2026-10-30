# P11: Flutter mobile app — design

Date: 2026-10-16. Source: `docs/plans/02. foundation.md` phase P11 (and its "Mobile client" section). This spec records the decisions made when P11 started, the additive server changes, and the plan amendments. Where this spec is silent, plan 02 governs.

## Goal

A Flutter mobile app in `mobile/` (Android and iOS) that signs in through the IdP and shows the same pending, unavailable, and dashboard shell screens as the web app, plus user management for admins.

- On an Android emulator prepared by `mobile/tool/android-dev.ps1`, `admin@postit.com` signs in through Zitadel and lands on Home.
- `member@postit.com` signs in and sees the Pending screen. The admin approves the member **from the phone** in Users, and the member reaches Home after a refresh.
- Sign-out revokes the refresh token, ends the IdP session, and returns the app to Sign-in.
- The Rust quality gates, the web checks, `flutter analyze`, `flutter test`, and the `app` and `contract` CI jobs are green.

Out of scope:

- Publishing screens (plan 03 D6).
- Audit and Jobs on mobile; they stay web-only.
- A web backend-for-frontend or server-side web sessions. The web app keeps its P10 design.
- Verified iOS builds. The iOS project is scaffolded and kept ready, but there is no Mac or Apple developer account to build or test it.
- Store releases (a plan 01 non-goal).

## Decisions

- **Platforms.** Android is built and verified. iOS is scaffolded (schemes, bundle IDs, URL schemes, simulator certificate notes) and marked unverified until a Mac is available.
- **Toolchain.** Flutter 3.47.6 stable (Dart 3.13.5), pinned in `pubspec.yaml` (`environment.flutter`) and in CI. Java 21 (Temurin) for Gradle.
- **Stack.** `flutter_riverpod` 3 without code generation (`Notifier`/`AsyncNotifier`), `go_router`, `dio`, `flutter_appauth` (a wrapper over the native AppAuth-Android and AppAuth-iOS libraries) for authorization code with PKCE in the system browser, `flutter_secure_storage` for tokens, `shared_preferences` for the theme, and `flutter_svg` for the logo. Exact versions are verified at implementation start and pinned in `pubspec.lock`.
- **Dependency rule.** No package whose name is an Arabic or Arabic-derived word, in any ecosystem. This replaced the `oidc` package originally named in plan 02.
- **Separate OIDC clients.** Web, Android, and iOS each get their own client ID. Each platform can then be disabled or rotated alone, the token's `azp` identifies the platform, and each client lists only its own redirects. The mobile clients are Zitadel native applications.
- **Design language.** The P7.1 tokens, fonts, and logo carry over to mobile.
- **Admin on mobile.** Admins get the Users screen on mobile (approve, disable, enable, change role, delete or reject, pending badge). Audit and Jobs stay web-only.
- **Navigation order.** Home, Posts, and Calendar are the first three tabs once plan 03 D6 adds Posts and Calendar. In P11 the tabs are Home, Profile, and Users (admins only).
- **Android floor.** `minSdk` 29 (Android 10). `targetSdk` and `compileSdk` follow Flutter's defaults.
- **Release signing.** One keystore per environment in the vault. CI builds qa as a debug build and needs no secret.
- **Folder layout.** Layered (`data/`, `state/`, `ui/`), not feature-first, so UI stays separate from data access.
- **Dart client generator.** `swagger_parser` followed by `build_runner`, with the output committed. The fallback is `openapi-generator` `dart-dio` if the output is not byte-stable across Windows and Linux.

## Server and tooling changes

All changes are additive and keep the `/api/v1` contract compatible.

### `postit-config`

- New optional keys `auth.oidc.android_client_id` and `auth.oidc.ios_client_id`. `auth.oidc.client_id` stays the web client.
- `auth.oidc.allowed_clients` defaults to every configured client ID: the web one, plus the Android and iOS ones when set. An explicit `allowed_clients` still overrides the default.
- Validation: the configured client IDs must all be different, and the error names the duplicated keys.
- `backend/config/qa.toml` and `production.toml` get commented placeholders for the two keys. `docs/configuration.md` documents them.

### `postit-api`

- `GET /api/v1/auth/config` gains `android_client_id` and `ios_client_id` (`string | null`). The app picks the one for its platform.
- `backend/openapi.json` and `web/src/api/schema.d.ts` are regenerated. The web app ignores the new fields.

### `cargo xtask zitadel-bootstrap`

- Creates two native OIDC applications, idempotently, in the `postit` project: `postit-android` and `postit-ios`. Each has PKCE, JWT access tokens, and the refresh-token grant, with redirect URI `postit-dev://auth/callback` and post-logout redirect URI `postit-dev://auth/logout`.
- `devMode` is off for the native apps. If the first spike shows Zitadel requires it for custom schemes, development turns it on, and `docs/deployment.md` records what qa and production need.
- Writes both client IDs into `backend/config/local.toml`.
- The web application (`postit-app`) keeps only the web and Swagger redirects.

### `cargo xtask openapi`

- After writing `backend/openapi.json` and the TypeScript types, it runs `swagger_parser` and then `build_runner` in `mobile/`, writing the Dart client into `mobile/lib/api/`.
- `--check` also diffs the Dart output after normalizing CRLF to LF.
- A missing Flutter SDK, or `mobile/` dependencies that were never fetched, fail with a message naming the fix (`flutter pub get` in `mobile/`).
- `--skip-dart` skips the Dart step in write mode, with a warning, for machines without Flutter. `--check` never skips it, so CI still catches a stale Dart client.
- The generated files are excluded from `dart format` checks and from analyzer lints that do not apply to generated code.

## App structure (`mobile/`)

```text
mobile/
  lib/
    main.dart
    api/                 generated client and models, never edited by hand
    data/
      config/            AppConfig from the dart-define file
      http/              dio instance, auth interceptor, problem+json -> ApiError
      auth/              SessionManager, SessionStore, AuthClient (flutter_appauth seam)
      repositories/      AuthConfigRepository, MeRepository, UsersRepository, PreferencesRepository
    state/               Riverpod notifiers: session, me, users, theme
    ui/
      router.dart        go_router with redirects from session and me
      theme/             tokens, ThemeData (light, dark), ThemeExtension
      widgets/           shared widgets
      screens/           sign_in, config_error, not_configured, pending, unavailable, home, profile, users
      errors/            error code -> message map
  config/                development.json, qa.json, production.json (API_BASE_URL, ENV)
  assets/                fonts, logo SVGs
  tool/                  android-dev.ps1, android-dev.sh
  test/                  unit and widget tests
  integration_test/      opt-in smoke flow against the dev stack
  android/, ios/
```

The app is built with `--flavor <env> --dart-define-from-file=config/<env>.json`. One `main.dart` serves every flavor.

## Auth and HTTP

### Startup

1. Read `AppConfig` (`API_BASE_URL`, `ENV`).
2. Call `GET /api/v1/auth/config` and pick the client ID for the current platform.
    - If that ID is null, show the "Mobile sign-in is not configured" screen.
    - If the call fails, show an error screen with a retry button.
3. If secure storage holds a refresh token, refresh it.
    - If the IdP rejects it (an OAuth error such as `invalid_grant`), clear storage and go to `signedOut` without an error message.
    - If the IdP cannot be reached (offline, DNS, TLS, timeout, `server_error`, `temporarily_unavailable`), keep the stored session and show the error screen with Retry.
4. Call `GET /me` to choose the screen (see Gating).

### Sign-in

- `flutter_appauth` `authorizeAndExchangeCode` with the issuer's discovery URL, the platform's client ID, redirect `postit-<flavor>://auth/callback` (`postit-dev`, `postit-qa`, `postit`), and the scopes from `/auth/config` (which include `offline_access`). PKCE is automatic, and the login opens in the system browser.
- The refresh token and the ID token go to `flutter_secure_storage` (Keystore on Android, Keychain on iOS). The access token and its expiry stay in memory only.
- A cancelled sign-in returns to the Sign-in screen with no message. An IdP error shows a short message with the IdP's `error_description` when present, and never retries automatically.

### Refresh

- `SessionManager` owns one single-flight refresh `Future` that every caller shares.
- A refresh also runs ahead of time when the access token is within 60 seconds of expiry.
- A rejected refresh clears storage and moves the session to `signedOut` with "Your session expired" on the Sign-in screen. A refresh that cannot reach the IdP keeps the session and fails the request as a network error ("Can't reach postit").
- A sign-out wins over a refresh in flight: tokens that arrive after the sign-out are discarded and never written to storage.

### HTTP interceptor

- Attaches `Authorization: Bearer <access token>`.
- On 401 it asks `SessionManager` for one refresh and retries the request once. If the refresh is rejected or the retry is also 401, the session is cleared as above. If the IdP cannot be reached, the request fails as a network error and the session is kept.
- Problem+json responses become `ApiError { status, code, detail }`. The error map covers every plan 02 code. An unknown code shows the server's `detail`.

### Sign-out

1. Revoke the refresh token at the discovery document's `revocation_endpoint`, through dio.
2. Call `flutter_appauth` `endSession` with the ID token hint and `postit-<flavor>://auth/logout`.
3. Clear secure storage and in-memory state.

Local state is cleared even if revocation fails or the user cancels the browser step.

### Account deletion

Profile's "Delete my postit account" asks the user to type their display name, calls `DELETE /me`, and then runs sign-out.

### `AuthClient` seam

`flutter_appauth` is used only through a small `AuthClient` interface (authorize, refresh, end session), so tests use a fake and a later library swap touches one file.

## Gating

Same rules as the web app (P7 spec, "Gating"):

- Signed out: the Sign-in screen.
- `/me` 200 with `pending`: the Pending screen, with pull-to-refresh, a refresh button, and sign-out.
- `/me` 403 `account_disabled`: the Unavailable screen ("This account is disabled or being deleted. Contact an administrator.") with sign-out.
- `/me` 200 with `active`: the shell. Users requires `role == admin`.
- An `account_pending`, `account_disabled`, or `forbidden` response mid-session invalidates `me` and moves the user to the matching screen, so a just-demoted admin loses the Users tab.

## Screens and navigation

- **Shell.** A bottom `NavigationBar` on screens narrower than 600 dp, and a `NavigationRail` from 600 dp. Tabs: Home, Profile, and Users for admins, with a pending-count badge on Users. When plan 03 D6 adds Posts and Calendar, they take slots 2 and 3, and Profile and Users move into the account menu or a "More" tab.
- **Sign in.** The logo and one button that opens the IdP.
- **Home.** Date line and time-of-day greeting, the single-admin warning (admins only, from `GET /users?role=admin&status=active`), an approval queue card for admins (pending count and a Review button that opens Users filtered to pending), and the account card. The team summary and the activity timeline from the web mockup are left out because they need the audit feed.
- **Profile.**
    - Name and email from the IdP, read-only.
    - A link to `account_url` that opens in the external browser when one is configured.
    - The theme control: three segments for system, light, and dark.
    - Sign-out.
    - A danger zone with "Delete my postit account".
- **Users (admins only).**
    - A list with infinite scroll (`emixdb` pagination), a search field, and status filter chips. Pending users come first: the default filter is pending when any exist.
    - Tapping a user opens a detail screen with the actions the user's state allows: approve, disable, enable, change role, and delete (reject for pending users).
    - Destructive actions ask for confirmation.
    - Server errors such as `last_admin`, `user_deleting`, and `validation_failed` show the mapped messages.

## Theme

- The P7.1 palette (`--bg`, `--surface`, `--raised`, `--line`, `--text`, `--muted`, `--faint`, `--pink`, `--pink-text`, `--pink-soft`, `--on-pink`, the `--warn-*` set, `--track`, `--disabled`) maps onto a Material 3 `ColorScheme` for each theme. A `ThemeExtension` holds the tokens Material has no slot for.
- Accent pink `#e0679f`. Focus and selection use it.
- Bricolage Grotesque (headings and large figures), Onest (body), and JetBrains Mono (labels, codes, counts, timestamps) are bundled as font assets under the SIL Open Font License, never fetched at runtime.
- The logo SVGs from `assets/` (the wordmark and the badges) render through `flutter_svg`. The launcher icons use the badge.
- The theme choice is stored with `shared_preferences` under `postit.theme`. The default is `system`, which follows the OS setting live. A storage failure falls back to `system`.
- Touch targets are at least 48 dp. Reduced motion follows the OS setting. Both themes meet WCAG AA contrast.

## Platform projects

### Android

- Product flavors `development`, `qa`, and `production` with application IDs `io.github.asm2025.postit.dev`, `io.github.asm2025.postit.qa`, and `io.github.asm2025.postit`, so all three install side by side. Each sets its `appAuthRedirectScheme` manifest placeholder (`postit-dev`, `postit-qa`, `postit`) and its own app label.
- `minSdk` 29.
- `android.permission.INTERNET` is declared in the main manifest (the Flutter template only declares it for debug and profile builds).
- **Dev certificate trust.** It has two parts, because Dart's `HttpClient` (used by dio) ignores `network_security_config` and trusts only the system roots.
    - Native code (AppAuth's token calls): the `development` source set holds `network_security_config.xml`, which trusts the dev CA as a raw resource. It applies to debug builds only, and it trusts that one CA, not every user-installed CA.
    - Dart code (dio): the CA is a Flutter asset under `assets/dev/`, declared for the `development` flavor only. In debug development builds the app loads it into a `SecurityContext` (system roots plus the dev CA) for dio. Every other build uses the system roots.
    - A Gradle task, wired through the variant API, copies `docker/shared/nginx/certs/postit-dev-ca.crt` into generated resources and into `mobile/assets/dev/` (git-ignored) for the development flavor. If the file is missing, a development build fails with a message pointing at `cert.ps1` / `cert.sh`.
    - The `qa` and `production` flavors contain no trust configuration, resource, or asset, and CI enforces that for both.
- **Release signing.** Release builds of `qa` and `production` read `android/key.properties`, which points at `!ref/vault/<environment>/android.jks` and its passwords in `!ref/vault/<environment>/android.env`. Without it a release build fails (changed after the P11 review; `-Ppostit.allowDebugSigning=true` allows a local debug-key release). `key.properties` is git-ignored. The maintainer creates the keystores and re-packs the vault. Claude never re-packs it.

### iOS (scaffolded, unverified)

- One xcconfig and scheme per flavor, with bundle IDs matching the Android application IDs and the flavor's URL scheme in `CFBundleURLTypes`.
- The deployment target is the oldest iOS that both Flutter 3.47 and `flutter_appauth` support, checked at implementation start.
- The README documents installing the dev CA profile on the simulator and states that the iOS build is unverified.

## Development tooling: `mobile/tool/android-dev.ps1` and `android-dev.sh`

Both scripts offer the same subcommands, in the style of `cert.ps1` / `cert.sh`.

- **Environment.**
    - They read `ANDROID_HOME` (SDK tools) and `ANDROID_USER_HOME`. AVDs live in `ANDROID_USER_HOME/avd`, and `ANDROID_AVD_HOME` overrides that directory.
    - The AVD name is a positional argument, defaulting to `POSTIT_AVD`.
    - Missing tools or variables fail with a message naming the fix.
- **`list`**: AVDs with their system image tag. Google Play images are marked as not preparable, because they cannot be rooted.
- **`prepare <avd>`**: one-time setup, idempotent. It refuses a Play image (it reads `tag.id` from the AVD's `config.ini`), then:
    1. Boots the AVD with `-writable-system` and waits for `sys.boot_completed`.
    2. Runs `adb root`, `avbctl disable-verification`, a reboot, then `adb root` and `adb remount`.
    3. Appends `10.0.2.2 postit.local` to `/system/etc/hosts` unless the line is already there. `10.0.2.2` is the emulator's alias for the host loopback, so no `adb reverse` is needed, and the entry survives restarts.
    4. Pushes the dev CA into the user CA store as `/data/misc/user/0/cacerts-added/<subject_hash_old>.0`, then sets its owner, mode, and SELinux label. Chrome, which runs the Zitadel login in a Custom Tab, trusts user CAs.

    It leaves the emulator running. Every `adb` call targets the emulator's serial (`adb -s`), matched to the AVD name, so other running devices are never touched.
- **`start <avd>`**: boots with `-writable-system`. The `/system` changes are visible only when the AVD boots this way.
- **`clear <avd>`**: wipes the AVD's data (`-wipe-data`). It asks for confirmation unless `-Force` (`--force`) is given.
- **`reset <avd>`**: `clear`, then `prepare`.
- **`help`**: usage.
- **The CA hash.** PowerShell computes `subject_hash_old` in .NET: the MD5 of the DER-encoded subject, first four bytes read little-endian, as eight lowercase hex digits. No OpenSSL needed. The shell script uses `openssl x509 -subject_hash_old`.
- **Physical devices** cannot be rooted for a hosts edit. The README covers `adb reverse` for USB-connected devices and a LAN DNS or router hosts entry for `postit.local`, plus installing the dev CA through Settings.

## Testing

- **Rust.**
    - Config validation: the client IDs must all differ, and `allowed_clients` defaults to every configured ID.
    - `GET /auth/config` returns the mobile IDs, both when set and when null.
    - The verifier accepts a token from each configured client (`azp` per client) and rejects an unlisted client.
    - End to end, through the real server started from config (the `postit-server` smoke harness with the test issuer): with only the Android client configured, tokens from the web and Android clients are accepted, and one from the unconfigured iOS client gets 401. This is the only test of the composition root's client rule over HTTP.
- **Flutter unit tests**, using a fake `AuthClient`, a fake `SessionStore`, and a mocked dio adapter.
    - `SessionManager`: restoring a session from a stored refresh token, a rejected restore going to `signedOut`, an unreachable IdP keeping the stored session (on restore and on refresh), single-flight refresh under concurrent callers, the early refresh near expiry, sign-out revoking and clearing, clearing even when revocation fails or `endSession` is cancelled, and a sign-out during a refresh or a store write not reviving the session.
    - The interceptor: attaching the token, one refresh and retry on 401, sign-out on a second 401, and a network error (session kept) when the IdP is unreachable.
    - Dev trust: never loaded outside debug development builds, and a missing or bad asset falls back to the system roots.
    - Problem+json parsing and the error map, including an unknown code.
    - `AppConfig` loading and picking the client ID per platform.
- **Widget tests.**
    - Redirects for every state: signed out, pending, unavailable, active member, and active admin. A member never sees Users.
    - The config error screen with retry, and the not-configured screen.
    - Users: filters, paging, each action with its confirmation, and the `last_admin` error message.
    - Profile: deletion with the typed-name check, and sign-out.
    - The theme control: persistence, and the fallback when storage throws.
    - The navigation bar below 600 dp and the rail from 600 dp.
- **`integration_test` smoke flow** against the dev stack with Zitadel: opt-in, not in CI, documented in the README.

## CI

- **`app` job (`ubuntu-latest`):**
    1. Set up Temurin 21 and Flutter 3.47.6, both pinned.
    2. Run `flutter pub get`, `dart format --set-exit-if-changed` (hand-written code only, never `lib/api`), `flutter analyze`, and `flutter test`.
    3. Run `flutter build apk --debug` for the `qa` and `production` flavors, each with its dart-define file.
    4. For both APKs, assert with `aapt2 dump xmltree` on the manifest that it has no `networkSecurityConfig` attribute, and assert from the APK listing that it holds no `network_security_config` resource, no dev CA resource, and no dev CA Flutter asset.
- **`contract` job:** installs Flutter and runs `cargo xtask openapi --check`, which now covers the Dart client.

## Phase-start spikes

These run first. Each has a recorded fallback.

1. `flutter_appauth` against a Zitadel native application on the Android 17 Google APIs emulator: PKCE sign-in, the custom-scheme redirect, refresh, and `endSession`. It also settles whether native applications need `devMode`, records how an unreachable IdP and a rejected refresh token differ in the plugin's error details, and confirms that dio needs the bundled dev CA. If a step fails, the spike diagnoses it before anything else is built, and a library change goes back to the user.
2. Chrome on Android 17 trusting a CA in the user store. Fallback: the README documents installing the CA through Settings, and `prepare` only pushes the file to `/sdcard/Download`.
3. `swagger_parser` and `build_runner` output being byte-identical on Windows and Linux. Fallback: `openapi-generator` `dart-dio`, run through its Docker image, with the cost recorded in this spec.

## Spike results (2026-10-17)

Run on the `Pixel_10a_API` AVD (Android 17, Google APIs, `-writable-system`) with `flutter_appauth` 12.1, the `development` flavor, and the native Zitadel app `postit-android` created by `cargo xtask zitadel-bootstrap` (Zitadel v4.19.1).

- **Sign-in (spike 1, step 1).** PKCE sign-in works. The Zitadel login opens in a Chrome Custom Tab, the `postit-dev://auth/callback` redirect returns to the app, and the log shows `sign in: ok` with a refresh token. Native apps did **not** need `devMode`: `devMode: false` with a custom-scheme redirect was accepted. The access token expires 12 hours after issue. AppAuth's native token exchange trusts the dev CA through `network_security_config`.
- **Chrome and the dev CA (spike 2).** Chrome loaded `https://postit.local:44300` with no certificate warning, so the user-store CA installed by `prepare` works. No `/sdcard/Download` fallback is needed.
- **`taskAffinity` breaks the redirect.** The `flutter create` template sets `android:taskAffinity=""` on `MainActivity`. With it, the redirect lands in a separate task and AppAuth reports `FlutterAppAuthUserCancelledException` even though Zitadel completed the flow (Zitadel's log shows the callback 302). Removing the attribute fixes sign-in, and Task 5 removes it from `mobile/android/app/src/main/AndroidManifest.xml`. A Task 8 reader should treat a spurious "cancelled" on Android as this problem first.
- **dio and the dev CA (step 2).** `GET /api/v1/me` returned 200 through a `Dio` whose `IOHttpClientAdapter` uses a `SecurityContext` loaded from the bundled `assets/dev/postit-dev-ca.crt`. A plain `Dio()` failed with `HandshakeException ... CERTIFICATE_VERIFY_FAILED: unable to get local issuer certificate`, which confirms Dart ignores `network_security_config` and needs the bundled CA (Task 9's `devTrust`). The first `GET /me` returned 401 because the running server container predated the mobile client IDs; a server built from this branch returned 200.
- **Refresh (step 3).** `token` with a refresh token succeeds, and the refresh token **rotates** on every refresh. The stored refresh token must be replaced after each refresh.
- **Unreachable (step 4).** With airplane mode on, and again with the Zitadel container stopped, `token` throws `FlutterAppAuthPlatformException` with `type=0 code=3 error=null description=Network error`. There is no OAuth `error`.
- **Rejected (step 6).** A refresh token Zitadel does not accept gives `type=2 code=2000 error=invalid_request description=Errors.User.RefreshToken.Invalid`. Zitadel answers with `invalid_request`, not `invalid_grant`. Task 8's `_isRejection` rule (`error` is one of the OAuth error codes, which already includes `invalid_request`) separates the two cases as written, so it is unchanged: `error == null` means unreachable and keeps the session, a non-null OAuth `error` means rejected and clears it. This case was produced with a refresh token Zitadel never issued, not a revoked one (see the ruling in the plan ledger), and the response should be identical for a revoked token.
- **End session (step 5).** `endSession` returns to the app on `postit-dev://auth/logout`. The next Sign in shows Zitadel's account picker with the user marked "Signed out" and asks for credentials again.
- **Cancel (step 7).** Pressing back in the Custom Tab throws `FlutterAppAuthUserCancelledException`.
- **Second factor.** The first sign-in went straight through with the password. The sign-in after End session asked for a second factor (Zitadel's "Verify 2-Factor" code step), which the spike could not satisfy, so step 6 used the unissued-token case above. The prompt was Zitadel's MFA setup on the seed user, not a policy postit needs: `cargo xtask zitadel-bootstrap` now sets a login policy that never forces or prompts for a second factor, so a later run can use a really revoked token for step 6.
- **Emulator notes.** The 2 GB default RAM let the low-memory killer kill the backgrounded app while Chrome ran, which loses the in-flight sign-in. The AVD was raised to 6 GB. The Zitadel console loads slowly on a fresh emulator boot (about two minutes for the first page).

## Documentation

- **README:** a mobile quick start (prerequisites, `android-dev.ps1`, running each flavor, the opt-in smoke test), physical devices, and the iOS notes, marked unverified.
- **`docs/deployment.md`:** a native application per environment and platform (redirect and post-logout URIs per flavor), `allowed_clients`, release signing from the vault, and building the mobile apps from source. It replaces the "P11 concern" note.
- **`docs/configuration.md`:** `auth.oidc.android_client_id` and `auth.oidc.ios_client_id`.
- **`docs/security.md`:**
    - The refresh token stored in Keystore or Keychain, recorded as an accepted risk with its mitigations: revocation on sign-out and rotation by the IdP.
    - The debug-only trust configuration, enforced in CI.
    - Separate client IDs, with `allowed_clients` limiting which clients the API accepts.
- **`CLAUDE.md`:** P11 marked done when the phase completes.

## Plan amendments

- **Plan 02:**
    - The objective and the Mobile client section gain user management for admins on mobile, the separate web, Android, and iOS client IDs, `minSdk` 29, release signing from the vault, and the `android-dev` scripts.
    - The Zitadel bootstrap bullet now names the native mobile applications.
    - The `app` CI job names Temurin 21.
    - The P11 exit criteria add approval from the phone.
- **Plan 03 D6:** "Admin screens stay on the web" becomes "User management is on mobile from P11; Audit, Jobs, and deployment settings stay on the web". Home, Posts, and Calendar are the first three tabs.

## Deviations

1. iOS build configurations and schemes are created in Xcode on the first Mac session. Only the flavor xcconfigs and `Info.plist` are committed, because editing `project.pbxproj` without Xcode is error-prone.
2. The generated Dart client names the inline item type of `PageUserDto.data` `Data3`, and the generated enums have a `json` field but no `toJson()`; the app maps `Data3` to `UserDto` in `mobile/lib/state/users.dart` and passes enum query parameters as `.json` strings.
3. Riverpod 3 exports `Override` from `package:flutter_riverpod/misc.dart`, and provider types in a cycle (`apiDioProvider`, `meRepositoryProvider`, `meProvider`) are annotated explicitly.
4. The logo SVGs carry inline fills instead of CSS classes, because `flutter_svg` ignores `<style>`; the wordmark's ink is swapped through a `ColorMapper`.
5. The exit flow was verified on 2026-10-17 on the `Pixel_API_35` AVD (API 35, Google APIs, x86_64) with the user: admin sign-in and sign-out, a new self-registered user held as pending and approved from the phone, the session kept across an app restart, the offline error and Retry, and the opt-in smoke test. The signed-release half of the signing check was not run, because the vault holds no Android keystore yet.
6. Sign-out clears local state first (storage wiped, state signed out), then revokes the refresh token and ends the IdP session, both best effort. The review found that clearing last let a killed process or a late refresh bring the session back.
7. The startup screen and launcher icon use the logo's black, `#231F20`, behind the white "p" and pink strokes, through `flutter_native_splash` and `flutter_launcher_icons`. The in-app loading screen continues it with the same mark, so launch does not flash. The first version put the badge in a pink circle, which the user rejected.
8. Dev Zitadel gained an optional `ZITADEL_ALLOW_REGISTRATION` switch (registration allowed when unset) and an SMTP provider for the host SMTP tool, both set by `cargo xtask zitadel-bootstrap`; email verification for self-registered users needs the second.
