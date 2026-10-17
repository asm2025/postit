# P11 Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish plan 02 P11. The exit flow has to run on the emulator, the startup screen and launcher icon have to match the logo, the emulator scripts have to stop exporting `ANDROID_AVD_HOME`, the emulator has to be faster, and dev Zitadel has to stop asking for a second factor and gain an `allowRegister` switch.

**Architecture:** The work lands on the existing `p11-flutter-app` branch, on top of `ea30d02`, through four channels:
- `flutter_native_splash` and `flutter_launcher_icons` config produce the startup screen and icons.
- The `app/tool/android-dev.*` scripts change in place.
- One new step in `cargo xtask zitadel-bootstrap` sets the Zitadel login policy.
- A human-in-the-loop run on the emulator covers the exit flow.

**Tech Stack:** Flutter 3.47.6, `flutter_native_splash`, `flutter_launcher_icons`, PowerShell and Bash, and the Rust xtask (reqwest, serde_json, `dotenvy`). The emulator is the Android Emulator with Google APIs images.

**Spec:** `docs/superpowers/specs/2026-10-16-p11-flutter-app-design.md`, plus the decisions below. The user made them on 2026-10-17 after reviewing the first P11 run.

## Global Constraints

- **Branch:** work on `p11-flutter-app`. Local `stage` stays at `origin/stage` until the user picks how to integrate.
- **Commit trailer:** `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Startup screen:** the background is the P7.1 `--pink` token, `#E0679F`, in light and dark mode. The logo is the original `postit-sb` badge (black rounded square, white "p", pink strokes), centered. No circle or other shape may show around it.
- **Launcher icon:**
  - The adaptive background is `#231F20` (the logo's black).
  - The foreground is the logo's white "p" and pink strokes, without the black square.
  - The legacy and iOS icons stay `assets/logo/postit-sb.png`.
- **Emulator scripts:**
  - They read `ANDROID_AVD_HOME` and never set or export it.
  - They fall back to `ANDROID_USER_HOME/avd`, then `~/.android/avd`.
  - When an AVD is found only in a folder the emulator does not search, they stop with a message telling the user to set `ANDROID_AVD_HOME`.
- **Dev Zitadel:**
  - The seed users are never asked to set up or verify a second factor.
  - Registration follows `ZITADEL_ALLOW_REGISTRATION`, which is optional and allowed when unset.
- **Human steps:** whenever a step needs a person (a code, a visual check, a choice), stop and ask the user. Never skip the step, time out, or record it as a blocker.
- **Gates:**
  - App: format, `flutter analyze`, `flutter test` from `app/`, through PowerShell.
  - Rust: the four Rust gates plus `cargo xtask openapi --check`, from `server/`.
- **Vault:** no step reads, prints or re-packs vault values.
- **Packages:** use pnpm only. Never add a dependency with an Arabic or Arabic-derived name.

## Review Focus

1. **`ANDROID_AVD_HOME` unset, AVD only under `ANDROID_USER_HOME/avd`.** `start` must stop with the "set ANDROID_AVD_HOME" message and must not change the environment. This is pinned in Task 1, Step 4.
2. **Dark mode at launch.** The startup screen stays pink with the badge, with no white flash and no circle. This is pinned in Task 4, Step 5.
3. **`ZITADEL_ALLOW_REGISTRATION=maybe`.** The bootstrap must fail and name the variable, not silently allow. This is pinned in Task 3 (`allow_registration_rejects_other_values`).
4. **Re-running `zitadel-bootstrap`** when the org already has a custom login policy (the user changed MFA in the console). The bootstrap must update that policy, not fail creating a second one. This is pinned in Task 3 (`login_policy_request_updates_a_custom_policy`).
5. **"Sign in again" while offline.** The button clears the stored session even though the IdP can't be reached, and lands on Sign in, not on an error. This is pinned in Task 5 (`sign in again clears the stored session`).

---

### Task 1: Emulator scripts: no exported `ANDROID_AVD_HOME`, boot log and timing

**Files:**
- Modify: `app/tool/android-dev.ps1`. That means `Get-AvdHome` (~line 45), `Wait-Boot` (~80), `Start-Avd` (~90), the help text (~190), and the lines that set `ANDROID_AVD_HOME` (~207-208).
- Modify: `app/tool/android-dev.sh`, the matching functions: `avd_home` (~17), `wait_boot` (~48), `start_avd` (~57), `usage` (~142), and the export (~162-164).

**Interfaces:**
- Produces: `start` prints `Booted <avd> in <seconds> s.` and writes the emulator console to `<temp>/postit-emulator-<avd>.log`. The boot timeout is `POSTIT_BOOT_MINUTES` (default 20). Task 2 uses the printed seconds.

- [ ] **Step 1: Resolve the AVD folder without changing the environment.**
  - Replace `Get-AvdHome` / `avd_home` with `Find-AvdDir <name>` / `find_avd_dir <name>`.
  - It returns the first of these folders that contains `<name>.avd/config.ini`: `$ANDROID_AVD_HOME`, then `$ANDROID_USER_HOME/avd`, then `~/.android/avd`.
  - If the match is not `$ANDROID_AVD_HOME`, and it is not `~/.android/avd` while `ANDROID_AVD_HOME` is unset, stop with this text, exactly:
    `The emulator does not search <dir>. Set ANDROID_AVD_HOME=<dir> (user environment variable) and open a new terminal.`
  - Delete both `$env:ANDROID_AVD_HOME = ...` and `export ANDROID_AVD_HOME`, with their comment lines.
  - `list` keeps showing AVDs from all three folders and marks each with the folder it came from.

- [ ] **Step 2: Capture the boot log and time the boot.**
  - `Start-Avd` / `start_avd` redirects the emulator's stdout and stderr to `<temp>/postit-emulator-<avd>.log` and overwrites it on each start. `<temp>` is `$env:TEMP` or `${TMPDIR:-/tmp}`.
  - `Wait-Boot` / `wait_boot`:
    - polls `sys.boot_completed` until `POSTIT_BOOT_MINUTES` (default `20`) runs out;
    - on success, prints `Booted <avd> in <seconds> s.`;
    - on timeout, prints the last 40 lines of the log, then stops with `<avd> did not finish booting in <n> minutes. Full log: <path>`.
  - Add one array, `$script:BadBootPatterns` / `bad_boot_patterns=()`, empty, with a comment:
    `# Lines that mean this boot will never finish; add the message when it is seen again.`
  - On each poll, if the log matches any pattern, kill the emulator (`adb emu kill`), print the matching line, and stop with `Bad boot detected; run start again.`

- [ ] **Step 3: Update the help text.** Say where AVDs are found, that the script never sets `ANDROID_AVD_HOME`, and describe `POSTIT_BOOT_MINUTES` and the log path.

- [ ] **Step 4: Verify (Review Focus 1).** From PowerShell:

```powershell
pwsh -NoProfile -Command '$env:ANDROID_AVD_HOME=$null; $env:ANDROID_USER_HOME="D:\Android\.android"; ./app/tool/android-dev.ps1 start Pixel_10a_API'
```

  Expected: stops with `The emulator does not search D:\Android\.android\avd. Set ANDROID_AVD_HOME=...`, and the calling shell's `ANDROID_AVD_HOME` is unchanged.

  Run the same in Git Bash with `ANDROID_AVD_HOME= ANDROID_USER_HOME=/d/Android/.android ./app/tool/android-dev.sh start Pixel_10a_API`. Expected: the same message.

  Then, with the user's real environment, run `./app/tool/android-dev.ps1 start Pixel_10a_API`. Expected: the AVD boots, `Booted Pixel_10a_API in <n> s.` is printed, and the log file exists. Record `<n>`; it is Task 2's baseline. If the AVD is already running, ask the user before killing it.

- [ ] **Step 5: Commit**

```bash
git add app/tool/android-dev.ps1 app/tool/android-dev.sh
git commit -m "fix(app): android-dev scripts read ANDROID_AVD_HOME, log and time the boot"
```

---

### Task 2: Faster emulator

**Files:**
- Modify: `README.md`, in the "Mobile app (Android)" prerequisites: the recommended image and AVD settings.
- Modify: `app/tool/android-dev.ps1` / `.sh` only if Step 4 shows `-writable-system` can be dropped (see there).

**Interfaces:**
- Consumes: Task 1's `Booted <avd> in <seconds> s.` output.
- Produces: a recommended AVD named `Pixel_API_35` that the user keeps or discards.

- [ ] **Step 1: Ask the user** before downloading about 1.5 GB: "Install `system-images;android-35;google_apis;x86_64` and create AVD `Pixel_API_35`?" Continue only on yes.

- [ ] **Step 2: Create the AVD.**

```powershell
& "$env:ANDROID_HOME\cmdline-tools\latest\bin\sdkmanager.bat" "system-images;android-35;google_apis;x86_64"
& "$env:ANDROID_HOME\cmdline-tools\latest\bin\avdmanager.bat" create avd -n Pixel_API_35 -k "system-images;android-35;google_apis;x86_64" -d pixel_8
```

Then set these in `$env:ANDROID_AVD_HOME\Pixel_API_35.avd\config.ini`:
- `hw.ramSize=6144`
- `hw.cpu.ncore=8`
- `hw.gpu.enabled=yes`
- `hw.gpu.mode=host`
- `disk.dataPartition.size=10G`
- `hw.keyboard=yes`

- [ ] **Step 3: Measure.**
  - Run `./app/tool/android-dev.ps1 prepare Pixel_API_35`.
  - Then run `start` twice. The first is a cold boot; the second start comes after `adb emu kill`.
  - Record each `Booted ... in <n> s.` against Task 1's baseline.
  - Also time the dev app from `am start` until its Sign-in screen shows (`adb shell am start -W` prints `TotalTime`), on both AVDs.
  - Ask the user whether System UI still stops responding.

- [ ] **Step 4: Snapshots.**
  - Check whether the second `start` restored a snapshot. The log has `Loading snapshot 'default_boot'` with no `Failed to load` after it.
  - If `-writable-system` blocks snapshots, say so in the README and keep `-writable-system`: the hosts entry needs it.
  - Do not build a DNS alternative in this plan. Mention the LAN DNS option already in the README.

- [ ] **Step 5: Record and recommend.**
  - In the README prerequisites, name the image (`google_apis` x86_64, API 35, not a `ps16k` image) and the config values from Step 2, with a one-line note: images above API 35 log "Guest Angle is still unstable" and boot and run slower.
  - Give the user the measured numbers.
  - Leave the old AVD in place; deleting it is the user's call.

- [ ] **Step 6: Commit**

```bash
git add README.md app/tool
git commit -m "docs(app): recommend an API 35 x86_64 AVD with host GPU for speed"
```

---

### Task 3: Dev Zitadel login policy: no second factor, optional registration

**Files:**
- Modify: `server/xtask/src/zitadel_bootstrap.rs`. Add the new functions next to `ensure_builtin_login` (~323), call them from `run()` (~61), and add tests in the existing `mod tests` (~385).
- Modify: `server/xtask/Cargo.toml`. Add `dotenvy` (latest, as a direct dependency of xtask only).
- Modify: `docker/.env.example`. Add the commented variable.
- Modify: `docs/deployment.md`. In "Zitadel production setup", add a step for registration and MFA in the login policy.

**Interfaces:**
- Produces:
  - `fn allow_registration(value: Option<&str>) -> Result<bool>`
  - `fn login_policy_request(current: &Value, allow_register: bool) -> (reqwest::Method, &'static str, Value)`
  - `async fn ensure_login_policy(client: &reqwest::Client, token: &str, allow_register: bool) -> Result<()>`

- [ ] **Step 1: Write the failing tests** in `mod tests`:

```rust
#[test]
fn allow_registration_defaults_to_true() {
    assert!(allow_registration(None).unwrap_or_else(|e| unreachable!("{e}")));
    assert!(allow_registration(Some("")).unwrap_or_else(|e| unreachable!("{e}")));
}

#[test]
fn allow_registration_reads_true_and_false() {
    for (v, want) in [("true", true), ("TRUE", true), ("1", true), ("false", false), ("False", false), ("0", false)] {
        assert_eq!(allow_registration(Some(v)).unwrap_or_else(|e| unreachable!("{e}")), want, "{v}");
    }
}

#[test]
fn allow_registration_rejects_other_values() {
    let err = allow_registration(Some("maybe")).err().unwrap_or_else(|| unreachable!("accepted"));
    assert!(err.to_string().contains("ZITADEL_ALLOW_REGISTRATION"), "{err}");
}

#[test]
fn login_policy_request_adds_a_custom_policy_over_the_default() {
    let current = json!({ "policy": { "isDefault": true, "allowUsernamePassword": true, "allowRegister": true,
        "forceMfa": true, "mfaInitSkipLifetime": "2592000s", "passwordlessType": "PASSWORDLESS_TYPE_ALLOWED" } });
    let (method, path, body) = login_policy_request(&current, false);
    assert_eq!(method, reqwest::Method::POST);
    assert_eq!(path, "/management/v1/policies/login");
    assert_eq!(body["allowRegister"], json!(false));
    assert_eq!(body["forceMfa"], json!(false));
    assert_eq!(body["mfaInitSkipLifetime"], json!("0s"));
    assert_eq!(body["allowUsernamePassword"], json!(true));
    assert_eq!(body["passwordlessType"], json!("PASSWORDLESS_TYPE_ALLOWED"));
    assert!(body.get("isDefault").is_none());
}

#[test]
fn login_policy_request_updates_a_custom_policy() {
    let current = json!({ "policy": { "isDefault": false, "allowRegister": false, "forceMfa": false,
        "mfaInitSkipLifetime": "0s" } });
    let (method, path, body) = login_policy_request(&current, true);
    assert_eq!(method, reqwest::Method::PUT);
    assert_eq!(path, "/management/v1/policies/login");
    assert_eq!(body["allowRegister"], json!(true));
}
```

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p xtask zitadel_bootstrap` from `server/`. Expected: compile errors, because `allow_registration` and `login_policy_request` are not defined.

- [ ] **Step 3: Implement.**
  - **`allow_registration`:** empty or `None` means `true`. `true`/`1` and `false`/`0` are accepted, case-insensitive. Anything else is an error: `ZITADEL_ALLOW_REGISTRATION must be true or false, got "<v>"`.
  - **`login_policy_request`:** copy `current["policy"]` and drop the read-only fields `isDefault`, `details` and `id`. Set `allowRegister`, set `forceMfa: false`, and set `mfaInitSkipLifetime: "0s"` (Zitadel then never prompts to set up MFA). Use `POST` when `isDefault` is true, `PUT` otherwise, both to `/management/v1/policies/login`.
  - **`ensure_login_policy`:** `GET {ISSUER}/management/v1/policies/login`, then send the request above through `check`.
  - **In `run()`:**
    - Call `dotenvy::from_path("../docker/.env").ok()` first. A missing file is fine, and process variables win because `dotenvy` never overrides them.
    - Read `std::env::var("ZITADEL_ALLOW_REGISTRATION").ok()`, then call `ensure_login_policy` after `ensure_builtin_login`.
    - Print `Registration: allowed` or `Registration: disabled` with the other summary lines.

- [ ] **Step 4: Run the tests and the gates.** `cargo test -p xtask` and the four Rust gates from `server/`. Expected: all pass, with no clippy warnings.

- [ ] **Step 5: Run it against the dev stack.**
  - With the stack up, run `cargo xtask zitadel-bootstrap` twice. Expected: both succeed; the second run takes the `PUT` path.
  - Ask the user to open `https://postit.local:44300/ui/console` (Organization > Login Behavior) and confirm "Register allowed" is on and no MFA is forced.
  - Then set `ZITADEL_ALLOW_REGISTRATION=false`, run again, and confirm the login page has no "Register" link.
  - Restore the variable to unset and run once more.

- [ ] **Step 6: Document.**
  - In `docker/.env.example`:
    ```
    # development: allow self-registration on the Zitadel login page (default true).
    # ZITADEL_ALLOW_REGISTRATION=false
    ```
  - In `docs/deployment.md`, add a step to "Zitadel production setup": in the organization's login policy, set "User registration allowed" to match whether new users may sign up on their own (they still start `pending`), and decide whether to force MFA. The dev bootstrap does both from `ZITADEL_ALLOW_REGISTRATION`.

- [ ] **Step 7: Commit**

```bash
git add server/xtask docker/.env.example docs/deployment.md server/Cargo.lock
git commit -m "feat(xtask): dev login policy without MFA prompts, optional registration"
```

---

### Task 4: Startup screen and launcher icon

**Files:**
- Modify: `app/pubspec.yaml`. Add the `flutter_native_splash` dev dependency (latest) and a `flutter_native_splash:` block, and change the `flutter_launcher_icons:` block.
- Create: `app/assets/logo/postit-splash-android12.png`.
- Replace: `app/assets/logo/postit-adaptive-fg.png`.
- Regenerated: `app/android/app/src/main/res/**`, `app/ios/Runner/Assets.xcassets/**`, `app/ios/Runner/Base.lproj/LaunchScreen.storyboard`, `app/ios/Runner/Info.plist` (whatever the two generators write).

**Interfaces:**
- Produces: no Dart API. The startup screen and icons come from generated platform resources.

- [ ] **Step 1: Make the two images.** Use a short PowerShell `System.Drawing` script that you do not commit.
  - **`postit-adaptive-fg.png`, 1024×1024, transparent:** start from `postit-sb.png` scaled to 1024. Set to fully transparent every pixel whose RGB is within 48 of `#231F20` (Euclidean), so only the white "p" and the pink strokes remain. Their dark anti-aliased edges sit on the black adaptive background and do not show.
  - **`postit-splash-android12.png`, 1152×1152, transparent:** draw `postit-sb.png` centered at 504×504. Android 12 shows the splash icon inside a circle 2/3 of its size, and 504 px fits a square inside that circle with margin.
  - Look at both images with Read before continuing.

- [ ] **Step 2: Configure.** In `app/pubspec.yaml`:

```yaml
flutter_launcher_icons:
  android: true
  ios: true
  image_path: assets/logo/postit-sb.png
  remove_alpha_ios: true
  adaptive_icon_background: "#231F20"
  adaptive_icon_foreground: assets/logo/postit-adaptive-fg.png

flutter_native_splash:
  color: "#E0679F"
  image: assets/logo/postit-sb.png
  android_12:
    color: "#E0679F"
    icon_background_color: "#E0679F"
    image: assets/logo/postit-splash-android12.png
  ios: true
  web: false
```

The splash colors are the same in dark mode, so no `color_dark` is set. If the installed `flutter_native_splash` falls back to white for dark mode when `color_dark` is missing, set `color_dark` and `android_12.color_dark` to `#E0679F` too.

- [ ] **Step 3: Generate.** From `app/`: `dart run flutter_launcher_icons`, then `dart run flutter_native_splash:create`.
  - Then `git diff --stat`. Revert the `project.pbxproj` change `ASSETCATALOG_COMPILER_GENERATE_SWIFT_ASSET_SYMBOL_EXTENSIONS = AppIcon`, as in the first P11 run.
  - Keep the rest.

- [ ] **Step 4: Run the gates.** `flutter analyze` and `flutter test`. Expected: clean, 103 tests passing.

- [ ] **Step 5: Look at it on the emulator (Review Focus 2).**
  - Build and install the dev flavor, then cold start it (`adb shell am force-stop`, then `am start`).
  - Capture `adb exec-out screencap` during the splash, in light mode and with `adb shell cmd uimode night yes`.
  - Capture the launcher icon from the app drawer.
  - Expected: pink full-screen startup with the black badge and no circle, in both modes; the launcher icon shows the white "p" and pink strokes on black, in the launcher's own shape.
  - Show the user the screenshots and ask them to confirm. Afterwards, turn night mode off with `cmd uimode night no`.

- [ ] **Step 6: Commit**

```bash
git add app/pubspec.yaml app/pubspec.lock app/assets/logo app/android/app/src/main/res app/ios/Runner
git commit -m "feat(app): pink startup screen with the logo, launcher icon on the logo's black"
```

---

### Task 5: "Sign in again" on the startup error screen

**Files:**
- Modify: `app/lib/ui/screens/status_screens.dart`, the `ErrorScreen` class.
- Test: `app/test/ui/screens/status_screens_test.dart`.

**Interfaces:**
- Consumes: `sessionStoreProvider` (`Provider<SessionStore>`) and `sessionManagerProvider` from `app/lib/state/providers.dart`, plus `MemoryStore` and `StoredSession` from the test helpers.

- [ ] **Step 1: Write the failing tests** in `status_screens_test.dart`:

```dart
testWidgets('sign in again clears the stored session', (tester) async {
  final store = MemoryStore(const StoredSession(refreshToken: 'kept', idToken: 'id-kept'));
  final client = FakeAuthClient()..onRefresh = (_) async => throw const AuthUnavailable();
  await pumpApp(tester, overrides: [
    sessionStoreProvider.overrideWithValue(store),
    sessionManagerProvider.overrideWith((ref) async {
      final m = SessionManager(client: client, store: ref.watch(sessionStoreProvider), revoke: (_) async {});
      await m.restore();
      return m;
    }),
  ]);
  expect(find.textContaining("Can't reach postit"), findsOneWidget);
  await tester.tap(find.text('Sign in again'));
  await tester.pumpAndSettle();
  expect(store.value, isNull);
  expect(find.text('Sign in'), findsOneWidget);
});

testWidgets('a startup error that is not the identity provider offers only retry', (tester) async {
  await pumpApp(tester, overrides: [
    sessionManagerProvider.overrideWith((ref) async => throw const ApiError(0, 'network')),
  ]);
  expect(find.text('Retry'), findsOneWidget);
  expect(find.text('Sign in again'), findsNothing);
});
```

Add the imports this needs: `session_manager.dart`, `session_store.dart`, `not_signed_in.dart`, and `../../helpers/fakes.dart`.

- [ ] **Step 2: Run them to see them fail.** Run `flutter test test/ui/screens/status_screens_test.dart`. Expected: the first test fails because no 'Sign in again' widget is found.

- [ ] **Step 3: Implement.**
  - When `startup.error is AuthUnavailable`, `ErrorScreen` adds a `TextButton` labeled `Sign in again` under Retry.
  - Its handler awaits `ref.read(sessionStoreProvider).clear()`, ignoring a thrown error the way `SessionManager` does, then calls `ref.invalidate(sessionManagerProvider)`.

- [ ] **Step 4: Run the tests and gates.** `flutter test` and `flutter analyze`. Expected: 105 tests passing, no issues.

- [ ] **Step 5: Commit**

```bash
git add app/lib/ui/screens/status_screens.dart app/test/ui/screens/status_screens_test.dart
git commit -m "feat(app): sign in again when the identity provider keeps failing at startup"
```

---

### Task 6: CI: make the dev-trust check able to fail

**Files:**
- Modify: `.github/workflows/ci.yml`, the `app` job: add a step before `build qa and production`.

- [ ] **Step 1: Add the step.**

```yaml
      - name: stand-in dev CA
        # The real CA is git-ignored, so without a file here the APK check below could not
        # notice the flavor filter on assets/dev/ breaking.
        run: printf 'stand-in, not a certificate\n' > assets/dev/postit-dev-ca.crt
```

- [ ] **Step 2: Prove the check catches a regression, locally, in Git Bash from `app/`.**
  - Temporarily delete the `flavors:` lines under `- path: assets/dev/` in `pubspec.yaml`.
  - Build `flutter build apk --flavor qa --debug --dart-define-from-file=config/qa.json`.
  - Run the `unzip -l ... | grep -Ei 'network_security_config|postit[-_]dev[-_]ca'` line from the CI step. Expected: it prints `assets/flutter_assets/assets/dev/postit-dev-ca.crt`.
  - Restore `pubspec.yaml` (`git checkout app/pubspec.yaml`), rebuild, run the grep again. Expected: no output.
  - Your real CA is in that folder locally, so no stand-in is needed for this check.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: stand-in dev CA so the qa and production APK check can fail"
```

---

### Task 7: The P11 exit flow on the emulator, with the user

**Files:** none, unless a step fails. Then fix the cause with TDD under superpowers:systematic-debugging, in a separate commit.

**Interfaces:**
- Consumes: the AVD from Task 2 (or the old one if the user kept it), Task 3's bootstrap, and Task 4's build.

At each step marked **(user)**, stop and ask the user to do it or to confirm what they see. You may drive the Zitadel login with `adb shell input` using the committed dev seed password. If any screen asks for a code, ask the user for it. If the user is not available, stop there and report which step you reached. Do not time out and move on.

- [ ] **Step 1: Start clean.**
  - With `./stack.ps1 up` running, run `cargo xtask zitadel-bootstrap` and `cargo run` from `server/` (in the background), and `./app/tool/android-dev.ps1 start <avd>`.
  - Install the dev flavor debug APK.
  - If `member@postit.com` is not pending, ask the user whether to reset it. Use `./stack.ps1 reset`, or let the user set it to pending in the web app; reset is their call.

- [ ] **Step 2:** Sign in as `admin@postit.com`. Expected: Home with "Good …, Postit" and a Users tab. Take a screenshot.
- [ ] **Step 3:** Sign out. Expected: Sign in. Tap Sign in again. Expected: Zitadel asks for credentials, not an instant return. Then press back to cancel.
- [ ] **Step 4:** Sign in as `member@postit.com`. Expected: "Waiting for approval".
- [ ] **Step 5:** Sign out, sign in as admin, open Users > Pending, open the member, tap Approve. Expected: the member shows Active.
- [ ] **Step 6:** Sign out, then sign in as member. Expected: Home, and no Users tab.
- [ ] **Step 7:** Force-stop and relaunch the app. Expected: Home, without signing in again.
- [ ] **Step 8:** Turn on airplane mode (`adb shell cmd connectivity airplane-mode enable`), force-stop and relaunch. Expected: "Can't reach postit" with Retry and "Sign in again". Disable airplane mode, tap Retry. Expected: Home, still signed in.
- [ ] **Step 9: Smoke test.**
  - Run `flutter test integration_test/smoke_test.dart --flavor development --dart-define-from-file=config/development.json` from `app/`.
  - When Chrome opens, sign in as admin through `adb shell input`, or ask the user to. Expected: `All tests passed!`.
- [ ] **Step 10: Report** each step's result with its screenshot path to the user. Every step must pass before Task 8.

---

### Task 8: Correct the record

**Files:**
- Modify: `README.md` (the smoke-test line ~421, and the qa note), `docs/deployment.md:106`, `docs/superpowers/specs/2026-10-16-p11-flutter-app-design.md` (spike results ~263-277, Deviations ~299), `!ref/plans/02. foundation.md` (the P11 "Built" line).

- [ ] **Step 1: README.**
  - Smoke-test line: drop "Zitadel may ask for a second factor". Say that the dev bootstrap turns off MFA prompts.
  - Under the flavors bullet, add: "A `qa` build talks to `https://api.qa.postit.com`, so until a qa deployment exists it opens on 'Can't reach postit'. That is expected."
- [ ] **Step 2: `docs/deployment.md:106`.** Replace "(the web and mobile apps share `auth.oidc.client_id`)" with "(the web app's `client_id` and the mobile apps' `android_client_id` and `ios_client_id`)".
- [ ] **Step 3: Spec.**
  - In "Spike results": the second factor came from Zitadel's MFA setup prompt on the seed user. It is now off in dev (Task 3), and spike step 6 can use a real revoked token next time.
  - In "Deviations": replace item 5 with the outcome of Task 7 (all steps passed, on which AVD), and add items for the startup screen and launcher icon (Task 4) and the `ZITADEL_ALLOW_REGISTRATION` switch (Task 3).
- [ ] **Step 4: Plan 02.** Append to the P11 "Built" line: `Exit flow verified on <AVD> on <date> (docs/superpowers/plans/2026-10-17-p11-completion.md).`
- [ ] **Step 5: Run the gates and commit.**
  - Rust gates and `cargo xtask openapi --check` from `server/`; app gates from `app/`.

```bash
git add README.md docs "!ref/plans/02. foundation.md"
git commit -m "docs: P11 exit flow verified; correct the second-factor and qa notes"
```

---

## After the last task

1. Run the whole-branch review against `origin/stage`.
2. Do one fix pass for Critical and Important findings, test first.
3. Then ask the user how to integrate: merge locally, open a PR, or keep the branch.

These are not in this plan:
- the remaining deferred review minors from the first run;
- iOS on a Mac;
- the vault keystores.
