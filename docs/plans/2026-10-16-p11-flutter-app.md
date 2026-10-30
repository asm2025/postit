# P11: Flutter Mobile App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Flutter mobile app in `mobile/` (Android verified, iOS scaffolded). It signs in through the IdP with its own client per platform and shows the sign-in, pending, unavailable, and dashboard shell screens (Home, Profile, and Users for admins). Ship the server, tooling, emulator scripts, CI, and docs it needs.

**Architecture:**
- **Server:** additive config keys and `GET /auth/config` fields give each mobile platform its own OIDC client. `cargo xtask zitadel-bootstrap` creates the two native Zitadel apps, and `cargo xtask openapi` also generates a Dart client.
- **App:** layered folders.
  - `data/` holds config, HTTP, auth, and repositories. `flutter_appauth` is used only behind an `AuthClient` seam.
  - `state/` holds the Riverpod 3 providers without code generation, plus a pure gate function.
  - `ui/` holds the go_router setup, the theme, and the screens.
- **Tooling:** `mobile/tool/android-dev.ps1` and `android-dev.sh` prepare a Google APIs emulator so the app and Chrome reach `https://postit.local` and trust the dev CA.

**Tech Stack:**
- **Server:** Rust 2024 (axum, utoipa, figment).
- **App:** Flutter 3.47.6 / Dart 3.13.5, `flutter_riverpod` 3, `go_router`, `dio`, `flutter_appauth`, `flutter_secure_storage`, `shared_preferences`, `flutter_svg`, `url_launcher`, `intl`, `swagger_parser` + `retrofit` + `json_serializable` + `build_runner`.
- **Android:** Gradle Kotlin DSL on Temurin 21.
- **Tooling:** PowerShell 7 and bash; GitHub Actions.

**Spec:** `docs/specs/2026-10-16-p11-flutter-app-design.md`. Plan 02 (`docs/plans/02. foundation.md`, P11 and "Mobile client") governs where the spec is silent.

## Global Constraints

- **Dependency rule:** never add a package (crate, npm, pub) whose name is an Arabic or Arabic-derived word. OIDC on Flutter means `flutter_appauth`; never the `oidc` pub package.
- **Toolchain:** Flutter 3.47.6 stable (`environment: flutter: 3.47.6`, `sdk: ^3.13.0` in `mobile/pubspec.yaml`); Java 21 (Temurin) for Gradle.
- **Android:** application IDs `io.github.asm2025.postit.dev`, `io.github.asm2025.postit.qa`, `io.github.asm2025.postit`; schemes `postit-dev`, `postit-qa`, `postit`; `minSdk` 29.
- **Redirects:** `<scheme>://auth/callback` for sign-in, `<scheme>://auth/logout` after sign-out.
- **Client IDs:** web, Android, and iOS never share a client ID. Config keys `auth.oidc.client_id` (web), `auth.oidc.android_client_id`, `auth.oidc.ios_client_id`. `allowed_clients` defaults to every configured ID.
- **Tokens:** the refresh token and ID token live in `flutter_secure_storage`; the access token lives in memory only. Never log or print a token.
- **Rust quality gates** after every Rust task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`.
- **App gates** after every app task, from `mobile/`: `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test` (add `integration_test` from Task 16; list only paths that exist, and never `lib/api`, which is generated), `flutter analyze`, and `flutter test`. Run `dart format` on the same paths first; the page width is 120 (`analysis_options.yaml`).
- **Web gates** when web files change (only generated types here), from `web/` in **PowerShell**: `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test; pnpm build`. pnpm only, never npm.
- **Contract:** `cargo xtask openapi --check` stays clean after every task that touches the API.
- **Settings:** every new setting gets a row in `docs/configuration.md` (`backend/crates/config/tests/reference_doc.rs` fails otherwise).
- **Migrations:** no migration in P11 (single-migration policy).
- **Vault:** no step reads, prints, or re-packs vault values. The maintainer creates keystores and re-packs `!ref/vault.7z`.
- **Prose:** docs, comments, and commit messages are in normal American English. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Code style:**
  - Rust: doc comments on public items, `# Errors` sections, `unwrap_or_else(|e| unreachable!(...))` in tests, no `unwrap()`/`expect()` outside tests.
  - Dart: `package:` imports, single quotes, no `print`.
- **Riverpod 3 retry:** every `ProviderScope` (app and tests) passes `retry: (retryCount, error) => null`, so failed providers surface at once and the error screens own retrying.
- **Dev CA trust has two paths.** Android's `network_security_config` covers native code only (AppAuth's token calls). Dart's `HttpClient`, which dio uses, ignores it and trusts the system roots only. So debug development builds also bundle the dev CA as a flavor-only Flutter asset and pass a `SecurityContext` to dio (Task 9, `devTrust`). Neither the resource nor the asset may ship in `qa` or `production`; CI checks both flavors.
- **An unreachable IdP is not a rejected session.** Only an OAuth error from the token endpoint (`invalid_grant` and the like) clears the stored session. Network failures keep the tokens and surface as "Can't reach postit" with Retry (`AuthUnavailable`, Task 8).

## Review Focus

1. **Rotated refresh token.** When the IdP returns a new refresh token, it must be persisted at once; the next cold start must use the new one, not the revoked one. Pinned in Task 8 (`restore persists a rotated refresh token`).
2. **A burst of 401s.** Several requests failing at once must cause exactly one refresh, and every request retries with the new token. Pinned in Task 9 (`concurrent 401s share one refresh`).
3. **The user backs out of the browser.** A cancelled sign-in shows no error and leaves the app on Sign-in. Pinned in Task 8 (`cancelled sign-in stays signed out`) and Task 13 (`cancelled sign-in shows no message`).
4. **Secure storage throws.** Android Keystore can fail after a device restore, so a read failure must mean "signed out", never a crash. Pinned in Task 8 (`unreadable store means signed out`). Task 4 also sets `android:allowBackup="false"`.
5. **The API is unreachable at launch** (offline device). The app shows an error screen with Retry, never an endless spinner. Pinned in Task 13 (`startup failure shows retry`).
6. **The IdP is unreachable during a refresh.** The stored refresh token survives, and the user is not told the session expired. Pinned in Task 8 (`restore that cannot reach the provider keeps the stored session`, `a refresh that cannot reach the provider keeps the session`) and Task 9 (`an unreachable provider fails the call as a network error`).
7. **Sign-out while a refresh is in flight.** The late refresh result must not sign the user back in or re-persist the revoked token. Pinned in Task 8 (`signing out during a refresh does not bring the session back`).
8. **Dio and the dev CA.** A debug development build reaches `https://postit.local` through dio; `qa` and `production` builds carry no dev trust at all. Proven in Task 5 (GET /me) and pinned by the CI check in Task 19.

---

### Task 1: Separate mobile client IDs in config and `GET /auth/config`

**Files:**
- Modify: `backend/crates/config/src/settings.rs` (`OidcSettings` ~150-190, `validate` ~470-503)
- Modify: `backend/crates/config/src/loader.rs` (tests module, next to `empty_audiences_are_rejected` ~934)
- Modify: `backend/crates/api/src/settings.rs`, `backend/crates/api/src/dto.rs:130-139`, `backend/crates/api/src/routes/auth.rs`, `backend/crates/api/src/testkit.rs:85-104`
- Modify: `backend/crates/api/tests/me.rs:6-20`, `backend/crates/identity/tests/verifier.rs` (after `client_falls_back_to_azp` ~312), `backend/crates/server/tests/smoke.rs`
- Modify: `backend/config/qa.toml`, `backend/config/production.toml`, `docs/configuration.md:138` and the `ALLOWED_CLIENTS` row
- Regenerate: `backend/openapi.json`, `web/src/api/schema.d.ts`

**Interfaces:**
- Produces:
  - `OidcSettings::android_client_id: Option<String>` and `OidcSettings::ios_client_id: Option<String>`.
  - `OidcSettings::client_ids(&self) -> impl Iterator<Item = &str>`.
  - `effective_allowed_clients()` now defaults to every configured ID.
  - `ApiSettings::{android_client_id, ios_client_id}: Option<String>`.
  - JSON fields `android_client_id` and `ios_client_id` (`string | null`) on `AuthConfigDto`.

- [ ] **Step 1: Write the failing config tests** in the `loader.rs` tests module:

```rust
    #[test]
    fn mobile_client_ids_must_differ_from_the_web_client() {
        let msg = validation_error(
            Environment::Development,
            "[auth.oidc]
android_client_id = \"postit-app\"
",
        );
        assert!(
            msg.contains("auth.oidc.client_id and auth.oidc.android_client_id must be different clients"),
            "{msg}"
        );
    }

    #[test]
    fn android_and_ios_client_ids_must_differ() {
        let msg = validation_error(
            Environment::Development,
            "[auth.oidc]
android_client_id = \"postit-mobile\"
ios_client_id = \"postit-mobile\"
",
        );
        assert!(
            msg.contains("auth.oidc.android_client_id and auth.oidc.ios_client_id must be different clients"),
            "{msg}"
        );
    }

    #[test]
    fn allowed_clients_default_to_every_configured_client() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[auth.oidc]
android_client_id = \"postit-android\"
ios_client_id = \"\"
",
        );
        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(settings.auth.oidc.ios_client_id, None);
        assert_eq!(
            settings.auth.oidc.effective_allowed_clients(),
            vec!["postit-app".to_string(), "postit-android".to_string()]
        );
    }

    #[test]
    fn explicit_allowed_clients_still_override_the_default() {
        let dir = open_tempdir();
        write(dir.path(), "default.toml", BASELINE);
        write(
            dir.path(),
            "development.toml",
            "[auth.oidc]
android_client_id = \"postit-android\"
allowed_clients = [\"postit-app\"]
",
        );
        let settings = ok_settings(load_with(Environment::Development, dir.path(), []));
        assert_eq!(
            settings.auth.oidc.effective_allowed_clients(),
            vec!["postit-app".to_string()]
        );
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p postit-config client` (from `backend/`)
Expected: compile error in the last two tests, `no field android_client_id on OidcSettings`. (With those two commented out, the first two fail at runtime with unknown field `android_client_id` from `deny_unknown_fields`, not with the expected message.)

- [ ] **Step 3: Implement the settings.** In `OidcSettings`, change the `client_id` doc comment and add the two fields right after it:

```rust
    /// OAuth client ID of the web client.
    pub client_id: String,
    /// OAuth client ID of the Android app, a native application in the identity provider.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub android_client_id: Option<String>,
    /// OAuth client ID of the iOS app, a native application in the identity provider.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub ios_client_id: Option<String>,
    /// Clients whose tokens are accepted. Empty means every configured client ID.
    #[serde(default)]
    pub allowed_clients: Vec<String>,
```

Replace the `impl OidcSettings` block:

```rust
impl OidcSettings {
    /// The configured client IDs: the web one, then Android and iOS when set.
    pub fn client_ids(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.client_id.as_str())
            .chain(self.android_client_id.as_deref())
            .chain(self.ios_client_id.as_deref())
    }

    /// `allowed_clients`, or every configured client ID when it is empty.
    #[must_use]
    pub fn effective_allowed_clients(&self) -> Vec<String> {
        if self.allowed_clients.is_empty() {
            self.client_ids().map(str::to_owned).collect()
        } else {
            self.allowed_clients.clone()
        }
    }
}
```

In `validate`, right after the `audiences` check:

```rust
        let oidc = &self.auth.oidc;
        let clients = [
            ("auth.oidc.client_id", Some(oidc.client_id.as_str())),
            ("auth.oidc.android_client_id", oidc.android_client_id.as_deref()),
            ("auth.oidc.ios_client_id", oidc.ios_client_id.as_deref()),
        ];
        for (i, (key_a, a)) in clients.iter().enumerate() {
            for (key_b, b) in &clients[i + 1..] {
                if a.is_some() && a == b {
                    return Err(ConfigError::Validation(format!(
                        "{key_a} and {key_b} must be different clients"
                    )));
                }
            }
        }
```

Add to the `validate` doc comment's error list: "two of `auth.oidc.client_id`, `android_client_id` and `ios_client_id` are equal".

- [ ] **Step 4: Run the config tests**

Run: `cargo test -p postit-config`
Expected: the new tests PASS. `every_setting_is_in_the_reference` FAILS, naming `POSTIT__AUTH__OIDC__ANDROID_CLIENT_ID` and `POSTIT__AUTH__OIDC__IOS_CLIENT_ID`.

- [ ] **Step 5: Document the keys.** In `docs/configuration.md`, change the `CLIENT_ID` row's description to "OAuth client ID of the web client.". Add these two rows directly below it:

```markdown
| `POSTIT__AUTH__OIDC__ANDROID_CLIENT_ID` | unset | no | OAuth client ID of the Android app, a native application in the identity provider. Must differ from the web and iOS client IDs. Returned by `GET /api/v1/auth/config`. |
| `POSTIT__AUTH__OIDC__IOS_CLIENT_ID` | unset | no | OAuth client ID of the iOS app, a native application in the identity provider. Must differ from the web and Android client IDs. Returned by `GET /api/v1/auth/config`. |
```

Change the `ALLOWED_CLIENTS` row's "Empty means only `client_id`." to "Empty means every configured client ID (`client_id`, `android_client_id`, `ios_client_id`)."

In `backend/config/qa.toml` and `backend/config/production.toml`, under `[auth.oidc]` (add the table header if the file has none), add:

```toml
# Native applications in the identity provider, one per mobile platform (see docs/deployment.md).
# android_client_id = ""
# ios_client_id = ""
```

Run: `cargo test -p postit-config`
Expected: PASS.

- [ ] **Step 6: Write the failing API test.** In `backend/crates/api/tests/me.rs`, add to `auth_config_is_public` after the `client_id` assertion:

```rust
    assert!(res.body["android_client_id"].is_null());
    assert!(res.body["ios_client_id"].is_null());
```

and add a test:

```rust
#[sqlx::test(migrations = "../data/migrations")]
async fn auth_config_lists_the_mobile_clients(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.android_client_id = Some("postit-android".into());
        s.ios_client_id = Some("postit-ios".into());
    })
    .await;
    let res = app
        .call(Method::GET, "/api/v1/auth/config", None, None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["client_id"], "postit-app");
    assert_eq!(res.body["android_client_id"], "postit-android");
    assert_eq!(res.body["ios_client_id"], "postit-ios");
}
```

Run: `cargo test -p postit-api auth_config`
Expected: compile error, `no field android_client_id on ApiSettings`.

- [ ] **Step 7: Implement the API side.**
  - **`ApiSettings`:** after `client_id`, add:

    ```rust
        /// OAuth client ID of the Android app, when configured.
        pub android_client_id: Option<String>,
        /// OAuth client ID of the iOS app, when configured.
        pub ios_client_id: Option<String>,
    ```

    Fill them in `from_settings` with `s.auth.oidc.android_client_id.clone()` and `s.auth.oidc.ios_client_id.clone()`. In `testkit.rs` `default_settings`, set both to `None`. Change the doc comment of `ApiSettings::client_id` to "OAuth client ID of the web client."
  - **`AuthConfigDto`:** change the `client_id` doc to "OAuth client ID of the web client." and add:

    ```rust
        /// OAuth client ID of the Android app, or null when the deployment has none.
        pub android_client_id: Option<String>,
        /// OAuth client ID of the iOS app, or null when the deployment has none.
        pub ios_client_id: Option<String>,
    ```

  - **`routes/auth.rs`:** add `android_client_id: s.android_client_id.clone(),` and `ios_client_id: s.ios_client_id.clone(),` to the `AuthConfigDto` literal.

Run: `cargo test -p postit-api auth_config`
Expected: PASS.

- [ ] **Step 8: Pin multi-client acceptance in the verifier.** Add to `backend/crates/identity/tests/verifier.rs`:

```rust
#[test]
fn every_configured_client_is_accepted() {
    let v = verifier().require_client(
        "client_id",
        vec![
            "postit-app".to_string(),
            "postit-android".to_string(),
            "postit-ios".to_string(),
        ],
    );
    let keys = Keys::generate();
    for client in ["postit-app", "postit-android", "postit-ios"] {
        let token = keys.mint(
            &claims_with(&serde_json::json!({ "azp": client })),
            Algorithm::RS256,
        );
        assert!(v.verify(&token, &keys.jwks()).is_ok(), "{client}");
    }
    let token = keys.mint(
        &claims_with(&serde_json::json!({ "azp": "postit-desktop" })),
        Algorithm::RS256,
    );
    assert!(matches!(
        v.verify(&token, &keys.jwks()),
        Err(VerifyError::WrongClient)
    ));
}
```

Run: `cargo test -p postit-identity every_configured_client`
Expected: PASS. This is a regression pin; the verifier already supports lists.

- [ ] **Step 8b: Pin the whole chain end to end.** The verifier test above does not prove the server's composition root passes `effective_allowed_clients()` (with the mobile IDs) to the verifier, or that a wrong client becomes a 401 over HTTP; today no test covers either. The `postit-server` smoke harness boots the real server from config with a `TestIssuer`, so add the test there.

In `backend/crates/server/tests/smoke.rs`, generalize `get` so it can send a bearer token (keep `get` as a wrapper, so the existing tests are unchanged):

```rust
fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    get_as(addr, path, None)
}

fn get_as(addr: SocketAddr, path: &str, bearer: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| unreachable!("connect: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap_or_default();
    let auth = bearer
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n"
    )
    .unwrap_or_default();
    // …the rest of the former `get` body, unchanged…
}
```

Then add:

```rust
/// An access token from `client`, as the identity provider would mint it for that app.
fn token_from(issuer: &TestIssuer, sub: &str, client: &str) -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 3600)
        .unwrap_or_else(|e| unreachable!("clock: {e}"));
    issuer.mint(
        &serde_json::json!({
            "sub": sub,
            "iss": issuer.issuer_url().to_string().trim_end_matches('/'),
            "aud": "postit",
            "exp": exp,
            "email": format!("{sub}@postit.test"),
            "email_verified": true,
            "name": sub,
            "client_id": client,
        }),
        jsonwebtoken::Algorithm::RS256,
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn configured_mobile_clients_are_accepted_and_unlisted_clients_get_401(pool: PgPool) {
    let issuer = TestIssuer::start().await;
    let dir = config_dir(&issuer, QUIET_LIMITS);
    let mut s = settings(&dir);
    // allowed_clients stays empty, so the default (every configured client) applies.
    s.auth.oidc.android_client_id = Some("postit-android".into());
    let running = start(StartOptions {
        env: Environment::Development,
        role: Role::Api,
        settings: s,
        db: Some(postit_data::Db::from_pool(pool)),
        api_listener: Some(listener()),
        worker_listener: None,
    })
    .await
    .unwrap_or_else(|e| unreachable!("start: {e:#}"));
    let Some(api_addr) = running.api_addr else {
        unreachable!("api role binds the api port");
    };
    assert!(wait_ready(api_addr).await);

    let web = token_from(&issuer, "web-user", "postit-app");
    let android = token_from(&issuer, "android-user", "postit-android");
    let ios = token_from(&issuer, "ios-user", "postit-ios");
    let (web, android, ios) = tokio::task::spawn_blocking(move || {
        (
            get_as(api_addr, "/api/v1/me", Some(&web)),
            get_as(api_addr, "/api/v1/me", Some(&android)),
            get_as(api_addr, "/api/v1/me", Some(&ios)),
        )
    })
    .await
    .unwrap_or_default();
    // A first sign-in provisions a pending user, and /me answers 200 for it.
    assert_eq!(web.0, 200, "web client: {}", web.1);
    assert_eq!(android.0, 200, "configured Android client: {}", android.1);
    // iOS is not configured in this deployment, so its client is not allowed.
    assert_eq!(ios.0, 401, "unconfigured iOS client: {}", ios.1);
    assert!(ios.1.contains("unauthenticated"), "{}", ios.1);
    shuts_down_cleanly(running).await;
}
```

`jsonwebtoken` and `serde_json` are already normal dependencies of `postit-server`, so the test needs no new dev-dependency. If `/me` for a brand-new user answers something other than 200 in this harness (for example because the bootstrap admin rule or the provisioning rate limit applies), assert "not 401" for the accepted clients instead. The point is accepted versus rejected.

Run: `cargo test -p postit-server configured_mobile_clients`
Expected: PASS. As a positive control, temporarily change `compose.rs` to pass `vec![oidc.client_id.clone()]` instead of `oidc.effective_allowed_clients()`: the Android assertion must then fail with 401. Revert the change.

- [ ] **Step 9: Regenerate the contract.** From `backend/`: `cargo xtask openapi`. Then run the web gates in PowerShell from `web/`.
Expected: `backend/openapi.json` and `web/src/api/schema.d.ts` gain the two nullable fields; web gates pass. `cargo xtask openapi --check` is clean.

- [ ] **Step 10: Run the Rust quality gates.** All four must pass.

- [ ] **Step 11: Commit**

```bash
git add backend/crates/config backend/crates/api backend/crates/identity/tests/verifier.rs backend/crates/server/tests/smoke.rs backend/config/qa.toml backend/config/production.toml docs/configuration.md backend/openapi.json web/src/api/schema.d.ts
git commit -m "feat(config,api): separate Android and iOS OIDC client IDs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Native Zitadel apps in `cargo xtask zitadel-bootstrap`

**Files:**
- Modify: `backend/xtask/src/zitadel_bootstrap.rs`

**Interfaces:**
- Consumes: the config keys from Task 1.
- Produces: `backend/config/local.toml` with `client_id`, `android_client_id`, `ios_client_id`, and `audiences = [web, project]`. Native apps `postit-android` and `postit-ios`, with redirect `postit-dev://auth/callback` and post-logout `postit-dev://auth/logout`.

- [ ] **Step 1: Write the failing unit tests** at the bottom of `zitadel_bootstrap.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_toml_lists_every_client() {
        let toml = local_toml("web-1", "android-1", "ios-1", "project-1");
        assert!(toml.contains("client_id = \"web-1\""), "{toml}");
        assert!(toml.contains("android_client_id = \"android-1\""), "{toml}");
        assert!(toml.contains("ios_client_id = \"ios-1\""), "{toml}");
        assert!(toml.contains("audiences = [\"web-1\", \"project-1\"]"), "{toml}");
    }

    #[test]
    fn native_apps_use_custom_schemes_without_dev_mode() {
        let config = native_oidc_config();
        assert_eq!(config["appType"], "OIDC_APP_TYPE_NATIVE");
        assert_eq!(config["authMethodType"], "OIDC_AUTH_METHOD_TYPE_NONE");
        assert_eq!(config["devMode"], false);
        assert_eq!(config["redirectUris"][0], "postit-dev://auth/callback");
        assert_eq!(config["postLogoutRedirectUris"][0], "postit-dev://auth/logout");
        assert_eq!(config["accessTokenType"], "OIDC_TOKEN_TYPE_JWT");
    }

    #[test]
    fn web_app_keeps_only_web_redirects() {
        let config = web_oidc_config();
        let uris = config["redirectUris"].as_array().map(Vec::len);
        assert_eq!(uris, Some(2));
        assert_eq!(config["appType"], "OIDC_APP_TYPE_USER_AGENT");
    }
}
```

Run: `cargo test -p xtask zitadel` (from `backend/`)
Expected: compile error, `cannot find function local_toml` / `native_oidc_config`.

- [ ] **Step 2: Implement.** Replace the constants block for the app with:

```rust
const WEB_APP: &str = "postit-app";
const ANDROID_APP: &str = "postit-android";
const IOS_APP: &str = "postit-ios";
/// The React web app, then Swagger UI's `oauth2-redirect.html` (plan 02 P3).
const WEB_REDIRECT_URIS: [&str; 2] = [
    "https://postit.local:44315/auth/callback",
    "https://postit.local:44310/docs/oauth2-redirect.html",
];
const WEB_POST_LOGOUT_REDIRECT_URIS: [&str; 1] = ["https://postit.local:44315/"];
/// The development flavor's custom scheme (plan 02 P11); qa and production use their own.
const MOBILE_REDIRECT_URIS: [&str; 1] = ["postit-dev://auth/callback"];
const MOBILE_POST_LOGOUT_REDIRECT_URIS: [&str; 1] = ["postit-dev://auth/logout"];
```

Rename `app_oidc_config` to `web_oidc_config` (using the `WEB_*` constants) and add:

```rust
/// A native application: Zitadel accepts custom-scheme redirects for this type without
/// development mode.
fn native_oidc_config() -> Value {
    json!({
        "redirectUris": MOBILE_REDIRECT_URIS,
        "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
        "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
        "appType": "OIDC_APP_TYPE_NATIVE",
        "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE",
        "postLogoutRedirectUris": MOBILE_POST_LOGOUT_REDIRECT_URIS,
        "devMode": false,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
    })
}
```

Generalize `ensure_app` to `async fn ensure_app(client: &reqwest::Client, token: &str, project_id: &str, name: &str, config: Value) -> Result<String>`:
- Use `name` wherever `APP_NAME` appeared.
- Use `config.clone()` for the create body (then set `["name"]`) and `&config` for the `oidc_config` PUT.
- Interpolate `name` into the error contexts, for example `format!("looking up the existing {name} application")`. `check` takes `&str`, so pass `&format!(...)`.

Replace `write_local_toml` with a pure `local_toml` plus a writer:

```rust
fn local_toml(web: &str, android: &str, ios: &str, project_id: &str) -> String {
    format!(
        r#"# Generated by `cargo xtask zitadel-bootstrap`. Safe to delete and regenerate;
# re-running the bootstrap task is idempotent. Git-ignored (see .gitignore).

[auth.oidc]
client_id = "{web}"
android_client_id = "{android}"
ios_client_id = "{ios}"
audiences = ["{web}", "{project_id}"]
"#
    )
}

fn write_local_toml(contents: &str) -> Result<()> {
    let path = Path::new(LOCAL_TOML_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}
```

In `run`:

```rust
    let web = ensure_app(&client, &access_token, &project_id, WEB_APP, web_oidc_config()).await?;
    let android =
        ensure_app(&client, &access_token, &project_id, ANDROID_APP, native_oidc_config()).await?;
    let ios = ensure_app(&client, &access_token, &project_id, IOS_APP, native_oidc_config()).await?;
    ensure_member_user(&client, &access_token).await?;

    write_local_toml(&local_toml(&web, &android, &ios, &project_id))?;

    println!("Done. backend/config/local.toml now has the web, Android and iOS client ids.");
```

Update the module doc comment: "creates the `postit` project, the `postit-app` web application, the `postit-android` and `postit-ios` native applications, and the `member@postit.com` user, then writes the generated client ids and audiences…". Mobile tokens carry the project ID in `aud`, so the audiences need no mobile entry.

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p xtask zitadel`
Expected: PASS.

- [ ] **Step 4: Run against the dev stack.** From the repo root in PowerShell: `./stack.ps1 up`. From `backend/`: `cargo xtask zitadel-bootstrap`.
Expected:
- "Done" is printed, and a second run also succeeds (idempotent).
- `backend/config/local.toml` holds three different client IDs.
- In the Zitadel console (`https://postit.local:44300/ui/console`, project `postit`), `postit-android` and `postit-ios` are Native apps with the custom-scheme redirects.

  If creation fails with an error naming dev mode or the redirect, set `"devMode": true` in `native_oidc_config`, update the test to `true`, and note it for Task 5's findings.
- `cargo run` from `backend/` starts, and `GET https://postit.local:44310/api/v1/auth/config` shows both mobile IDs.

- [ ] **Step 5: Run the Rust quality gates, then commit**

```bash
git add backend/xtask/src/zitadel_bootstrap.rs
git commit -m "feat(xtask): create native Android and iOS Zitadel apps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Emulator scripts `mobile/tool/android-dev.ps1` and `android-dev.sh`

**Files:**
- Create: `mobile/tool/android-dev.ps1`, `mobile/tool/android-dev.sh`

**Interfaces:**
- Consumes: `docker/shared/nginx/certs/postit-dev-ca.crt` (from `cert.ps1`), `ANDROID_HOME`, `ANDROID_USER_HOME`, `ANDROID_AVD_HOME`, `POSTIT_AVD`.
- Produces: the subcommands `list`, `prepare <avd>`, `start <avd>`, `clear <avd>`, `reset <avd>`, `help`. Task 21 and the README use them.

- [ ] **Step 1: Write `mobile/tool/android-dev.ps1`**

```powershell
#Requires -Version 7
<#
.SYNOPSIS
    Prepares an Android emulator for postit development.
.DESCRIPTION
    prepare makes postit.local resolve to the host (10.0.2.2) and installs the dev CA in the
    emulator's user CA store, so the app and Chrome (which runs the Zitadel login) trust
    https://postit.local. It needs a Google APIs image: Google Play images cannot be rooted.
    The /system change is visible only when the AVD boots with -writable-system, which start does.
.EXAMPLE
    ./android-dev.ps1 list
    ./android-dev.ps1 prepare Pixel_10a_API
    ./android-dev.ps1 start Pixel_10a_API
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('list', 'prepare', 'start', 'clear', 'reset', 'help')]
    [string] $Command = 'help',
    [Parameter(Position = 1)]
    [string] $Avd = $env:POSTIT_AVD,
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$CaPath = Join-Path $RepoRoot 'docker/shared/nginx/certs/postit-dev-ca.crt'
$HostsLine = '10.0.2.2 postit.local'
$Exe = if ($IsWindows) { '.exe' } else { '' }

function Stop-WithError([string] $Message) {
    Write-Host "error: $Message" -ForegroundColor Red
    exit 1
}

function Get-Tool([string] $RelativePath) {
    if (-not $env:ANDROID_HOME) {
        Stop-WithError 'ANDROID_HOME is not set. Point it at the Android SDK (for example D:\Android\sdk).'
    }
    $path = Join-Path $env:ANDROID_HOME "$RelativePath$Exe"
    if (-not (Test-Path $path)) { Stop-WithError "$path is missing. Install it with the Android SDK Manager." }
    $path
}

function Get-AvdHome {
    if ($env:ANDROID_AVD_HOME) { return $env:ANDROID_AVD_HOME }
    if ($env:ANDROID_USER_HOME) { return Join-Path $env:ANDROID_USER_HOME 'avd' }
    Join-Path $HOME '.android/avd'
}

function Get-AvdTag([string] $Name) {
    $ini = Join-Path (Get-AvdHome) "$Name.avd/config.ini"
    if (-not (Test-Path $ini)) { Stop-WithError "No AVD named '$Name' in $(Get-AvdHome). Run: ./android-dev.ps1 list" }
    $line = Get-Content $ini | Where-Object { $_ -like 'tag.id=*' } | Select-Object -First 1
    if ($line) { $line.Substring('tag.id='.Length) } else { 'unknown' }
}

function Assert-AvdName {
    if (-not $Avd) { Stop-WithError 'Name the AVD (positional argument) or set POSTIT_AVD. Run: ./android-dev.ps1 list' }
}

function Invoke-Adb([string] $Serial, [string[]] $Arguments) {
    $output = & $script:Adb -s $Serial @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { Stop-WithError "adb $($Arguments -join ' ') failed: $output" }
    $output
}

function Find-Serial([string] $Name) {
    $lines = & $script:Adb devices | Select-Object -Skip 1
    foreach ($line in $lines) {
        if ($line -match '^(emulator-\d+)\s+device') {
            $serial = $Matches[1]
            $running = (& $script:Adb -s $serial emu avd name 2>$null | Select-Object -First 1)
            if ($running -and $running.Trim() -eq $Name) { return $serial }
        }
    }
    $null
}

function Wait-Boot([string] $Serial) {
    & $script:Adb -s $Serial wait-for-device | Out-Null
    for ($i = 0; $i -lt 180; $i++) {
        $done = (& $script:Adb -s $Serial shell getprop sys.boot_completed 2>$null) -join ''
        if ($done.Trim() -eq '1') { return }
        Start-Sleep -Seconds 1
    }
    Stop-WithError "$Serial did not finish booting in 3 minutes."
}

function Start-Avd([switch] $Wipe) {
    $existing = Find-Serial $Avd
    if ($existing) {
        if (-not $Wipe) { Write-Host "$Avd is already running as $existing."; return $existing }
        & $script:Adb -s $existing emu kill | Out-Null
        while (Find-Serial $Avd) { Start-Sleep -Seconds 1 }
    }
    $arguments = @('-avd', $Avd, '-writable-system')
    if ($Wipe) { $arguments += '-wipe-data' }
    Start-Process -FilePath $script:Emulator -ArgumentList $arguments | Out-Null
    Write-Host "Starting $Avd ..."
    for ($i = 0; $i -lt 120; $i++) {
        $serial = Find-Serial $Avd
        if ($serial) { Wait-Boot $serial; return $serial }
        Start-Sleep -Seconds 1
    }
    Stop-WithError "$Avd did not show up in adb devices within 2 minutes."
}

function Restart-AsRoot([string] $Serial) {
    Invoke-Adb $Serial @('reboot') | Out-Null
    Wait-Boot $Serial
    Invoke-Adb $Serial @('root') | Out-Null
    Wait-Boot $Serial
}

# subject_hash_old: the MD5 of the DER-encoded subject, first four bytes read little-endian.
function Get-SubjectHashOld([string] $Path) {
    $loader = 'System.Security.Cryptography.X509Certificates.X509CertificateLoader' -as [type]
    $cert = if ($loader) { $loader::LoadCertificateFromFile($Path) }
            else { [System.Security.Cryptography.X509Certificates.X509Certificate2]::new($Path) }
    $md5 = [System.Security.Cryptography.MD5]::HashData($cert.SubjectName.RawData)
    '{0:x8}' -f [BitConverter]::ToUInt32($md5, 0)
}

function Invoke-Prepare {
    Assert-AvdName
    $tag = Get-AvdTag $Avd
    if ($tag -like '*playstore*') {
        Stop-WithError "$Avd uses a Google Play image ($tag), which cannot be rooted. Create an AVD with a Google APIs image."
    }
    if (-not (Test-Path $CaPath)) { Stop-WithError "$CaPath is missing. Run ./cert.ps1 from the repo root first." }

    $serial = Start-Avd
    Invoke-Adb $serial @('root') | Out-Null
    Wait-Boot $serial
    Invoke-Adb $serial @('shell', 'avbctl', 'disable-verification') | Out-Null
    Restart-AsRoot $serial
    $remount = (Invoke-Adb $serial @('remount')) -join "`n"
    if ($remount -match 'reboot') {
        Restart-AsRoot $serial
        Invoke-Adb $serial @('remount') | Out-Null
    }

    $hosts = (Invoke-Adb $serial @('shell', 'cat', '/system/etc/hosts')) -join "`n"
    if ($hosts -notmatch [regex]::Escape($HostsLine)) {
        Invoke-Adb $serial @('shell', "echo '$HostsLine' >> /system/etc/hosts") | Out-Null
        Write-Host "Added '$HostsLine' to /system/etc/hosts."
    } else {
        Write-Host 'postit.local is already in /system/etc/hosts.'
    }

    $hash = Get-SubjectHashOld $CaPath
    $target = "/data/misc/user/0/cacerts-added/$hash.0"
    Invoke-Adb $serial @('push', $CaPath, '/data/local/tmp/postit-dev-ca.crt') | Out-Null
    Invoke-Adb $serial @('shell', "mkdir -p /data/misc/user/0/cacerts-added && cp /data/local/tmp/postit-dev-ca.crt $target && chown system:system $target && chmod 644 $target && restorecon $target && rm /data/local/tmp/postit-dev-ca.crt") | Out-Null
    Write-Host "Installed the dev CA as $target."
    Write-Host "$Avd is ready. Keep starting it with: ./android-dev.ps1 start $Avd"
}

function Invoke-Clear {
    Assert-AvdName
    if (-not $Force) {
        $answer = Read-Host "Wipe all data on $Avd? Type yes to continue"
        if ($answer -ne 'yes') { Write-Host 'Cancelled.'; exit 0 }
    }
    Start-Avd -Wipe | Out-Null
    Write-Host "$Avd was wiped and is running."
}

function Show-List {
    $avdHome = Get-AvdHome
    $items = Get-ChildItem -Path $avdHome -Filter '*.ini' -File -ErrorAction SilentlyContinue
    if (-not $items) { Write-Host "No AVDs in $avdHome."; return }
    foreach ($item in $items) {
        $name = $item.BaseName
        $tag = Get-AvdTag $name
        $note = if ($tag -like '*playstore*') { 'cannot be prepared (Google Play image)' } else { 'can be prepared' }
        '{0,-28} {1,-32} {2}' -f $name, $tag, $note
    }
}

function Show-Help {
    @'
Usage: ./android-dev.ps1 <command> [avd] [-Force]

  list            List AVDs and whether they can be prepared.
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry).
  clear <avd>     Wipe the AVD's data. Asks first unless -Force is given.
  reset <avd>     clear, then prepare.
  help            Show this text.

The AVD defaults to $env:POSTIT_AVD. AVDs are read from ANDROID_AVD_HOME, else
ANDROID_USER_HOME/avd, else ~/.android/avd. Tools come from ANDROID_HOME.
'@
}

if ($Command -eq 'help') { Show-Help; exit 0 }
if ($Command -eq 'list') { Show-List; exit 0 }
$script:Adb = Get-Tool 'platform-tools/adb'
$script:Emulator = Get-Tool 'emulator/emulator'
switch ($Command) {
    'prepare' { Invoke-Prepare }
    'start' { Assert-AvdName; $serial = Start-Avd; Write-Host "$Avd is running as $serial." }
    'clear' { Invoke-Clear }
    'reset' { Invoke-Clear; Invoke-Prepare }
}
```

- [ ] **Step 2: Write `mobile/tool/android-dev.sh`** with the same commands and behavior:

```bash
#!/usr/bin/env bash
# Prepares an Android emulator for postit development. See android-dev.ps1 for details;
# both scripts offer the same commands.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
ca_path="$repo_root/docker/shared/nginx/certs/postit-dev-ca.crt"
hosts_line='10.0.2.2 postit.local'
command="${1:-help}"
avd="${2:-${POSTIT_AVD:-}}"
force=false
for arg in "$@"; do [[ "$arg" == "--force" ]] && force=true; done

die() { echo "error: $*" >&2; exit 1; }

avd_home() {
  if [[ -n "${ANDROID_AVD_HOME:-}" ]]; then echo "$ANDROID_AVD_HOME"
  elif [[ -n "${ANDROID_USER_HOME:-}" ]]; then echo "$ANDROID_USER_HOME/avd"
  else echo "$HOME/.android/avd"; fi
}

tool() {
  [[ -n "${ANDROID_HOME:-}" ]] || die "ANDROID_HOME is not set. Point it at the Android SDK."
  local path="$ANDROID_HOME/$1"
  [[ -x "$path" ]] || die "$path is missing. Install it with the Android SDK Manager."
  echo "$path"
}

avd_tag() {
  local ini; ini="$(avd_home)/$1.avd/config.ini"
  [[ -f "$ini" ]] || die "No AVD named '$1' in $(avd_home). Run: ./android-dev.sh list"
  grep -m1 '^tag.id=' "$ini" | cut -d= -f2 || echo unknown
}

require_avd() { [[ -n "$avd" ]] || die "Name the AVD or set POSTIT_AVD. Run: ./android-dev.sh list"; }

find_serial() {
  local serial
  for serial in $("$adb" devices | awk 'NR>1 && $2=="device" && $1 ~ /^emulator-/ {print $1}'); do
    if [[ "$("$adb" -s "$serial" emu avd name 2>/dev/null | head -n1 | tr -d '\r')" == "$1" ]]; then
      echo "$serial"; return 0
    fi
  done
  return 0
}

wait_boot() {
  "$adb" -s "$1" wait-for-device
  for _ in $(seq 1 180); do
    [[ "$("$adb" -s "$1" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == "1" ]] && return 0
    sleep 1
  done
  die "$1 did not finish booting in 3 minutes."
}

start_avd() { # $1 = wipe flag (true|false); prints the serial
  local serial; serial="$(find_serial "$avd")"
  if [[ -n "$serial" ]]; then
    if [[ "$1" != true ]]; then echo "$serial"; return 0; fi
    "$adb" -s "$serial" emu kill >/dev/null
    while [[ -n "$(find_serial "$avd")" ]]; do sleep 1; done
  fi
  local args=(-avd "$avd" -writable-system)
  [[ "$1" == true ]] && args+=(-wipe-data)
  nohup "$emulator" "${args[@]}" >/dev/null 2>&1 &
  for _ in $(seq 1 120); do
    serial="$(find_serial "$avd")"
    if [[ -n "$serial" ]]; then wait_boot "$serial"; echo "$serial"; return 0; fi
    sleep 1
  done
  die "$avd did not show up in adb devices within 2 minutes."
}

restart_as_root() {
  "$adb" -s "$1" reboot; wait_boot "$1"
  "$adb" -s "$1" root >/dev/null; wait_boot "$1"
}

cmd_prepare() {
  require_avd
  local tag; tag="$(avd_tag "$avd")"
  [[ "$tag" == *playstore* ]] && die "$avd uses a Google Play image ($tag), which cannot be rooted. Create an AVD with a Google APIs image."
  [[ -f "$ca_path" ]] || die "$ca_path is missing. Run ./cert.sh from the repo root first."
  command -v openssl >/dev/null || die "openssl is required to compute the CA file name."

  local serial; serial="$(start_avd false)"
  "$adb" -s "$serial" root >/dev/null; wait_boot "$serial"
  "$adb" -s "$serial" shell avbctl disable-verification >/dev/null
  restart_as_root "$serial"
  if "$adb" -s "$serial" remount 2>&1 | grep -qi reboot; then
    restart_as_root "$serial"
    "$adb" -s "$serial" remount >/dev/null
  fi

  if "$adb" -s "$serial" shell cat /system/etc/hosts | grep -qF "$hosts_line"; then
    echo "postit.local is already in /system/etc/hosts."
  else
    "$adb" -s "$serial" shell "echo '$hosts_line' >> /system/etc/hosts"
    echo "Added '$hosts_line' to /system/etc/hosts."
  fi

  local hash target
  hash="$(openssl x509 -subject_hash_old -noout -in "$ca_path")"
  target="/data/misc/user/0/cacerts-added/$hash.0"
  "$adb" -s "$serial" push "$ca_path" /data/local/tmp/postit-dev-ca.crt >/dev/null
  "$adb" -s "$serial" shell "mkdir -p /data/misc/user/0/cacerts-added && cp /data/local/tmp/postit-dev-ca.crt $target && chown system:system $target && chmod 644 $target && restorecon $target && rm /data/local/tmp/postit-dev-ca.crt"
  echo "Installed the dev CA as $target."
  echo "$avd is ready. Keep starting it with: ./android-dev.sh start $avd"
}

cmd_clear() {
  require_avd
  if [[ "$force" != true ]]; then
    read -r -p "Wipe all data on $avd? Type yes to continue: " answer
    [[ "$answer" == "yes" ]] || { echo "Cancelled."; exit 0; }
  fi
  start_avd true >/dev/null
  echo "$avd was wiped and is running."
}

cmd_list() {
  local ini name tag note found=false
  for ini in "$(avd_home)"/*.ini; do
    [[ -f "$ini" ]] || continue
    found=true
    name="$(basename "$ini" .ini)"; tag="$(avd_tag "$name")"
    if [[ "$tag" == *playstore* ]]; then note="cannot be prepared (Google Play image)"; else note="can be prepared"; fi
    printf '%-28s %-32s %s\n' "$name" "$tag" "$note"
  done
  [[ "$found" == true ]] || echo "No AVDs in $(avd_home)."
}

usage() {
  cat <<'EOF'
Usage: ./android-dev.sh <command> [avd] [--force]

  list            List AVDs and whether they can be prepared.
  prepare <avd>   One-time setup: postit.local in the hosts file and the dev CA trusted.
  start <avd>     Boot the AVD with -writable-system (needed for the hosts entry).
  clear <avd>     Wipe the AVD's data. Asks first unless --force is given.
  reset <avd>     clear, then prepare.
  help            Show this text.

The AVD defaults to $POSTIT_AVD. AVDs are read from ANDROID_AVD_HOME, else
ANDROID_USER_HOME/avd, else ~/.android/avd. Tools come from ANDROID_HOME.
EOF
}

case "$command" in
  help) usage; exit 0 ;;
  list) cmd_list; exit 0 ;;
esac
adb="$(tool platform-tools/adb)"
emulator="$(tool emulator/emulator)"
case "$command" in
  prepare) cmd_prepare ;;
  start) require_avd; echo "$avd is running as $(start_avd false)." ;;
  clear) cmd_clear ;;
  reset) cmd_clear; cmd_prepare ;;
  *) usage; exit 1 ;;
esac
```

Mark it executable: `git update-index --chmod=+x mobile/tool/android-dev.sh` (after `git add`).

- [ ] **Step 3: Verify the CA hash matches OpenSSL.** In PowerShell from the repo root:

```powershell
$cert = [System.Security.Cryptography.X509Certificates.X509CertificateLoader]::LoadCertificateFromFile("$PWD/docker/shared/nginx/certs/postit-dev-ca.crt")
'{0:x8}' -f [BitConverter]::ToUInt32([System.Security.Cryptography.MD5]::HashData($cert.SubjectName.RawData), 0)
& 'C:\Program Files\Git\usr\bin\openssl.exe' x509 -subject_hash_old -noout -in docker/shared/nginx/certs/postit-dev-ca.crt
```

(The first two lines are the body of `Get-SubjectHashOld`.)

Expected: both print the same eight hex digits.

- [ ] **Step 4: Run against a real AVD.** Use the Pixel 10a Google APIs AVD (`./app/tool/android-dev.ps1 list` shows its name).
  - `./app/tool/android-dev.ps1 list`: the Play AVD shows "cannot be prepared" and the Google APIs AVD shows "can be prepared".
  - `./app/tool/android-dev.ps1 prepare <google-apis-avd>`: finishes with "ready". A second `prepare` run reports the hosts entry already present.
  - `./app/tool/android-dev.ps1 prepare <play-avd>`: fails with the Google Play message.
  - With the dev stack up, open Chrome on the emulator at `https://postit.local:44300/.well-known/openid-configuration`. It loads the JSON with no certificate warning. If Chrome warns, record that for Task 5 (spike 2).
  - Run `bash mobile/tool/android-dev.sh list` from Git Bash. It prints the same list.

- [ ] **Step 5: Commit**

```bash
git add mobile/tool/android-dev.ps1 mobile/tool/android-dev.sh
git update-index --chmod=+x mobile/tool/android-dev.sh
git commit -m "feat(app): android-dev scripts to prepare an emulator for postit.local

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Scaffold `mobile/` with flavors and the dev trust configuration

**Files:**
- Create (by `flutter create`, then edited): `mobile/pubspec.yaml`, `mobile/analysis_options.yaml`, `mobile/lib/main.dart`, `mobile/android/**`, `mobile/ios/**`
- Create: `mobile/config/development.json`, `mobile/config/qa.json`, `mobile/config/production.json`
- Create: `mobile/android/app/src/development/AndroidManifest.xml`, `mobile/android/app/src/development/res/xml/network_security_config.xml`, `mobile/assets/dev/.gitkeep`
- Modify: `mobile/android/app/build.gradle.kts`, `mobile/android/app/src/main/AndroidManifest.xml`, `.gitignore`, `.gitattributes`

**Interfaces:**
- Produces:
  - The `postit` Dart package (`package:postit/...`).
  - Flavors `development`, `qa`, and `production`, run with `flutter run --flavor <env> --dart-define-from-file=config/<env>.json`.
  - Dart defines `API_BASE_URL` and `ENV`.
  - Raw resource `@raw/postit_dev_ca` in development builds (for native code: AppAuth's token calls).
  - Flutter asset `assets/dev/postit-dev-ca.crt` in development builds only (for dio, Task 9).

- [ ] **Step 1: Create the project.** From the repo root (PowerShell):

```powershell
flutter create --org io.github.asm2025 --project-name postit --platforms android,ios --empty app
```

Then, from `mobile/`:

```powershell
flutter pub add flutter_riverpod go_router dio flutter_appauth flutter_secure_storage shared_preferences flutter_svg url_launcher intl retrofit json_annotation
flutter pub add --dev build_runner retrofit_generator json_serializable swagger_parser flutter_launcher_icons
```

Check every added name against the dependency rule; all of the above comply.

- [ ] **Step 2: Pin the toolchain in `pubspec.yaml`.** Set `description: The postit mobile app.`, `publish_to: none`, and:

```yaml
environment:
  sdk: ^3.13.0
  flutter: 3.47.6
```

Add the development-only asset directory (flavor-conditional assets, Flutter 3.19+). Task 12 extends this `flutter:` block and must keep the entry:

```yaml
flutter:
  uses-material-design: true
  assets:
    # The dev CA, copied in by Gradle for development builds (Task 4 Step 5). Never bundled
    # into qa or production.
    - path: assets/dev/
      flavors:
        - development
```

- [ ] **Step 3: Write `analysis_options.yaml`**

```yaml
include: package:flutter_lints/flutter.yaml

formatter:
  page_width: 120

analyzer:
  exclude:
    - lib/api/**
  language:
    strict-casts: true
    strict-inference: true
    strict-raw-types: true

linter:
  rules:
    - always_use_package_imports
    - avoid_print
    - prefer_single_quotes
    - unawaited_futures
```

- [ ] **Step 4: Write the dart-define files**

`mobile/config/development.json`:

```json
{ "API_BASE_URL": "https://postit.local:44310", "ENV": "development" }
```

`mobile/config/qa.json`:

```json
{ "API_BASE_URL": "https://api.qa.postit.com", "ENV": "qa" }
```

`mobile/config/production.json`:

```json
{ "API_BASE_URL": "https://api.postit.com", "ENV": "production" }
```

- [ ] **Step 5: Configure Android flavors.** In `mobile/android/app/build.gradle.kts`:
  - In `defaultConfig`, set `applicationId = "io.github.asm2025.postit"` and `minSdk = 29`.
  - Inside `android { }`, add:

```kotlin
    buildFeatures {
        resValues = true
    }

    flavorDimensions += "env"
    productFlavors {
        create("development") {
            dimension = "env"
            applicationIdSuffix = ".dev"
            resValue("string", "app_name", "postit dev")
            manifestPlaceholders["appAuthRedirectScheme"] = "postit-dev"
        }
        create("qa") {
            dimension = "env"
            applicationIdSuffix = ".qa"
            resValue("string", "app_name", "postit qa")
            manifestPlaceholders["appAuthRedirectScheme"] = "postit-qa"
        }
        create("production") {
            dimension = "env"
            resValue("string", "app_name", "postit")
            manifestPlaceholders["appAuthRedirectScheme"] = "postit"
        }
    }
```

At the bottom of the file, add the dev CA copy. It is wired through the variant API (`androidComponents`), because `android.sourceSets…res.srcDir(task)` does not reliably make the build run the task first:

```kotlin
// The development flavor trusts the dev CA in debug builds. The CA file is generated by
// cert.ps1 / cert.sh and git-ignored, so it is copied in at build time, twice:
// - as res/raw/postit_dev_ca.crt for network_security_config (native code: AppAuth);
// - as the Flutter asset assets/dev/postit-dev-ca.crt for dio, because Dart's HttpClient
//   ignores network_security_config.
val devCa = rootProject.file("../../docker/shared/nginx/certs/postit-dev-ca.crt")
val devCaAsset = rootProject.file("../assets/dev/postit-dev-ca.crt")

abstract class CopyDevCa : DefaultTask() {
    // InputFiles, not InputFile: a missing CA reaches the task action and its message.
    @get:InputFiles
    abstract val source: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val resDir: DirectoryProperty

    @get:OutputFile
    abstract val assetFile: RegularFileProperty

    @TaskAction
    fun copy() {
        val ca = source.singleFile
        if (!ca.isFile) {
            throw GradleException(
                "Missing ${ca.path}. Run ./cert.ps1 (or ./cert.sh) from the repo root to create the dev certificate.",
            )
        }
        val raw = resDir.get().dir("raw").asFile.apply { mkdirs() }
        ca.copyTo(File(raw, "postit_dev_ca.crt"), overwrite = true)
        ca.copyTo(assetFile.get().asFile, overwrite = true)
    }
}

androidComponents {
    onVariants(selector().withFlavor("env" to "development")) { variant ->
        val copy = tasks.register<CopyDevCa>("copy${variant.name.replaceFirstChar(Char::uppercase)}DevCa") {
            source.from(devCa)
            assetFile.set(devCaAsset)
        }
        variant.sources.res?.addGeneratedSourceDirectory(copy, CopyDevCa::resDir)
        // Flutter bundles assets in compileFlutterBuild<Variant>; the asset must exist first.
        val flutterTask = "compileFlutterBuild${variant.name.replaceFirstChar(Char::uppercase)}"
        tasks.matching { it.name == flutterTask }.configureEach { dependsOn(copy) }
    }
}
```

If the Flutter Gradle plugin in 3.47 names the bundling task differently, find it with `./gradlew :app:tasks --all` (from `mobile/android`) and match that name instead; Step 10 proves the asset lands in the APK.

- [ ] **Step 6: Edit the main manifest.** In `mobile/android/app/src/main/AndroidManifest.xml`:
  - Add `<uses-permission android:name="android.permission.INTERNET" />` as the first child of `<manifest>`. The `flutter create` template declares it only in the `debug` and `profile` manifests, so without this line release builds of every flavor have no network.
  - On `<application>`, set `android:label="@string/app_name"` and add `android:allowBackup="false"`. Restored Keystore entries cannot decrypt, so backups would only produce broken sessions.

- [ ] **Step 7: Add the development-only trust configuration**

`mobile/android/app/src/development/AndroidManifest.xml`:

```xml
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application android:networkSecurityConfig="@xml/network_security_config" />
</manifest>
```

`mobile/android/app/src/development/res/xml/network_security_config.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<!-- Development flavor only, and only in debuggable builds: trust the postit dev CA. -->
<network-security-config>
    <debug-overrides>
        <trust-anchors>
            <certificates src="@raw/postit_dev_ca" />
            <certificates src="system" />
        </trust-anchors>
    </debug-overrides>
</network-security-config>
```

- [ ] **Step 8: Ignore rules and line endings.**
  - Append to `.gitignore`: `mobile/android/key.properties`, `mobile/android/.gradle/`, `mobile/android/local.properties`, `mobile/ios/Pods/`, `mobile/ios/Flutter/Generated.xcconfig`, `mobile/coverage/`, `mobile/assets/dev/*.crt`.
  - Create an empty `mobile/assets/dev/.gitkeep`, so the asset directory exists in a fresh checkout and `qa` builds (which skip the copy) do not fail on a missing directory.
  - Append to `.gitattributes` (create it if missing): `mobile/lib/api/** text eol=lf`.

- [ ] **Step 9: Placeholder `lib/main.dart`.** Tasks 11–13 replace it.

```dart
import 'package:flutter/material.dart';

void main() => runApp(const MaterialApp(home: Scaffold(body: Center(child: Text('postit')))));
```

Delete `test/widget_test.dart` if `flutter create` made one.

- [ ] **Step 10: Build every flavor**

Run from `mobile/`:

```powershell
flutter analyze
flutter build apk --flavor development --debug --dart-define-from-file=config/development.json
flutter build apk --flavor qa --debug --dart-define-from-file=config/qa.json
flutter build apk --flavor production --debug --dart-define-from-file=config/production.json
```

Expected: all three build. Then check the trust configuration with the newest `aapt2` under `$env:ANDROID_HOME\build-tools`:

```powershell
$aapt2 = Get-ChildItem "$env:ANDROID_HOME\build-tools" | Sort-Object { [version]$_.Name } | Select-Object -Last 1 | ForEach-Object { Join-Path $_.FullName 'aapt2.exe' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($flavor in 'development', 'qa', 'production') {
    $apk = "build/app/outputs/flutter-apk/app-$flavor-debug.apk"
    $manifest = & $aapt2 dump xmltree --file AndroidManifest.xml $apk | Select-String networkSecurityConfig
    $zip = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $apk))
    $files = $zip.Entries.FullName | Select-String 'network_security_config|postit[-_]dev[-_]ca'
    $zip.Dispose()
    "{0,-12} manifest: {1}  files: {2}" -f $flavor, [bool]$manifest, ($files -join ', ')
}
```

Expected: `development` reports the manifest attribute and both `res/raw/postit_dev_ca.crt` (its obfuscated resource path may differ; the XML resource name is enough) and `assets/flutter_assets/assets/dev/postit-dev-ca.crt`. This is the positive control for the CI check. `qa` and `production` report `False` and no files. Also confirm `INTERNET` is in the qa manifest: `& $aapt2 dump permissions build/app/outputs/flutter-apk/app-qa-debug.apk`.

- [ ] **Step 11: Commit**

```bash
git add app .gitignore .gitattributes
git status --short mobile/assets/dev   # must list only .gitkeep, never the .crt
git commit -m "feat(app): scaffold the Flutter app with flavors and dev-only CA trust

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Phase-start spikes: sign-in round trip and Chrome CA trust

Spike code is throwaway: it is never committed. This task commits only the findings in the spec.

**Files:**
- Create (temporary, deleted at the end): `mobile/lib/spike/auth_spike.dart`
- Modify: `docs/specs/2026-10-16-p11-flutter-app-design.md` (new "Spike results" section)

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces: confirmed `flutter_appauth` behavior, which Task 8's `AppAuthClient` relies on: `authorizeAndExchangeCode`, `token` with a refresh token, `endSession`, `FlutterAppAuthUserCancelledException`, and `FlutterAppAuthPlatformException.platformErrorDetails` (including how a network failure differs from `invalid_grant`). Also confirms that dio needs the bundled CA (Task 9's `devTrust`).

- [ ] **Step 1: Write the spike**

```dart
import 'dart:io';

import 'package:dio/dio.dart';
import 'package:dio/io.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_appauth/flutter_appauth.dart';

const _issuer = 'https://postit.local:44300';
const _clientId = String.fromEnvironment('SPIKE_CLIENT_ID');
const _discovery = '$_issuer/.well-known/openid-configuration';
const _redirect = 'postit-dev://auth/callback';
const _logout = 'postit-dev://auth/logout';

void main() => runApp(const MaterialApp(home: SpikePage()));

class SpikePage extends StatefulWidget {
  const SpikePage({super.key});
  @override
  State<SpikePage> createState() => _SpikePageState();
}

class _SpikePageState extends State<SpikePage> {
  final _appAuth = const FlutterAppAuth();
  final _log = <String>[];
  TokenResponse? _tokens;

  void _add(String line) => setState(() => _log.insert(0, line));

  Future<void> _run(String name, Future<void> Function() step) async {
    try {
      await step();
      _add('$name: ok');
    } on FlutterAppAuthUserCancelledException {
      _add('$name: cancelled');
    } on FlutterAppAuthPlatformException catch (e) {
      final d = e.platformErrorDetails;
      _add('$name: type=${d.type} code=${d.code} error=${d.error} description=${d.errorDescription}');
    } on Object catch (e) {
      _add('$name: ${e.runtimeType} $e');
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: const Text('auth spike')),
        body: Column(children: [
          Wrap(spacing: 8, children: [
            FilledButton(
              onPressed: () => _run('sign in', () async {
                _tokens = await _appAuth.authorizeAndExchangeCode(AuthorizationTokenRequest(
                  _clientId, _redirect,
                  discoveryUrl: _discovery,
                  scopes: ['openid', 'profile', 'email', 'offline_access'],
                ));
                _add('refresh token present: ${_tokens?.refreshToken != null}, '
                    'expires: ${_tokens?.accessTokenExpirationDateTime}');
              }),
              child: const Text('Sign in'),
            ),
            FilledButton(
              onPressed: () => _run('refresh', () async {
                final old = _tokens?.refreshToken;
                _tokens = await _appAuth.token(TokenRequest(_clientId, _redirect,
                    discoveryUrl: _discovery, refreshToken: old));
                _add('refresh token rotated: ${_tokens?.refreshToken != old}');
              }),
              child: const Text('Refresh'),
            ),
            FilledButton(
              onPressed: () => _run('GET /me', () async {
                // Dart's HttpClient ignores network_security_config: trust the bundled asset.
                final pem = await rootBundle.load('assets/dev/postit-dev-ca.crt');
                final context = SecurityContext(withTrustedRoots: true)
                  ..setTrustedCertificatesBytes(pem.buffer.asUint8List(pem.offsetInBytes, pem.lengthInBytes));
                final dio = Dio()
                  ..httpClientAdapter = IOHttpClientAdapter(createHttpClient: () => HttpClient(context: context));
                final res = await dio.get<Object>('https://postit.local:44310/api/v1/me',
                    options: Options(headers: {'Authorization': 'Bearer ${_tokens?.accessToken}'}));
                _add('status ${res.statusCode}');
              }),
              child: const Text('GET /me'),
            ),
            FilledButton(
              onPressed: () => _run('end session', () async {
                await _appAuth.endSession(EndSessionRequest(
                    idTokenHint: _tokens?.idToken,
                    postLogoutRedirectUrl: _logout,
                    discoveryUrl: _discovery));
              }),
              child: const Text('End session'),
            ),
          ]),
          Expanded(child: ListView(children: [for (final l in _log) Text(l)])),
        ]),
      );
}
```

- [ ] **Step 2: Run it** with the dev stack up (`./stack.ps1 up`, then `cargo run` from `backend/`) on the AVD prepared in Task 3 (`./app/tool/android-dev.ps1 start <avd>`):

```powershell
flutter run --flavor development -t lib/spike/auth_spike.dart --dart-define-from-file=config/development.json --dart-define=SPIKE_CLIENT_ID=<android_client_id from backend/config/local.toml>
```

Exercise and record each step:
1. Sign in as `admin@postit.com`. Expected: the Zitadel login opens in a Chrome Custom Tab with no certificate warning (spike 2) and returns to the app with "sign in: ok" and a refresh token present.
   A successful step 1 also proves AppAuth's native token exchange trusts the dev CA through `network_security_config`.
2. Run GET /me. Expected: status 200, which proves dio trusts the dev CA through the bundled asset and `SecurityContext`. As a negative control, replace the dio with a plain `Dio()` once: it must fail with `HandshakeException` (`CERTIFICATE_VERIFY_FAILED`), confirming that Dart ignores `network_security_config`.
3. Run Refresh. Expected: ok, and the log records whether the refresh token rotated.
4. Turn on airplane mode, run Refresh, then turn it off. Record the logged `type`, `code`, `error`, and `description`. Expected: `error` is null (no OAuth error code), which is what Task 8's `AppAuthClient` uses to tell "unreachable" from "rejected". Then sign in again, stop the dev stack's Zitadel container (`./stack.ps1 help` lists the commands), run Refresh, and record that error too; start Zitadel again.
5. Run End session. Expected: the browser returns to the app on `postit-dev://auth/logout`, and a new Sign in prompts for credentials again.
6. Sign in again, then end that session's refresh token in the Zitadel console (the user's sessions or refresh tokens), and run Refresh. Expected: an OAuth error with `error=invalid_grant`. This is the "rejected" case.
7. Press back during Sign in. Expected: "sign in: cancelled".

Fallbacks:
- If step 1 fails on the redirect, check the native app in Zitadel. If Zitadel demands dev mode, apply Task 2's fallback (`devMode: true`), re-run `cargo xtask zitadel-bootstrap`, and retry.
- If Chrome warns about the certificate, install the CA by hand (Settings > Security > Encryption & credentials > Install a certificate > CA certificate, from `/sdcard/Download/postit-dev-ca.crt` after `adb push`), and change `prepare` in both scripts to also push the file to `/sdcard/Download/` and print those Settings steps.
- If `flutter_appauth` itself cannot complete PKCE with Zitadel, stop and report to the user before continuing. A library change is their decision.

- [ ] **Step 3: Record the results** in the spec, as a new `## Spike results (2026-10-…)` section after "Phase-start spikes". State the outcome of each step, whether native apps needed `devMode`, whether refresh tokens rotate, whether the Chrome user-store CA worked, and the error details for "unreachable" (step 4) versus "rejected" (step 6). If unreachable errors carry a non-null `error`, adjust Task 8's `_isRejection` rule to whatever separates the two cases, and record the rule here.

- [ ] **Step 4: Delete the spike and commit the findings**

```powershell
Remove-Item -Recurse mobile/lib/spike
git add docs/specs/2026-10-16-p11-flutter-app-design.md
git commit -m "docs(spec): P11 spike results for flutter_appauth with Zitadel

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

If the spike changed `zitadel_bootstrap.rs` or the scripts, include those files in this commit.

---

### Task 6: Generate the Dart client from `cargo xtask openapi`

**Files:**
- Create: `mobile/swagger_parser.yaml`, `mobile/test/api/generated_contract_test.dart`
- Create (generated): `mobile/lib/api/**`
- Modify: `backend/xtask/src/openapi.rs`, `backend/xtask/src/main.rs`

**Interfaces:**
- Produces generated Dart API (`package:postit/api/export.dart`). Repositories (Task 10) rely on these names:

| Kind | Name | Shape used |
| --- | --- | --- |
| client | `AuthApi(Dio)` | `Future<AuthConfigDto> authConfig()` |
| client | `MeApi(Dio)` | `Future<MeDto> getMe()`, `Future<void> deleteMe({required DeleteMeRequest body})` |
| client | `UsersApi(Dio)` | `Future<PageUserDto> listUsers({StatusDto? status, RoleDto? role, String? sort, String? search, int? page, int? pageSize})`, `Future<UserDto> getUser({required String id})`, `Future<UserDto> updateUser({required String id, required PatchUserRequest body})`, `Future<void> deleteUser({required String id})` |
| model | `AuthConfigDto` | `issuer`, `clientId`, `androidClientId?`, `iosClientId?`, `scopes` |
| model | `UserDto` | `id`, `displayName`, `email?`, `emailVerified`, `role`, `status`, `createdAt`, `lastSeenAt?` |
| model | `MeDto` | the `UserDto` fields plus `accountUrl?` |
| model | `PageUserDto` | `data`, `total`, `page`, `pageSize` |
| enums | `StatusDto`, `RoleDto` | `pending`, `active`, `disabled`, `deleting`; `admin`, `member` |
| requests | `PatchUserRequest({StatusDto? status, RoleDto? role})`, `DeleteMeRequest({required String displayName})` | |

- [ ] **Step 1: Write `mobile/swagger_parser.yaml`**

```yaml
# Generates lib/api from the committed contract. Run through `cargo xtask openapi`, never
# by hand, so the Dart client always matches backend/openapi.json.
swagger_parser:
  schema_path: ../backend/openapi.json
  output_directory: lib/api
  name: postit
  language: dart
  json_serializer: json_serializable
  client_postfix: Api
  root_client: false
  put_clients_in_folder: true
  mark_files_as_generated: true
```

Check these option names against the README of the `swagger_parser` version in `pubspec.lock`. If one was renamed, use the new name with the same intent.

- [ ] **Step 2: Write the contract test** (`mobile/test/api/generated_contract_test.dart`). It fails to compile until the generated names match the table.

```dart
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';

void main() {
  test('the generated client has the names the repositories use', () {
    final dio = Dio();
    final auth = AuthApi(dio);
    final me = MeApi(dio);
    final users = UsersApi(dio);
    final Future<AuthConfigDto> Function() authConfig = auth.authConfig;
    final Future<MeDto> Function() getMe = me.getMe;
    final Future<void> Function({required DeleteMeRequest body}) deleteMe = me.deleteMe;
    final Future<UserDto> Function({required String id}) getUser = users.getUser;
    final Future<void> Function({required String id}) deleteUser = users.deleteUser;
    expect([authConfig, getMe, deleteMe, getUser, deleteUser], hasLength(5));
    expect(users.listUsers, isNotNull);
    expect(users.updateUser, isNotNull);
  });

  test('models decode the server JSON', () {
    final user = <String, Object?>{
      'id': '0192f0c4-0000-7000-8000-000000000001',
      'display_name': 'Ada',
      'email': 'ada@postit.test',
      'email_verified': true,
      'role': 'admin',
      'status': 'active',
      'created_at': '2026-10-16T10:00:00Z',
      'updated_at': '2026-10-16T10:00:00Z',
      'approved_at': null,
      'approved_by': null,
      'last_seen_at': null,
    };
    final me = MeDto.fromJson({...user, 'account_url': 'https://idp.test/account'});
    expect(me.displayName, 'Ada');
    expect(me.role, RoleDto.admin);
    expect(me.status, StatusDto.active);
    expect(me.accountUrl, 'https://idp.test/account');

    final page = PageUserDto.fromJson({'data': [user], 'total': 1, 'page': 1, 'page_size': 25});
    expect(page.data.single.emailVerified, isTrue);
    expect(page.pageSize, 25);

    final config = AuthConfigDto.fromJson({
      'issuer': 'https://idp.test',
      'client_id': 'web',
      'android_client_id': 'android',
      'ios_client_id': null,
      'scopes': ['openid'],
    });
    expect(config.androidClientId, 'android');
    expect(config.iosClientId, isNull);

    expect(const PatchUserRequest(status: StatusDto.active).toJson()['status'], 'active');
    expect(const DeleteMeRequest(displayName: 'Ada').toJson(), {'display_name': 'Ada'});
  });
}
```

- [ ] **Step 3: Extend `backend/xtask/src/openapi.rs`.** Add these helpers above `run`:

```rust
fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../mobile")
}

fn dart_api_dir() -> PathBuf {
    app_dir().join("lib/api")
}

/// Runs `dart` in `mobile/`. On Windows `dart` is a `.bat`, which only `cmd` resolves.
fn dart(args: &[&str]) -> anyhow::Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "dart"]);
        c
    } else {
        std::process::Command::new("dart")
    };
    let status = command
        .args(args)
        .current_dir(app_dir())
        .status()
        .context("running dart (is the Flutter SDK on PATH?)")?;
    if !status.success() {
        bail!("`dart {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Regenerates `mobile/lib/api` from the on-disk `backend/openapi.json`.
fn generate_dart() -> anyhow::Result<()> {
    if !app_dir().join(".dart_tool/package_config.json").is_file() {
        bail!("app dependencies are missing; run `flutter pub get` in mobile/ first");
    }
    dart(&["run", "swagger_parser"])?;
    dart(&["run", "build_runner", "build", "--delete-conflicting-outputs"])?;
    Ok(())
}

/// Every file under `dir` by relative path, with LF line endings.
fn snapshot(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<PathBuf, String>> {
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut std::collections::BTreeMap<PathBuf, String>,
    ) -> anyhow::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(rel, text.replace("\r\n", "\n"));
            }
        }
        Ok(())
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Puts `dir` back exactly as `files` describes it.
fn restore(dir: &Path, files: &std::collections::BTreeMap<PathBuf, String>) -> anyhow::Result<()> {
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    for (rel, text) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}
```

In `run`'s check branch, replace `println!("{} and {} are current", ...)` with:

```rust
        // The on-disk spec equals the in-memory one here, so the Dart client is generated
        // from it in place and the original files are put back afterwards.
        let api_dir = dart_api_dir();
        let before = snapshot(&api_dir)?;
        let generated = generate_dart().and_then(|()| snapshot(&api_dir));
        restore(&api_dir, &before)?;
        if generated? != before {
            bail!("mobile/lib/api is stale; run `cargo xtask openapi`");
        }
        println!("{}, {} and {} are current", path.display(), schema.display(), api_dir.display());
        return Ok(());
```

Change the signature to `pub fn run(check: bool, skip_dart: bool) -> anyhow::Result<()>`. At the end of the write branch, before `Ok(())`:

```rust
    if skip_dart {
        // For machines without Flutter. The contract CI job runs --check, which never skips,
        // so a stale Dart client still fails CI.
        eprintln!(
            "warning: skipped {}; run `cargo xtask openapi` with Flutter before committing",
            dart_api_dir().display()
        );
    } else {
        generate_dart()?;
        println!("wrote {}", dart_api_dir().display());
    }
```

In `main.rs`, parse the flags in any order and reject `--skip-dart` with `--check`:

```rust
        Some("openapi") => {
            let flags: Vec<String> = args.collect();
            let check = flags.iter().any(|f| f == "--check");
            let skip_dart = flags.iter().any(|f| f == "--skip-dart");
            if flags.iter().any(|f| f != "--check" && f != "--skip-dart") || (check && skip_dart) {
                anyhow::bail!("usage: xtask openapi [--check | --skip-dart]");
            }
            openapi::run(check, skip_dart)
        }
```

Adapt the names (`args`) to the existing `main.rs`, and update its top-level usage string to `openapi [--check | --skip-dart]`.

Add a unit test module to `openapi.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_puts_a_snapshot_back() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let root = dir.path().join("api");
        std::fs::create_dir_all(root.join("clients")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("a.dart"), "one\r\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("clients/b.dart"), "two\n").unwrap_or_else(|e| unreachable!("{e}"));
        let before = snapshot(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(before.get(Path::new("a.dart")).map(String::as_str), Some("one\n"));

        std::fs::write(root.join("a.dart"), "changed").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("new.dart"), "extra").unwrap_or_else(|e| unreachable!("{e}"));
        restore(&root, &before).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(snapshot(&root).unwrap_or_else(|e| unreachable!("{e}")), before);
    }
}
```

- [ ] **Step 4: Generate and run the contract test.** From `mobile/`: `flutter pub get`. From `backend/`: `cargo xtask openapi`. From `mobile/`: `flutter test test/api/generated_contract_test.dart`.
Expected: `mobile/lib/api/` exists and the test PASSES. Then `cargo xtask openapi --skip-dart` prints the warning and leaves `mobile/lib/api` untouched, and `cargo xtask openapi --check --skip-dart` exits with the usage error.

If a generated name or signature differs from the table, first try to fix it through `swagger_parser.yaml` options. If that's impossible, change the table in this plan, the contract test, and Task 10's code to the generated names, and note it in the commit message.

If the generator cannot handle the OpenAPI 3.1 constructs here (`type: [string, null]`, `allOf` in `MeDto`, `oneOf` with `null` in `PatchUserRequest`), stop and switch to the spec's fallback (`openapi-generator` `dart-dio` through Docker). Record that in the spec and tell the user.

- [ ] **Step 5: Check byte stability.** From `backend/`: `cargo xtask openapi --check` (Windows). Expected: "… are current". Then confirm `git status` shows no changes under `mobile/lib/api`, and `git diff --stat` is empty after the check. Task 19's CI `contract` job proves the Linux side; if it fails there only, normalize the generator config first, then fall back as in Step 4.

- [ ] **Step 6: Gates and commit.** Run the Rust quality gates and the app gates.

```bash
git add mobile/swagger_parser.yaml mobile/lib/api mobile/test/api mobile/pubspec.yaml mobile/pubspec.lock backend/xtask/src/openapi.rs backend/xtask/src/main.rs
git commit -m "feat(xtask): generate the Dart API client into mobile/lib/api

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: App config, API errors, and messages

**Files:**
- Create: `mobile/lib/data/config/app_config.dart`, `mobile/lib/data/http/api_error.dart`, `mobile/lib/data/http/api_call.dart`, `mobile/lib/ui/errors/messages.dart`
- Test: `mobile/test/data/config/app_config_test.dart`, `mobile/test/data/http/api_error_test.dart`, `mobile/test/ui/errors/messages_test.dart`

**Interfaces:**
- Produces:
  - `AppEnv` enum.
  - `AppConfig({required Uri apiBaseUrl, required AppEnv env})` with `AppConfig.fromEnvironment()`, `AppConfig.parse({required String apiBaseUrl, required String env})`, and the getters `scheme`, `redirectUrl`, `postLogoutRedirectUrl`.
  - `ApiError(int status, String code, [String? detail])` with `ApiError.from(int status, Object? body)` and `ApiError.fromDio(DioException e)`. `fromDio` maps an `AuthUnavailable` cause (Task 8) to `network` and a `NotSignedIn` cause to `unauthenticated`.
  - `Future<T> apiCall<T>(Future<T> Function() call)`.
  - `String messageFor(Object error)`.
- Consumes: `NotSignedIn` and `AuthUnavailable` from Task 8. To keep this task self-contained, define both here in `mobile/lib/data/auth/not_signed_in.dart`; Task 8 imports them, and `auth_client.dart` re-exports `AuthUnavailable`.

- [ ] **Step 1: Write the failing tests**

`mobile/test/data/config/app_config_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/config/app_config.dart';

void main() {
  test('parses the dart-define values and trims a trailing slash', () {
    final c = AppConfig.parse(apiBaseUrl: 'https://postit.local:44310/', env: 'development');
    expect(c.apiBaseUrl.toString(), 'https://postit.local:44310');
    expect(c.env, AppEnv.development);
    expect(c.redirectUrl, 'postit-dev://auth/callback');
    expect(c.postLogoutRedirectUrl, 'postit-dev://auth/logout');
  });

  test('each environment has its own scheme', () {
    expect(AppConfig.parse(apiBaseUrl: 'https://a', env: 'qa').scheme, 'postit-qa');
    expect(AppConfig.parse(apiBaseUrl: 'https://a', env: 'production').scheme, 'postit');
  });

  test('a missing base URL names the fix', () {
    expect(
      () => AppConfig.parse(apiBaseUrl: '', env: 'qa'),
      throwsA(isA<FormatException>().having((e) => e.message, 'message', contains('--dart-define-from-file'))),
    );
  });

  test('an unknown environment is rejected', () {
    expect(() => AppConfig.parse(apiBaseUrl: 'https://a', env: 'staging'), throwsFormatException);
  });
}
```

`mobile/test/data/http/api_error_test.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';

void main() {
  final req = RequestOptions(path: '/api/v1/users');

  test('reads code and detail from problem+json', () {
    final e = ApiError.from(409, {'code': 'last_admin', 'detail': 'nope', 'status': 409});
    expect((e.status, e.code, e.detail), (409, 'last_admin', 'nope'));
  });

  test('a body without a code is unknown', () {
    expect(ApiError.from(500, 'oops').code, 'unknown');
  });

  test('a dio response error becomes its problem', () {
    final e = ApiError.fromDio(DioException(
      requestOptions: req,
      response: Response(requestOptions: req, statusCode: 403, data: {'code': 'forbidden'}),
    ));
    expect((e.status, e.code), (403, 'forbidden'));
  });

  test('no response means a network error', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, type: DioExceptionType.connectionError));
    expect((e.status, e.code), (0, 'network'));
  });

  test('a signed-out session is unauthenticated', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, error: const NotSignedIn()));
    expect((e.status, e.code), (401, 'unauthenticated'));
  });

  test('an unreachable identity provider is a network error', () {
    final e = ApiError.fromDio(DioException(requestOptions: req, error: const AuthUnavailable()));
    expect((e.status, e.code), (0, 'network'));
  });
}
```

`mobile/test/ui/errors/messages_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/ui/errors/messages.dart';

void main() {
  test('known codes have messages', () {
    expect(messageFor(const ApiError(409, 'last_admin')), 'That would remove the last active admin.');
    expect(messageFor(const ApiError(0, 'network')), contains("Can't reach postit"));
  });

  test('an unknown code shows the server detail', () {
    expect(messageFor(const ApiError(418, 'teapot', 'Short and stout.')), 'Short and stout.');
  });

  test('an unreachable identity provider reads as a network problem', () {
    expect(messageFor(const AuthUnavailable()), contains("Can't reach postit"));
  });

  test('anything else gets the generic message', () {
    expect(messageFor(StateError('x')), 'Something went wrong. Try again.');
  });
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `flutter test test/data test/ui` (from `mobile/`)
Expected: FAIL, the imports don't exist.

- [ ] **Step 3: Implement**

`mobile/lib/data/auth/not_signed_in.dart`:

```dart
/// Thrown when an API call needs a session and there is none.
class NotSignedIn implements Exception {
  const NotSignedIn();
}

/// The identity provider could not be reached (offline, DNS, TLS, timeout). The session is
/// kept: this is not a rejection, and retrying later may succeed.
class AuthUnavailable implements Exception {
  const AuthUnavailable();
}
```

`mobile/lib/data/config/app_config.dart`:

```dart
/// The deployment environment a build targets.
enum AppEnv { development, qa, production }

/// Build-time settings from `--dart-define-from-file=config/<env>.json`.
class AppConfig {
  const AppConfig({required this.apiBaseUrl, required this.env});

  final Uri apiBaseUrl;
  final AppEnv env;

  factory AppConfig.fromEnvironment() => AppConfig.parse(
        apiBaseUrl: const String.fromEnvironment('API_BASE_URL'),
        env: const String.fromEnvironment('ENV'),
      );

  factory AppConfig.parse({required String apiBaseUrl, required String env}) {
    if (apiBaseUrl.isEmpty) {
      throw const FormatException(
          'API_BASE_URL is not set; build with --dart-define-from-file=config/<env>.json');
    }
    final trimmed = apiBaseUrl.endsWith('/') ? apiBaseUrl.substring(0, apiBaseUrl.length - 1) : apiBaseUrl;
    final parsed = AppEnv.values.where((e) => e.name == env).firstOrNull;
    if (parsed == null) {
      throw FormatException('ENV must be development, qa or production, got "$env"');
    }
    return AppConfig(apiBaseUrl: Uri.parse(trimmed), env: parsed);
  }

  /// The custom URL scheme registered for this flavor.
  String get scheme => switch (env) {
        AppEnv.development => 'postit-dev',
        AppEnv.qa => 'postit-qa',
        AppEnv.production => 'postit',
      };

  String get redirectUrl => '$scheme://auth/callback';
  String get postLogoutRedirectUrl => '$scheme://auth/logout';
}
```

`mobile/lib/data/http/api_error.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:postit/data/auth/not_signed_in.dart';

/// A failed API call: the problem+json `code` plus its HTTP status. Status 0 and code
/// `network` mean the server was not reached.
class ApiError implements Exception {
  const ApiError(this.status, this.code, [this.detail]);

  final int status;
  final String code;
  final String? detail;

  factory ApiError.from(int status, Object? body) {
    if (body is Map) {
      final code = body['code'];
      final detail = body['detail'];
      return ApiError(status, code is String ? code : 'unknown', detail is String ? detail : null);
    }
    return ApiError(status, 'unknown');
  }

  factory ApiError.fromDio(DioException e) {
    if (e.error is NotSignedIn) return const ApiError(401, 'unauthenticated');
    if (e.error is AuthUnavailable) return const ApiError(0, 'network');
    final response = e.response;
    if (response == null) return const ApiError(0, 'network');
    return ApiError.from(response.statusCode ?? 0, response.data);
  }

  @override
  String toString() => 'ApiError($status, $code)';
}
```

`mobile/lib/data/http/api_call.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:postit/data/http/api_error.dart';

/// Runs a generated-client call and turns its `DioException` into an [ApiError].
Future<T> apiCall<T>(Future<T> Function() call) async {
  try {
    return await call();
  } on DioException catch (e) {
    throw ApiError.fromDio(e);
  }
}
```

`mobile/lib/ui/errors/messages.dart` (the same table as `web/src/api/errors.ts`, plus `network`):

```dart
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';

const _messages = <String, String>{
  'unauthenticated': 'Your session expired. Sign in again.',
  'account_pending': 'Your account is waiting for approval.',
  'account_disabled': 'Your account is disabled.',
  'forbidden': 'You are not allowed to do that.',
  'not_found': 'That item no longer exists.',
  'validation_failed': 'The request was not valid.',
  'last_admin': 'That would remove the last active admin.',
  'user_deleting': 'That user is being deleted.',
  'rate_limited': 'Too many requests. Wait a moment and try again.',
  'version_conflict': 'Someone else changed this. Reload and try again.',
  'precondition_required': 'Reload and try again.',
  'idempotency_key_reused': 'That request was already used with different data.',
  'idempotency_in_progress': 'That request is already running.',
  'request_timeout': 'The request took too long. Try again.',
  'payload_too_large': 'That is too large to send.',
  'job_action_not_allowed': "That job can't take this action right now. It may have changed; refresh and try again.",
  'unavailable': 'The service is starting up. Try again shortly.',
  'internal': 'Something went wrong on the server.',
  'network': "Can't reach postit. Check your connection and try again.",
};

/// The user-facing message for an error.
String messageFor(Object error) {
  if (error is AuthUnavailable) return _messages['network']!;
  if (error is ApiError) {
    return _messages[error.code] ?? error.detail ?? 'Something went wrong. Try again.';
  }
  return 'Something went wrong. Try again.';
}
```

- [ ] **Step 4: Run the tests**

Run: `flutter test test/data test/ui` then `flutter analyze`
Expected: PASS, no issues.

- [ ] **Step 5: Commit**

```bash
git add mobile/lib/data mobile/lib/ui mobile/test/data mobile/test/ui
git commit -m "feat(app): build config, API errors and user-facing messages

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The auth seam, the session store, token revocation, and `SessionManager`

**Files:**
- Create: `mobile/lib/data/auth/auth_client.dart`, `mobile/lib/data/auth/app_auth_client.dart`, `mobile/lib/data/auth/session_store.dart`, `mobile/lib/data/auth/token_revoker.dart`, `mobile/lib/data/auth/session_manager.dart`
- Create: `mobile/test/helpers/fakes.dart`, `mobile/test/helpers/scripted_adapter.dart`
- Test: `mobile/test/data/auth/session_manager_test.dart`, `mobile/test/data/auth/token_revoker_test.dart`

**Interfaces:**
- Consumes: `NotSignedIn` (Task 7).
- Produces:
  - **Token types:** `TokenSet`, `OidcClientConfig`, `AuthCancelled`, `AuthFailed(String message)` (the IdP rejected the request), and the re-exported `AuthUnavailable` (the IdP was not reached).
  - **`AuthClient` interface:** `signIn()`, `refresh(String)`, `endSession(String)`. `AppAuthClient(OidcClientConfig)` implements it.
  - **Storage:** `StoredSession`, the `SessionStore` interface (`read`/`write`/`clear`), and `SecureSessionStore`.
  - **Revocation:** `TokenRevoker(Dio, OidcClientConfig).revoke(String)`.
  - **Session state:** `SessionStatus`, `SessionState` (`.loading()`, `.signedOut([notice])`, `.signedIn()`), and `sessionExpiredNotice`.
  - **`SessionManager`:** `state` (a `ValueNotifier<SessionState>`), `restore()`, `signIn()`, `accessToken()`, `refresh()`, `expire()`, `signOut()`, `dispose()`.
  - **Test helpers:** `FakeAuthClient`, `MemoryStore`, `tokens(...)`, `ScriptedAdapter`.

- [ ] **Step 1: Write the test helpers**

`mobile/test/helpers/fakes.dart`:

```dart
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_store.dart';

TokenSet tokens(String n, {DateTime? expiresAt, String? refreshToken, bool noRefresh = false}) => TokenSet(
      accessToken: 'access-$n',
      expiresAt: expiresAt ?? DateTime(2030),
      refreshToken: noRefresh ? null : (refreshToken ?? 'refresh-$n'),
      idToken: 'id-$n',
    );

class FakeAuthClient implements AuthClient {
  Future<TokenSet> Function() onSignIn = () async => tokens('signin');
  Future<TokenSet> Function(String refreshToken) onRefresh = (rt) async => tokens('refreshed');
  Future<void> Function(String idToken) onEndSession = (_) async {};
  int refreshCalls = 0;
  final refreshedWith = <String>[];
  final endedSessions = <String>[];

  @override
  Future<TokenSet> signIn() => onSignIn();

  @override
  Future<TokenSet> refresh(String refreshToken) {
    refreshCalls++;
    refreshedWith.add(refreshToken);
    return onRefresh(refreshToken);
  }

  @override
  Future<void> endSession(String idTokenHint) {
    endedSessions.add(idTokenHint);
    return onEndSession(idTokenHint);
  }
}

class MemoryStore implements SessionStore {
  MemoryStore([this.value]);
  StoredSession? value;
  bool failReads = false;

  @override
  Future<StoredSession?> read() async {
    if (failReads) throw StateError('keystore unavailable');
    return value;
  }

  @override
  Future<void> write(StoredSession session) async => value = session;

  @override
  Future<void> clear() async => value = null;
}
```

`mobile/test/helpers/scripted_adapter.dart`:

```dart
import 'dart:convert';
import 'dart:typed_data';

import 'package:dio/dio.dart';

/// A dio adapter that answers from [respond] and records every request.
class ScriptedAdapter implements HttpClientAdapter {
  ScriptedAdapter(this.respond);

  final Future<ResponseBody> Function(RequestOptions options) respond;
  final requests = <RequestOptions>[];

  static ResponseBody json(int status, Object? body) => ResponseBody.fromString(
        jsonEncode(body),
        status,
        headers: {
          Headers.contentTypeHeader: ['application/json'],
        },
      );

  @override
  Future<ResponseBody> fetch(RequestOptions options, Stream<Uint8List>? requestStream, Future<void>? cancelFuture) {
    requests.add(options);
    return respond(options);
  }

  @override
  void close({bool force = false}) {}
}
```

- [ ] **Step 2: Write the failing `SessionManager` tests** (`mobile/test/data/auth/session_manager_test.dart`):

```dart
import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/auth/session_store.dart';

import '../../helpers/fakes.dart';

void main() {
  late FakeAuthClient client;
  late MemoryStore store;
  late List<String> revoked;
  var now = DateTime(2030, 1, 1, 12);

  SessionManager manager({Future<void> Function(String)? revoke}) => SessionManager(
        client: client,
        store: store,
        revoke: revoke ?? (rt) async => revoked.add(rt),
        now: () => now,
      );

  setUp(() {
    client = FakeAuthClient();
    store = MemoryStore();
    revoked = [];
    now = DateTime(2030, 1, 1, 12);
  });

  test('restore with nothing stored is signed out without a notice', () async {
    final m = manager();
    await m.restore();
    expect(m.state.value, const SessionState.signedOut());
    expect(client.refreshCalls, 0);
  });

  test('restore refreshes the stored token and signs in', () async {
    store.value = const StoredSession(refreshToken: 'stored', idToken: 'id-stored');
    final m = manager();
    await m.restore();
    expect(client.refreshedWith, ['stored']);
    expect(m.state.value, const SessionState.signedIn());
    expect(await m.accessToken(), 'access-refreshed');
  });

  test('restore persists a rotated refresh token', () async {
    store.value = const StoredSession(refreshToken: 'old');
    client.onRefresh = (_) async => tokens('r', refreshToken: 'rotated');
    await manager().restore();
    expect(store.value?.refreshToken, 'rotated');
  });

  test('a refresh without a new refresh token keeps the old one', () async {
    store.value = const StoredSession(refreshToken: 'old');
    client.onRefresh = (_) async => tokens('r', noRefresh: true);
    await manager().restore();
    expect(store.value?.refreshToken, 'old');
  });

  test('restore that fails to refresh is signed out silently and clears the store', () async {
    store.value = const StoredSession(refreshToken: 'revoked');
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    final m = manager();
    await m.restore();
    expect(m.state.value, const SessionState.signedOut());
    expect(store.value, isNull);
  });

  test('restore that cannot reach the provider keeps the stored session and fails', () async {
    store.value = const StoredSession(refreshToken: 'stored', idToken: 'id-stored');
    client.onRefresh = (_) async => throw const AuthUnavailable();
    final m = manager();
    await expectLater(m.restore(), throwsA(isA<AuthUnavailable>()));
    expect(store.value?.refreshToken, 'stored');
    // A retry (the error screen rebuilds the manager) restores normally.
    client.onRefresh = (_) async => tokens('later');
    final again = manager();
    await again.restore();
    expect(again.state.value, const SessionState.signedIn());
  });

  test('unreadable store means signed out', () async {
    store.failReads = true;
    final m = manager();
    await m.restore();
    expect(m.state.value.status, SessionStatus.signedOut);
  });

  test('sign-in stores the refresh token and signs in', () async {
    final m = manager();
    await m.restore();
    await m.signIn();
    expect(m.state.value, const SessionState.signedIn());
    expect(store.value?.refreshToken, 'refresh-signin');
    expect(store.value?.idToken, 'id-signin');
  });

  test('cancelled sign-in stays signed out', () async {
    client.onSignIn = () async => throw const AuthCancelled();
    final m = manager();
    await m.restore();
    await expectLater(m.signIn(), throwsA(isA<AuthCancelled>()));
    expect(m.state.value, const SessionState.signedOut());
  });

  test('concurrent refreshes share one call', () async {
    final m = manager();
    await m.signIn();
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final all = Future.wait([m.refresh(), m.refresh(), m.refresh()]);
    gate.complete(tokens('shared'));
    expect(await all, ['access-shared', 'access-shared', 'access-shared']);
    expect(client.refreshCalls, 1);
  });

  test('the access token is refreshed within 60 seconds of expiry', () async {
    client.onSignIn = () async => tokens('a', expiresAt: now.add(const Duration(seconds: 30)));
    final m = manager();
    await m.signIn();
    expect(await m.accessToken(), 'access-refreshed');
    expect(client.refreshCalls, 1);
  });

  test('a fresh access token is used as is', () async {
    client.onSignIn = () async => tokens('a', expiresAt: now.add(const Duration(minutes: 10)));
    final m = manager();
    await m.signIn();
    expect(await m.accessToken(), 'access-a');
    expect(client.refreshCalls, 0);
  });

  test('a failed refresh expires the session with a notice', () async {
    final m = manager();
    await m.signIn();
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    await expectLater(m.refresh(), throwsA(isA<NotSignedIn>()));
    expect(m.state.value, const SessionState.signedOut(sessionExpiredNotice));
    expect(store.value, isNull);
  });

  test('a refresh that cannot reach the provider keeps the session', () async {
    final m = manager();
    await m.signIn();
    client.onRefresh = (_) async => throw const AuthUnavailable();
    await expectLater(m.refresh(), throwsA(isA<AuthUnavailable>()));
    expect(m.state.value, const SessionState.signedIn());
    expect(store.value?.refreshToken, 'refresh-signin');
    client.onRefresh = (_) async => tokens('back');
    expect(await m.refresh(), 'access-back');
  });

  test('signing out during a refresh does not bring the session back', () async {
    final m = manager();
    await m.signIn();
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final inFlight = m.refresh();
    await m.signOut();
    gate.complete(tokens('late', refreshToken: 'late-refresh'));
    await expectLater(inFlight, throwsA(isA<NotSignedIn>()));
    expect(m.state.value, const SessionState.signedOut());
    expect(store.value, isNull);
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });

  test('a session saved while signing out is cleared again', () async {
    final writing = Completer<void>();
    final slow = _SlowStore(writing.future);
    final m = SessionManager(client: client, store: slow, revoke: (_) async {}, now: () => now);
    final signingIn = m.signIn();
    await Future<void>.delayed(Duration.zero); // signIn is now blocked in the store write
    await m.signOut();
    writing.complete();
    await expectLater(signingIn, throwsA(isA<NotSignedIn>()));
    expect(slow.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
  });

  test('accessToken without a session throws NotSignedIn', () async {
    final m = manager();
    await m.restore();
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });

  test('sign-out revokes, ends the session and clears', () async {
    final m = manager();
    await m.signIn();
    await m.signOut();
    expect(revoked, ['refresh-signin']);
    expect(client.endedSessions, ['id-signin']);
    expect(store.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
  });

  test('sign-out clears even when revocation fails and the browser step is cancelled', () async {
    final m = manager(revoke: (_) async => throw StateError('offline'));
    client.onEndSession = (_) async => throw const AuthCancelled();
    await m.signIn();
    await m.signOut();
    expect(store.value, isNull);
    expect(m.state.value, const SessionState.signedOut());
    await expectLater(m.accessToken(), throwsA(isA<NotSignedIn>()));
  });
}

/// A store whose writes wait for [_gate], to hold a save open while the user signs out.
class _SlowStore extends MemoryStore {
  _SlowStore(this._gate);
  final Future<void> _gate;

  @override
  Future<void> write(StoredSession session) async {
    await _gate;
    value = session;
  }
}
```

- [ ] **Step 3: Write the failing revoker test** (`mobile/test/data/auth/token_revoker_test.dart`):

```dart
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/token_revoker.dart';

import '../../helpers/scripted_adapter.dart';

const _config = OidcClientConfig(
  issuer: 'https://idp.test',
  clientId: 'android-1',
  scopes: ['openid'],
  redirectUrl: 'postit-dev://auth/callback',
  postLogoutRedirectUrl: 'postit-dev://auth/logout',
);

void main() {
  test('posts the refresh token to the revocation endpoint', () async {
    final adapter = ScriptedAdapter((o) async => o.path.endsWith('openid-configuration')
        ? ScriptedAdapter.json(200, {'revocation_endpoint': 'https://idp.test/oauth/v2/revoke'})
        : ScriptedAdapter.json(200, {}));
    final dio = Dio()..httpClientAdapter = adapter;
    await TokenRevoker(dio, _config).revoke('rt-1');
    final post = adapter.requests.last;
    expect(post.method, 'POST');
    expect(post.uri.toString(), 'https://idp.test/oauth/v2/revoke');
    expect(post.data, {'token': 'rt-1', 'token_type_hint': 'refresh_token', 'client_id': 'android-1'});
    expect(post.contentType, Headers.formUrlEncodedContentType);
  });

  test('a provider without revocation is skipped', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {'issuer': 'https://idp.test'}));
    final dio = Dio()..httpClientAdapter = adapter;
    await TokenRevoker(dio, _config).revoke('rt-1');
    expect(adapter.requests, hasLength(1));
  });
}
```

Run: `flutter test test/data/auth`
Expected: FAIL, the imports don't exist.

- [ ] **Step 4: Implement**

`mobile/lib/data/auth/auth_client.dart`:

```dart
export 'package:postit/data/auth/not_signed_in.dart' show AuthUnavailable;

/// Tokens from one sign-in or refresh.
class TokenSet {
  const TokenSet({required this.accessToken, required this.expiresAt, this.refreshToken, this.idToken});

  final String accessToken;
  final DateTime expiresAt;
  final String? refreshToken;
  final String? idToken;
}

/// The OIDC client this build signs in with.
class OidcClientConfig {
  const OidcClientConfig({
    required this.issuer,
    required this.clientId,
    required this.scopes,
    required this.redirectUrl,
    required this.postLogoutRedirectUrl,
  });

  final String issuer;
  final String clientId;
  final List<String> scopes;
  final String redirectUrl;
  final String postLogoutRedirectUrl;

  String get discoveryUrl => '$issuer/.well-known/openid-configuration';
}

/// The user closed the browser before finishing.
class AuthCancelled implements Exception {
  const AuthCancelled();
}

/// The identity provider refused or failed; [message] is safe to show.
class AuthFailed implements Exception {
  const AuthFailed(this.message);
  final String message;

  @override
  String toString() => 'AuthFailed: $message';
}

/// The only door to the OIDC library, so tests use a fake and a library change touches
/// one file. Every method throws [AuthCancelled], [AuthFailed] (rejected), or
/// [AuthUnavailable] (not reached).
abstract interface class AuthClient {
  Future<TokenSet> signIn();
  Future<TokenSet> refresh(String refreshToken);
  Future<void> endSession(String idTokenHint);
}
```

`mobile/lib/data/auth/app_auth_client.dart` (if Task 5 found different exception or field names, use those):

```dart
import 'package:flutter/services.dart';
import 'package:flutter_appauth/flutter_appauth.dart';
import 'package:postit/data/auth/auth_client.dart';

/// OAuth error codes (RFC 6749 §4.1.2.1 and §5.2, OIDC Core §3.1.2.6): the provider answered
/// and said no. AppAuth leaves `error` null for its own failures (network, discovery, TLS).
/// `server_error` and `temporarily_unavailable` are left out on purpose: they say nothing
/// about the refresh token, so they keep the session like a network failure.
const _oauthErrors = {
  'invalid_request', 'invalid_client', 'invalid_grant', 'unauthorized_client', 'unsupported_grant_type',
  'invalid_scope', 'access_denied', 'unsupported_response_type',
  'interaction_required', 'login_required', 'account_selection_required', 'consent_required',
};

/// True when the provider rejected the request, as opposed to not being reached. Task 5's
/// spike confirms this rule; adjust it there if the recorded error details differ.
bool _isRejection(FlutterAppAuthPlatformErrorDetails details) => _oauthErrors.contains(details.error);

/// [AuthClient] over `flutter_appauth`: PKCE in the system browser.
class AppAuthClient implements AuthClient {
  AppAuthClient(this._config, {FlutterAppAuth appAuth = const FlutterAppAuth()}) : _appAuth = appAuth;

  final OidcClientConfig _config;
  final FlutterAppAuth _appAuth;

  @override
  Future<TokenSet> signIn() => _guard(() async => _tokens(await _appAuth.authorizeAndExchangeCode(
        AuthorizationTokenRequest(
          _config.clientId,
          _config.redirectUrl,
          discoveryUrl: _config.discoveryUrl,
          scopes: _config.scopes,
        ),
      )));

  @override
  Future<TokenSet> refresh(String refreshToken) => _guard(() async => _tokens(await _appAuth.token(
        TokenRequest(
          _config.clientId,
          _config.redirectUrl,
          discoveryUrl: _config.discoveryUrl,
          refreshToken: refreshToken,
          scopes: _config.scopes,
        ),
      )));

  @override
  Future<void> endSession(String idTokenHint) => _guard(() async {
        await _appAuth.endSession(EndSessionRequest(
          idTokenHint: idTokenHint,
          postLogoutRedirectUrl: _config.postLogoutRedirectUrl,
          discoveryUrl: _config.discoveryUrl,
        ));
      });

  Future<T> _guard<T>(Future<T> Function() call) async {
    try {
      return await call();
    } on FlutterAppAuthUserCancelledException {
      throw const AuthCancelled();
    } on FlutterAppAuthPlatformException catch (e) {
      final details = e.platformErrorDetails;
      if (!_isRejection(details)) throw const AuthUnavailable();
      throw AuthFailed(details.errorDescription ?? details.error ?? 'Sign-in failed.');
    } on PlatformException {
      // Any other plugin failure: treat as not reached, never as a rejected session.
      throw const AuthUnavailable();
    }
  }

  TokenSet _tokens(TokenResponse response) {
    final access = response.accessToken;
    if (access == null) throw const AuthFailed('The identity provider returned no access token.');
    return TokenSet(
      accessToken: access,
      expiresAt: response.accessTokenExpirationDateTime ?? DateTime.now().add(const Duration(minutes: 5)),
      refreshToken: response.refreshToken,
      idToken: response.idToken,
    );
  }
}
```

`mobile/lib/data/auth/session_store.dart`:

```dart
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// What survives an app restart: never the access token.
class StoredSession {
  const StoredSession({required this.refreshToken, this.idToken});
  final String refreshToken;
  final String? idToken;
}

abstract interface class SessionStore {
  Future<StoredSession?> read();
  Future<void> write(StoredSession session);
  Future<void> clear();
}

/// Keystore (Android) or Keychain (iOS).
class SecureSessionStore implements SessionStore {
  SecureSessionStore([this._storage = const FlutterSecureStorage()]);

  final FlutterSecureStorage _storage;
  static const _refreshKey = 'postit.refresh_token';
  static const _idKey = 'postit.id_token';

  @override
  Future<StoredSession?> read() async {
    final refresh = await _storage.read(key: _refreshKey);
    if (refresh == null) return null;
    return StoredSession(refreshToken: refresh, idToken: await _storage.read(key: _idKey));
  }

  @override
  Future<void> write(StoredSession session) async {
    await _storage.write(key: _refreshKey, value: session.refreshToken);
    final id = session.idToken;
    if (id == null) {
      await _storage.delete(key: _idKey);
    } else {
      await _storage.write(key: _idKey, value: id);
    }
  }

  @override
  Future<void> clear() async {
    await _storage.delete(key: _refreshKey);
    await _storage.delete(key: _idKey);
  }
}
```

`mobile/lib/data/auth/token_revoker.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:postit/data/auth/auth_client.dart';

/// Revokes a refresh token at the provider's `revocation_endpoint` (RFC 7009).
class TokenRevoker {
  TokenRevoker(this._dio, this._config);

  final Dio _dio;
  final OidcClientConfig _config;

  Future<void> revoke(String refreshToken) async {
    final discovery = await _dio.get<Map<String, dynamic>>(_config.discoveryUrl);
    final endpoint = discovery.data?['revocation_endpoint'];
    // A provider without revocation leaves the token to expire on its own.
    if (endpoint is! String) return;
    await _dio.post<void>(
      endpoint,
      data: {'token': refreshToken, 'token_type_hint': 'refresh_token', 'client_id': _config.clientId},
      options: Options(contentType: Headers.formUrlEncodedContentType),
    );
  }
}
```

`mobile/lib/data/auth/session_manager.dart`:

```dart
import 'package:flutter/foundation.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_store.dart';

enum SessionStatus { loading, signedOut, signedIn }

/// Where the session stands; [notice] explains a sign-out the user did not ask for.
@immutable
class SessionState {
  const SessionState._(this.status, [this.notice]);
  const SessionState.loading() : this._(SessionStatus.loading);
  const SessionState.signedOut([String? notice]) : this._(SessionStatus.signedOut, notice);
  const SessionState.signedIn() : this._(SessionStatus.signedIn);

  final SessionStatus status;
  final String? notice;

  @override
  bool operator ==(Object other) => other is SessionState && other.status == status && other.notice == notice;

  @override
  int get hashCode => Object.hash(status, notice);

  @override
  String toString() => 'SessionState($status, $notice)';
}

const sessionExpiredNotice = 'Your session expired. Sign in again.';

typedef RevokeToken = Future<void> Function(String refreshToken);

/// Owns the tokens: the access token in memory, the refresh and ID tokens in [SessionStore].
class SessionManager {
  SessionManager({
    required AuthClient client,
    required SessionStore store,
    required RevokeToken revoke,
    DateTime Function() now = DateTime.now,
  })  : _client = client,
        _store = store,
        _revoke = revoke,
        _now = now;

  final AuthClient _client;
  final SessionStore _store;
  final RevokeToken _revoke;
  final DateTime Function() _now;

  final ValueNotifier<SessionState> state = ValueNotifier(const SessionState.loading());

  /// Refresh this long before expiry, so a request never leaves with a dying token.
  static const refreshMargin = Duration(seconds: 60);

  String? _accessToken;
  DateTime? _expiresAt;
  String? _refreshToken;
  String? _idToken;
  Future<String>? _refreshing;

  /// Bumped by every wipe. A sign-in or refresh that started under an older generation
  /// finished after a sign-out, so its tokens are discarded instead of reviving the session.
  int _generation = 0;

  /// Resumes a stored session, or settles on signed out without a notice. Throws
  /// [AuthUnavailable], keeping the stored session, when the provider cannot be reached.
  Future<void> restore() async {
    StoredSession? stored;
    try {
      stored = await _store.read();
    } on Object {
      stored = null;
    }
    if (stored == null) {
      await _wipe();
      state.value = const SessionState.signedOut();
      return;
    }
    _refreshToken = stored.refreshToken;
    _idToken = stored.idToken;
    try {
      await _apply(await _client.refresh(stored.refreshToken));
    } on AuthUnavailable {
      // Offline or the provider is down: keep the stored session for the next try.
      _refreshToken = null;
      _idToken = null;
      rethrow;
    } on Object {
      await _wipe();
      state.value = const SessionState.signedOut();
    }
  }

  /// Opens the browser sign-in. Throws [AuthCancelled], [AuthFailed], or [AuthUnavailable],
  /// or [NotSignedIn] if a sign-out raced it.
  Future<void> signIn() async => _apply(await _client.signIn());

  /// A usable access token, refreshed first when it is close to expiry.
  Future<String> accessToken() async {
    final token = _accessToken;
    final expiresAt = _expiresAt;
    if (token == null || expiresAt == null) throw const NotSignedIn();
    if (!_now().isBefore(expiresAt.subtract(refreshMargin))) return refresh();
    return token;
  }

  /// One refresh shared by every caller. A rejection expires the session and throws
  /// [NotSignedIn]; an unreachable provider throws [AuthUnavailable] and keeps the session.
  Future<String> refresh() => _refreshing ??= _refreshOnce().whenComplete(() => _refreshing = null);

  Future<String> _refreshOnce() async {
    final generation = _generation;
    final refreshToken = _refreshToken;
    if (refreshToken == null) {
      await expire();
      throw const NotSignedIn();
    }
    final TokenSet next;
    try {
      next = await _client.refresh(refreshToken);
    } on AuthUnavailable {
      rethrow;
    } on Object {
      // Only expire the session this refresh belonged to, never a newer one.
      if (generation == _generation) await expire();
      throw const NotSignedIn();
    }
    if (generation != _generation) throw const NotSignedIn();
    await _apply(next);
    return next.accessToken;
  }

  /// Ends the session locally with the "expired" notice.
  Future<void> expire() async {
    if (state.value.status == SessionStatus.signedOut) return;
    await _wipe();
    state.value = const SessionState.signedOut(sessionExpiredNotice);
  }

  /// Revokes the refresh token, ends the IdP session, and clears local state, which happens
  /// even if either remote step fails.
  Future<void> signOut() async {
    final refreshToken = _refreshToken;
    final idToken = _idToken;
    try {
      if (refreshToken != null) await _revoke(refreshToken);
    } on Object {
      // Best effort: the refresh token still expires at the IdP.
    }
    try {
      if (idToken != null) await _client.endSession(idToken);
    } on Object {
      // Cancelled or unreachable: local sign-out still happens.
    }
    await _wipe();
    state.value = const SessionState.signedOut();
  }

  /// Saves [tokens] and signs in, unless a sign-out happened meanwhile: then the save is
  /// undone and [NotSignedIn] is thrown.
  Future<void> _apply(TokenSet tokens) async {
    final generation = _generation;
    final refreshToken = tokens.refreshToken ?? _refreshToken;
    final idToken = tokens.idToken ?? _idToken;
    if (refreshToken != null) {
      try {
        await _store.write(StoredSession(refreshToken: refreshToken, idToken: idToken));
      } on Object {
        // Storage unavailable: the session lasts until the app closes.
      }
    }
    if (generation != _generation) {
      try {
        await _store.clear();
      } on Object {
        // Nothing more to do.
      }
      throw const NotSignedIn();
    }
    _accessToken = tokens.accessToken;
    _expiresAt = tokens.expiresAt;
    _refreshToken = refreshToken;
    _idToken = idToken;
    state.value = const SessionState.signedIn();
  }

  Future<void> _wipe() async {
    _generation++;
    _accessToken = null;
    _expiresAt = null;
    _refreshToken = null;
    _idToken = null;
    try {
      await _store.clear();
    } on Object {
      // Nothing more to do; the next restore reads nothing usable either way.
    }
  }

  void dispose() => state.dispose();
}
```

- [ ] **Step 5: Run the tests**

Run: `flutter test test/data/auth` then `flutter analyze`
Expected: PASS, no issues.

- [ ] **Step 6: Commit**

```bash
git add mobile/lib/data/auth mobile/test/helpers mobile/test/data/auth
git commit -m "feat(app): session manager over flutter_appauth with secure storage

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Dio factory, the auth interceptor, and the recheck interceptor

**Files:**
- Create: `mobile/lib/data/http/dio_factory.dart`, `mobile/lib/data/http/dev_trust.dart`, `mobile/lib/data/http/auth_interceptor.dart`, `mobile/lib/data/http/recheck_interceptor.dart`
- Test: `mobile/test/data/http/auth_interceptor_test.dart`, `mobile/test/data/http/recheck_interceptor_test.dart`, `mobile/test/data/http/dev_trust_test.dart`

**Interfaces:**
- Consumes: `SessionManager`, `NotSignedIn`, and `AuthUnavailable` (Task 8), `ScriptedAdapter`, `FakeAuthClient`, `MemoryStore`, the `assets/dev/` flavor asset (Task 4).
- Produces: `Dio createDio(Uri baseUrl, {SecurityContext? trust})`, `Future<SecurityContext?> devTrust(AppConfig, {AssetBundle? bundle})`, `AuthInterceptor(SessionManager, Dio)`, `RecheckInterceptor(void Function() onRecheck)`.

- [ ] **Step 1: Write the failing tests**

`mobile/test/data/http/auth_interceptor_test.dart`:

```dart
import 'dart:async';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/auth_interceptor.dart';
import 'package:postit/data/http/dio_factory.dart';

import '../../helpers/fakes.dart';
import '../../helpers/scripted_adapter.dart';

void main() {
  late FakeAuthClient client;
  late SessionManager session;

  Dio dioWith(ScriptedAdapter adapter) {
    final dio = createDio(Uri.parse('https://api.test'))..httpClientAdapter = adapter;
    dio.interceptors.add(AuthInterceptor(session, dio));
    return dio;
  }

  setUp(() async {
    client = FakeAuthClient();
    session = SessionManager(client: client, store: MemoryStore(), revoke: (_) async {});
    await session.signIn();
  });

  test('attaches the bearer token', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {}));
    await dioWith(adapter).get<Object>('/api/v1/me');
    expect(adapter.requests.single.headers['Authorization'], 'Bearer access-signin');
  });

  test('a 401 refreshes once and retries with the new token', () async {
    final adapter = ScriptedAdapter((o) async => o.headers['Authorization'] == 'Bearer access-signin'
        ? ScriptedAdapter.json(401, {'code': 'unauthenticated'})
        : ScriptedAdapter.json(200, {'ok': true}));
    final res = await dioWith(adapter).get<Map<String, dynamic>>('/api/v1/me');
    expect(res.data, {'ok': true});
    expect(client.refreshCalls, 1);
    expect(adapter.requests.last.headers['Authorization'], 'Bearer access-refreshed');
  });

  test('concurrent 401s share one refresh', () async {
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final adapter = ScriptedAdapter((o) async => o.headers['Authorization'] == 'Bearer access-signin'
        ? ScriptedAdapter.json(401, {'code': 'unauthenticated'})
        : ScriptedAdapter.json(200, {}));
    final dio = dioWith(adapter);
    final calls = Future.wait([dio.get<Object>('/a'), dio.get<Object>('/b'), dio.get<Object>('/c')]);
    await Future<void>.delayed(Duration.zero);
    gate.complete(tokens('new'));
    final results = await calls;
    expect(results.map((r) => r.statusCode), [200, 200, 200]);
    expect(client.refreshCalls, 1);
  });

  test('a second 401 expires the session', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(isA<DioException>().having((e) => e.response?.statusCode, 'status', 401)),
    );
    expect(session.state.value, const SessionState.signedOut(sessionExpiredNotice));
  });

  test('an unreachable provider fails the call as a network error and keeps the session', () async {
    client.onRefresh = (_) async => throw const AuthUnavailable();
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(isA<DioException>()
          .having((e) => e.type, 'type', DioExceptionType.connectionError)
          .having((e) => e.error, 'error', isA<AuthUnavailable>())),
    );
    expect(session.state.value, const SessionState.signedIn());
  });

  test('a failed refresh expires the session and fails the call', () async {
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(dioWith(adapter).get<Object>('/api/v1/me'), throwsA(isA<DioException>()));
    expect(session.state.value.status, SessionStatus.signedOut);
  });

  test('without a session the request is never sent', () async {
    await session.signOut();
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(isA<DioException>().having((e) => e.error, 'error', isA<NotSignedIn>())),
    );
    expect(adapter.requests, isEmpty);
  });
}
```

`mobile/test/data/http/recheck_interceptor_test.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/http/recheck_interceptor.dart';

import '../../helpers/scripted_adapter.dart';

void main() {
  Future<int> rechecks(String path, Map<String, Object?> body, int status) async {
    var count = 0;
    final dio = Dio(BaseOptions(baseUrl: 'https://api.test'))
      ..httpClientAdapter = ScriptedAdapter((o) async => ScriptedAdapter.json(status, body))
      ..interceptors.add(RecheckInterceptor(() => count++));
    try {
      await dio.get<Object>(path);
    } on DioException {
      // expected
    }
    return count;
  }

  test('forbidden, pending or disabled on another route rechecks /me', () async {
    expect(await rechecks('/api/v1/users', {'code': 'forbidden'}, 403), 1);
    expect(await rechecks('/api/v1/users', {'code': 'account_pending'}, 403), 1);
    expect(await rechecks('/api/v1/users', {'code': 'account_disabled'}, 403), 1);
  });

  test('errors from /me itself and other codes do not', () async {
    expect(await rechecks('/api/v1/me', {'code': 'account_disabled'}, 403), 0);
    expect(await rechecks('/api/v1/users', {'code': 'last_admin'}, 409), 0);
  });
}
```

`mobile/test/data/http/dev_trust_test.dart`:

```dart
import 'dart:io';
import 'dart:typed_data';

import 'package:dio/io.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/dev_trust.dart';
import 'package:postit/data/http/dio_factory.dart';

class _Bundle extends CachingAssetBundle {
  _Bundle(this.assets);
  final Map<String, List<int>> assets;

  @override
  Future<ByteData> load(String key) async {
    final bytes = assets[key];
    if (bytes == null) throw FlutterError('missing $key');
    return ByteData.sublistView(Uint8List.fromList(bytes));
  }
}

void main() {
  final dev = AppConfig.parse(apiBaseUrl: 'https://postit.local:44310', env: 'development');

  test('qa and production never load the dev CA', () async {
    for (final env in ['qa', 'production']) {
      final config = AppConfig.parse(apiBaseUrl: 'https://api.test', env: env);
      expect(await devTrust(config, bundle: _Bundle({devCaAsset: [1, 2, 3]})), isNull, reason: env);
    }
  });

  test('a development build without the asset falls back to the system roots', () async {
    expect(await devTrust(dev, bundle: _Bundle({})), isNull);
  });

  test('an unreadable certificate falls back to the system roots', () async {
    expect(await devTrust(dev, bundle: _Bundle({devCaAsset: 'not a certificate'.codeUnits})), isNull);
  });

  test('createDio uses the given trust and the default adapter otherwise', () {
    expect(createDio(Uri.parse('https://api.test'), trust: SecurityContext()).httpClientAdapter,
        isA<IOHttpClientAdapter>().having((a) => a.createHttpClient, 'createHttpClient', isNotNull));
    expect(createDio(Uri.parse('https://api.test')).httpClientAdapter,
        isA<IOHttpClientAdapter>().having((a) => a.createHttpClient, 'createHttpClient', isNull));
  });
}
```

Task 5's GET /me step is the positive proof that a real dev CA is trusted; no committed test holds certificate material.

Run: `flutter test test/data/http`
Expected: FAIL, the imports don't exist.

- [ ] **Step 2: Implement**

`mobile/lib/data/http/dio_factory.dart`:

```dart
import 'dart:io';

import 'package:dio/dio.dart';
import 'package:dio/io.dart';

/// A dio for the postit API (or the IdP, for absolute URLs). problem+json decodes as JSON.
/// [trust] replaces the system roots for TLS (see `devTrust`); null keeps the default client.
Dio createDio(Uri baseUrl, {SecurityContext? trust}) {
  final dio = Dio(BaseOptions(
    baseUrl: baseUrl.toString(),
    connectTimeout: const Duration(seconds: 10),
    receiveTimeout: const Duration(seconds: 30),
    headers: {'Accept': 'application/json'},
  ));
  if (trust != null) {
    dio.httpClientAdapter = IOHttpClientAdapter(createHttpClient: () => HttpClient(context: trust));
  }
  return dio;
}
```

`mobile/lib/data/http/dev_trust.dart`:

```dart
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:postit/data/config/app_config.dart';

/// Bundled only in the development flavor (pubspec `flavors:`), copied in by Gradle.
const devCaAsset = 'assets/dev/postit-dev-ca.crt';

/// Dart's HttpClient ignores Android's network_security_config, so debug development builds
/// trust the postit dev CA here, on top of the system roots. Null everywhere else, and when
/// the asset is missing or unreadable, which means the system roots only.
Future<SecurityContext?> devTrust(AppConfig config, {AssetBundle? bundle}) async {
  if (!kDebugMode || config.env != AppEnv.development) return null;
  try {
    final pem = await (bundle ?? rootBundle).load(devCaAsset);
    return SecurityContext(withTrustedRoots: true)
      ..setTrustedCertificatesBytes(pem.buffer.asUint8List(pem.offsetInBytes, pem.lengthInBytes));
  } on Object {
    return null;
  }
}
```

`mobile/lib/data/http/auth_interceptor.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';

/// Adds the bearer token; on 401 asks for one refresh and retries the request once. An
/// unreachable identity provider fails the call as a connection error, keeping the session.
class AuthInterceptor extends Interceptor {
  AuthInterceptor(this._session, this._dio);

  final SessionManager _session;
  final Dio _dio;
  static const _retried = 'postit.retried';

  static DioException _unreachable(RequestOptions options) =>
      DioException(requestOptions: options, type: DioExceptionType.connectionError, error: const AuthUnavailable());

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) async {
    try {
      options.headers['Authorization'] = 'Bearer ${await _session.accessToken()}';
      handler.next(options);
    } on NotSignedIn catch (e) {
      handler.reject(DioException(requestOptions: options, error: e));
    } on AuthUnavailable {
      handler.reject(_unreachable(options));
    }
  }

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) async {
    if (err.response?.statusCode != 401) return handler.next(err);
    if (err.requestOptions.extra[_retried] == true) {
      await _session.expire();
      return handler.next(err);
    }
    try {
      await _session.refresh();
      final retry = err.requestOptions..extra[_retried] = true;
      handler.resolve(await _dio.fetch<dynamic>(retry));
    } on NotSignedIn {
      handler.next(err);
    } on AuthUnavailable {
      handler.next(_unreachable(err.requestOptions));
    } on DioException catch (e) {
      handler.next(e);
    }
  }
}
```

`mobile/lib/data/http/recheck_interceptor.dart`:

```dart
import 'package:dio/dio.dart';

/// A pending, disabled or forbidden answer means the user's status or role changed, so
/// `/me` is fetched again and the gate moves the user (a demoted admin loses Users).
class RecheckInterceptor extends Interceptor {
  RecheckInterceptor(this._onRecheck);

  final void Function() _onRecheck;
  static const _codes = {'account_pending', 'account_disabled', 'forbidden'};

  @override
  void onError(DioException err, ErrorInterceptorHandler handler) {
    final data = err.response?.data;
    if (!err.requestOptions.path.endsWith('/api/v1/me') && data is Map && _codes.contains(data['code'])) {
      _onRecheck();
    }
    handler.next(err);
  }
}
```

- [ ] **Step 3: Run the tests**

Run: `flutter test test/data/http` then `flutter analyze`
Expected: PASS, no issues.

- [ ] **Step 4: Commit**

```bash
git add mobile/lib/data/http mobile/test/data/http
git commit -m "feat(app): bearer interceptor with one shared refresh, and /me recheck

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Repositories and preferences

**Files:**
- Create: `mobile/lib/data/repositories/auth_config_repository.dart`, `me_repository.dart`, `users_repository.dart`, `preferences_repository.dart`
- Test: `mobile/test/data/repositories/repositories_test.dart`, `mobile/test/data/repositories/preferences_repository_test.dart`

**Interfaces:**
- Consumes: the generated `AuthApi`, `MeApi`, `UsersApi`, and models (Task 6), plus `apiCall` (Task 7).
- Produces:
  - `AuthConfigRepository(AuthApi).load() -> Future<AuthConfigDto>`.
  - `MeRepository(MeApi)`: `get() -> Future<MeDto>`, `delete(String displayName) -> Future<void>`.
  - `UsersRepository(UsersApi)`:
    - `list({StatusDto? status, RoleDto? role, String? search, required int page, required int pageSize}) -> Future<PageUserDto>`
    - `get(String id) -> Future<UserDto>`
    - `approve(String id)`, `enable(String id)`, `disable(String id)`, `setRole(String id, RoleDto role)`, each `-> Future<UserDto>`
    - `delete(String id) -> Future<void>`
  - `PreferencesRepository`: `static Future<PreferencesRepository> open()`, the `themeMode` getter, `setThemeMode(ThemeMode)`. The storage key is `postit.theme`.

- [ ] **Step 1: Write the failing tests**

`mobile/test/data/repositories/repositories_test.dart`:

```dart
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/data/repositories/users_repository.dart';

import '../../helpers/scripted_adapter.dart';

const _user = <String, Object?>{
  'id': 'u1',
  'display_name': 'Ada',
  'email': 'ada@postit.test',
  'email_verified': true,
  'role': 'member',
  'status': 'pending',
  'created_at': '2026-10-16T10:00:00Z',
  'updated_at': '2026-10-16T10:00:00Z',
};

void main() {
  late ScriptedAdapter adapter;
  Dio dio(Object? Function(RequestOptions o) body, {int status = 200}) {
    adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(status, body(o)));
    return Dio(BaseOptions(baseUrl: 'https://api.test'))..httpClientAdapter = adapter;
  }

  test('list sends filters and paging as query parameters', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {'data': [_user], 'total': 1, 'page': 2, 'page_size': 25})));
    final page = await repo.list(status: StatusDto.pending, search: 'ada', page: 2, pageSize: 25);
    final q = adapter.requests.single.uri.queryParameters;
    expect(q['status'], 'pending');
    expect(q['search'], 'ada');
    expect(q['page'], '2');
    expect(q['page_size'], '25');
    expect(page.data.single.displayName, 'Ada');
  });

  test('an empty search is left out', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {'data': <Object>[], 'total': 0, 'page': 1, 'page_size': 25})));
    await repo.list(search: '', page: 1, pageSize: 25);
    expect(adapter.requests.single.uri.queryParameters.containsKey('search'), isFalse);
  });

  test('approve patches the status to active', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {..._user, 'status': 'active'})));
    final user = await repo.approve('u1');
    final req = adapter.requests.single;
    expect(req.method, 'PATCH');
    expect(req.uri.path, '/api/v1/users/u1');
    expect((req.data as Map)['status'], 'active');
    expect((req.data as Map)['role'], isNull);
    expect(user.status, StatusDto.active);
  });

  test('setRole patches only the role', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {..._user, 'role': 'admin'})));
    await repo.setRole('u1', RoleDto.admin);
    expect((adapter.requests.single.data as Map)['role'], 'admin');
    expect((adapter.requests.single.data as Map)['status'], isNull);
  });

  test('problems become ApiError', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {'code': 'last_admin', 'status': 409}, status: 409)));
    await expectLater(
      repo.disable('u1'),
      throwsA(isA<ApiError>().having((e) => e.code, 'code', 'last_admin')),
    );
  });

  test('delete me sends the typed display name', () async {
    final repo = MeRepository(MeApi(dio((_) => null, status: 202)));
    await repo.delete('Ada');
    final req = adapter.requests.single;
    expect(req.method, 'DELETE');
    expect(req.uri.path, '/api/v1/me');
    expect(req.data, isA<Map<String, dynamic>>().having((m) => m['display_name'], 'display_name', 'Ada'));
  });
}
```

`mobile/test/data/repositories/preferences_repository_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  test('defaults to system', () async {
    SharedPreferences.setMockInitialValues({});
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.system);
  });

  test('round-trips the choice under postit.theme', () async {
    SharedPreferences.setMockInitialValues({});
    final repo = await PreferencesRepository.open();
    await repo.setThemeMode(ThemeMode.dark);
    expect((await SharedPreferences.getInstance()).getString('postit.theme'), 'dark');
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.dark);
  });

  test('an unknown stored value falls back to system', () async {
    SharedPreferences.setMockInitialValues({'postit.theme': 'sepia'});
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.system);
  });

  test('without storage the choice is system and saving does nothing', () async {
    final repo = PreferencesRepository(null);
    expect(repo.themeMode, ThemeMode.system);
    await repo.setThemeMode(ThemeMode.light);
    expect(repo.themeMode, ThemeMode.system);
  });
}
```

Run: `flutter test test/data/repositories`
Expected: FAIL, the imports don't exist.

- [ ] **Step 2: Implement**

`auth_config_repository.dart`:

```dart
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

class AuthConfigRepository {
  AuthConfigRepository(this._api);
  final AuthApi _api;

  Future<AuthConfigDto> load() => apiCall(_api.authConfig);
}
```

`me_repository.dart`:

```dart
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

class MeRepository {
  MeRepository(this._api);
  final MeApi _api;

  Future<MeDto> get() => apiCall(_api.getMe);

  /// Deletes the caller's account; [displayName] must repeat the current display name.
  Future<void> delete(String displayName) =>
      apiCall(() => _api.deleteMe(body: DeleteMeRequest(displayName: displayName)));
}
```

`users_repository.dart`:

```dart
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_call.dart';

/// Admin user management (`/api/v1/users`).
class UsersRepository {
  UsersRepository(this._api);
  final UsersApi _api;

  Future<PageUserDto> list({
    StatusDto? status,
    RoleDto? role,
    String? search,
    required int page,
    required int pageSize,
  }) =>
      apiCall(() => _api.listUsers(
            status: status,
            role: role,
            search: (search == null || search.isEmpty) ? null : search,
            page: page,
            pageSize: pageSize,
          ));

  Future<UserDto> get(String id) => apiCall(() => _api.getUser(id: id));

  /// A pending user becomes active.
  Future<UserDto> approve(String id) => _patch(id, const PatchUserRequest(status: StatusDto.active));
  Future<UserDto> enable(String id) => _patch(id, const PatchUserRequest(status: StatusDto.active));
  Future<UserDto> disable(String id) => _patch(id, const PatchUserRequest(status: StatusDto.disabled));
  Future<UserDto> setRole(String id, RoleDto role) => _patch(id, PatchUserRequest(role: role));

  /// Deletes a user, or rejects a pending one.
  Future<void> delete(String id) => apiCall(() => _api.deleteUser(id: id));

  Future<UserDto> _patch(String id, PatchUserRequest body) => apiCall(() => _api.updateUser(id: id, body: body));
}
```

`preferences_repository.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Device-local preferences. Every read and write tolerates storage being unavailable.
class PreferencesRepository {
  PreferencesRepository(this._prefs);

  final SharedPreferences? _prefs;
  static const _themeKey = 'postit.theme';

  static Future<PreferencesRepository> open() async {
    try {
      return PreferencesRepository(await SharedPreferences.getInstance());
    } on Object {
      return PreferencesRepository(null);
    }
  }

  ThemeMode get themeMode {
    try {
      return switch (_prefs?.getString(_themeKey)) {
        'light' => ThemeMode.light,
        'dark' => ThemeMode.dark,
        _ => ThemeMode.system,
      };
    } on Object {
      return ThemeMode.system;
    }
  }

  Future<void> setThemeMode(ThemeMode mode) async {
    try {
      await _prefs?.setString(_themeKey, mode.name);
    } on Object {
      // Storage unavailable: the choice lasts until the app closes.
    }
  }
}
```

- [ ] **Step 3: Run the tests**

Run: `flutter test test/data/repositories` then `flutter analyze`
Expected: PASS. If the `PATCH` body carries `"role": null`, keep it: the server reads `null` as absent, and the test allows either.

- [ ] **Step 4: Commit**

```bash
git add mobile/lib/data/repositories mobile/test/data/repositories
git commit -m "feat(app): repositories for auth config, me, users and preferences

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Providers and the gate

**Files:**
- Create: `mobile/lib/state/providers.dart`, `mobile/lib/state/gate.dart`, `mobile/lib/state/users.dart`, `mobile/lib/state/theme.dart`
- Test: `mobile/test/state/gate_test.dart`, `mobile/test/state/client_id_test.dart`, `mobile/test/state/users_list_test.dart`

**Interfaces:**
- Consumes: everything from Tasks 7–10.
- Produces:
  - **Overridable providers:** `appConfigProvider`, `preferencesProvider`, `trustProvider` (`SecurityContext?`, default null; `main()` sets it from `devTrust`), `platformProvider` (a `MobilePlatform`), `authClientFactoryProvider`, `sessionStoreProvider`, `clockProvider`.
  - **Session:** `sessionManagerProvider` (`FutureProvider<SessionManager?>`), `sessionStateProvider`.
  - **API:** `apiDioProvider`, plus `meRepositoryProvider` and `usersRepositoryProvider`.
  - **Data:** `meProvider` (`FutureProvider<MeDto>`), `pendingCountProvider` and `activeAdminCountProvider` (`FutureProvider<int>`), `userProvider` (a family by ID), `usersListProvider` (a family by `UsersFilter`).
  - **Theme:** `themeModeProvider` (`NotifierProvider<ThemeModeNotifier, ThemeMode>`).
  - **Gate:** `GateTarget` (`startup`, `error`, `notConfigured`, `signIn`, `pending`, `unavailable`, `shell`, each with `path`), `gateTarget(...)`, `redirectFor(...)`, and `String? clientIdFor(AuthConfigDto, MobilePlatform)`.
  - **Users list:** `UsersFilter = ({StatusDto? status, String search})`, `UsersPage`, and `UsersListNotifier.loadMore()`.

- [ ] **Step 1: Write the failing gate tests** (`mobile/test/state/gate_test.dart`):

```dart
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/gate.dart';

import '../helpers/fake_users.dart';
import '../helpers/fakes.dart';

void main() {
  final manager = SessionManager(client: FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  const signedIn = SessionState.signedIn();

  GateTarget target({
    AsyncValue<SessionManager?>? startup,
    SessionState session = signedIn,
    AsyncValue<MeDto>? meValue,
  }) =>
      gateTarget(
        startup: startup ?? AsyncData(manager),
        session: session,
        me: meValue ?? AsyncData(fakeMe(StatusDto.active)),
      );

  test('startup states', () {
    expect(target(startup: const AsyncLoading()), GateTarget.startup);
    expect(target(startup: AsyncError(const ApiError(0, 'network'), StackTrace.empty)), GateTarget.error);
    expect(target(startup: const AsyncData(null)), GateTarget.notConfigured);
  });

  test('session states', () {
    expect(target(session: const SessionState.loading()), GateTarget.startup);
    expect(target(session: const SessionState.signedOut()), GateTarget.signIn);
  });

  test('me states', () {
    expect(target(meValue: const AsyncLoading()), GateTarget.startup);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.pending))), GateTarget.pending);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.disabled))), GateTarget.unavailable);
    expect(target(meValue: AsyncData(fakeMe(StatusDto.deleting))), GateTarget.unavailable);
    expect(target(), GateTarget.shell);
    expect(target(meValue: AsyncError(const ApiError(403, 'account_disabled'), StackTrace.empty)), GateTarget.unavailable);
    expect(target(meValue: AsyncError(const ApiError(403, 'account_pending'), StackTrace.empty)), GateTarget.pending);
    expect(target(meValue: AsyncError(const ApiError(0, 'network'), StackTrace.empty)), GateTarget.error);
  });

  test('redirects', () {
    expect(redirectFor(GateTarget.signIn, '/home', isAdmin: false), '/sign-in');
    expect(redirectFor(GateTarget.signIn, '/sign-in', isAdmin: false), isNull);
    expect(redirectFor(GateTarget.shell, '/sign-in', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/profile', isAdmin: false), isNull);
    expect(redirectFor(GateTarget.shell, '/users', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/users/u2', isAdmin: false), '/home');
    expect(redirectFor(GateTarget.shell, '/users/u2', isAdmin: true), isNull);
  });
}
```

`mobile/test/state/client_id_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

void main() {
  final config = AuthConfigDto.fromJson({
    'issuer': 'https://idp.test',
    'client_id': 'web',
    'android_client_id': 'android',
    'ios_client_id': null,
    'scopes': ['openid'],
  });

  test('each platform reads its own client id, never the web one', () {
    expect(clientIdFor(config, MobilePlatform.android), 'android');
    expect(clientIdFor(config, MobilePlatform.ios), isNull);
  });
}
```

`mobile/test/state/users_list_test.dart`:

```dart
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';

import '../helpers/fake_users.dart';

void main() {
  test('loadMore appends the next page until total is reached', () async {
    final repo = FakeUsersRepository(List.generate(30, (i) => fakeUser('u$i')));
    final container = ProviderContainer(
      overrides: [usersRepositoryProvider.overrideWithValue(repo)],
      retry: (retryCount, error) => null,
    );
    addTearDown(container.dispose);
    const UsersFilter filter = (status: null, search: '');
    final first = await container.read(usersListProvider(filter).future);
    expect(first.items, hasLength(UsersListNotifier.pageSize));
    expect(first.hasMore, isTrue);
    await container.read(usersListProvider(filter).notifier).loadMore();
    final all = container.read(usersListProvider(filter)).value!;
    expect(all.items, hasLength(30));
    expect(all.hasMore, isFalse);
    expect(repo.pagesRequested, [1, 2]);
  });
}
```

Create `mobile/test/helpers/fake_users.dart` (used here, by the gate test, and by the screen tests). Create it before running Step 1's tests:

```dart
import 'package:postit/api/export.dart';
import 'package:postit/data/repositories/users_repository.dart';

/// The signed-in user as `GET /me` returns it: Ada, member unless [role] says otherwise.
MeDto fakeMe(StatusDto status, {RoleDto role = RoleDto.member}) => MeDto.fromJson({
      'id': 'u1',
      'display_name': 'Ada',
      'email_verified': true,
      'role': role.toJson(),
      'status': status.toJson(),
      'created_at': '2026-10-16T10:00:00Z',
      'updated_at': '2026-10-16T10:00:00Z',
    });

UserDto fakeUser(String id, {StatusDto status = StatusDto.active, RoleDto role = RoleDto.member, String? name}) =>
    UserDto.fromJson({
      'id': id,
      'display_name': name ?? 'User $id',
      'email': '$id@postit.test',
      'email_verified': true,
      'role': role.toJson(),
      'status': status.toJson(),
      'created_at': '2026-10-16T10:00:00Z',
      'updated_at': '2026-10-16T10:00:00Z',
    });

/// In-memory admin user list; records the calls the screens make.
class FakeUsersRepository implements UsersRepository {
  FakeUsersRepository(this.users);

  final List<UserDto> users;
  final pagesRequested = <int>[];
  final calls = <String>[];
  Object? failWith;

  @override
  Future<PageUserDto> list({StatusDto? status, RoleDto? role, String? search, required int page, required int pageSize}) async {
    pagesRequested.add(page);
    final matching = users
        .where((u) => status == null || u.status == status)
        .where((u) => role == null || u.role == role)
        .where((u) => search == null || search.isEmpty || u.displayName.toLowerCase().contains(search.toLowerCase()))
        .toList();
    final start = (page - 1) * pageSize;
    final slice = matching.skip(start).take(pageSize).toList();
    return PageUserDto.fromJson({
      'data': [for (final u in slice) u.toJson()],
      'total': matching.length,
      'page': page,
      'page_size': pageSize,
    });
  }

  @override
  Future<UserDto> get(String id) async => users.firstWhere((u) => u.id == id);

  Future<UserDto> _record(String call, String id, UserDto Function(UserDto) change) async {
    calls.add('$call $id');
    final error = failWith;
    if (error != null) throw error;
    final i = users.indexWhere((u) => u.id == id);
    users[i] = change(users[i]);
    return users[i];
  }

  UserDto _with(UserDto u, {StatusDto? status, RoleDto? role}) =>
      UserDto.fromJson({...u.toJson(), if (status != null) 'status': status.toJson(), if (role != null) 'role': role.toJson()});

  @override
  Future<UserDto> approve(String id) => _record('approve', id, (u) => _with(u, status: StatusDto.active));
  @override
  Future<UserDto> enable(String id) => _record('enable', id, (u) => _with(u, status: StatusDto.active));
  @override
  Future<UserDto> disable(String id) => _record('disable', id, (u) => _with(u, status: StatusDto.disabled));
  @override
  Future<UserDto> setRole(String id, RoleDto role) => _record('role:${role.toJson()}', id, (u) => _with(u, role: role));

  @override
  Future<void> delete(String id) async {
    await _record('delete', id, (u) => u);
    users.removeWhere((u) => u.id == id);
  }
}
```

If the generated enums have no `toJson()`, use their JSON string through the generated `$StatusDtoEnumMap`, or switch on the enum. The contract test in Task 6 shows the available API.

Run: `flutter test test/state`
Expected: FAIL, the imports don't exist.

- [ ] **Step 2: Implement `mobile/lib/state/gate.dart`**

```dart
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/api_error.dart';

/// The screen the app must show; [path] is null for the signed-in shell.
enum GateTarget {
  startup('/startup'),
  error('/error'),
  notConfigured('/not-configured'),
  signIn('/sign-in'),
  pending('/pending'),
  unavailable('/unavailable'),
  shell(null);

  const GateTarget(this.path);
  final String? path;
}

/// Same rules as the web app's gate: startup, then session, then `/me`.
GateTarget gateTarget({
  required AsyncValue<SessionManager?> startup,
  required SessionState session,
  required AsyncValue<MeDto> me,
}) {
  if (startup.isLoading) return GateTarget.startup;
  if (startup.hasError) return GateTarget.error;
  if (startup.value == null) return GateTarget.notConfigured;
  switch (session.status) {
    case SessionStatus.loading:
      return GateTarget.startup;
    case SessionStatus.signedOut:
      return GateTarget.signIn;
    case SessionStatus.signedIn:
      break;
  }
  if (me.isLoading && !me.hasValue) return GateTarget.startup;
  if (me.hasError && !me.isLoading) {
    final error = me.error;
    final code = error is ApiError ? error.code : '';
    return switch (code) {
      'account_disabled' => GateTarget.unavailable,
      'account_pending' => GateTarget.pending,
      _ => GateTarget.error,
    };
  }
  final user = me.value;
  if (user == null) return GateTarget.startup;
  return switch (user.status) {
    StatusDto.pending => GateTarget.pending,
    StatusDto.active => GateTarget.shell,
    _ => GateTarget.unavailable,
  };
}

/// go_router redirect for [target] at [location].
String? redirectFor(GateTarget target, String location, {required bool isAdmin}) {
  final path = target.path;
  if (path != null) return location == path ? null : path;
  final onGateScreen = location == '/' || GateTarget.values.any((t) => t.path == location);
  if (onGateScreen) return '/home';
  if (location.startsWith('/users') && !isAdmin) return '/home';
  return null;
}
```

If the generated `StatusDto` has an extra unknown-value member (some generators add `$unknown`), the `_` arm already maps it to `unavailable`.

- [ ] **Step 3: Implement `mobile/lib/state/providers.dart`**

```dart
import 'dart:io' show Platform, SecurityContext;

import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/app_auth_client.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/auth/session_store.dart';
import 'package:postit/data/auth/token_revoker.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/auth_interceptor.dart';
import 'package:postit/data/http/dio_factory.dart';
import 'package:postit/data/http/recheck_interceptor.dart';
import 'package:postit/data/repositories/auth_config_repository.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/data/repositories/users_repository.dart';

enum MobilePlatform { android, ios }

/// Overridden in `main()` with the dart-define values.
final appConfigProvider = Provider<AppConfig>((ref) => throw StateError('appConfigProvider is set in main()'));

/// Overridden in `main()` once shared preferences are open.
final preferencesProvider =
    Provider<PreferencesRepository>((ref) => throw StateError('preferencesProvider is set in main()'));

/// Extra TLS trust for dio: the dev CA in debug development builds (`devTrust`), else null.
final trustProvider = Provider<SecurityContext?>((ref) => null);

final platformProvider =
    Provider<MobilePlatform>((ref) => Platform.isIOS ? MobilePlatform.ios : MobilePlatform.android);
final authClientFactoryProvider = Provider<AuthClient Function(OidcClientConfig)>((ref) => AppAuthClient.new);
final sessionStoreProvider = Provider<SessionStore>((ref) => SecureSessionStore());
final clockProvider = Provider<DateTime Function()>((ref) => DateTime.now);

/// This platform's client ID; the web `client_id` is never used by the app.
String? clientIdFor(AuthConfigDto config, MobilePlatform platform) => switch (platform) {
      MobilePlatform.android => config.androidClientId,
      MobilePlatform.ios => config.iosClientId,
    };

final _publicDioProvider =
    Provider<Dio>((ref) => createDio(ref.watch(appConfigProvider).apiBaseUrl, trust: ref.watch(trustProvider)));

/// The session for this platform's client, restored from storage; null when the deployment
/// has no client for this platform.
final sessionManagerProvider = FutureProvider<SessionManager?>((ref) async {
  final config = ref.watch(appConfigProvider);
  final publicDio = ref.watch(_publicDioProvider);
  final auth = await AuthConfigRepository(AuthApi(publicDio)).load();
  final clientId = clientIdFor(auth, ref.watch(platformProvider));
  if (clientId == null) return null;
  final oidc = OidcClientConfig(
    issuer: auth.issuer,
    clientId: clientId,
    scopes: auth.scopes,
    redirectUrl: config.redirectUrl,
    postLogoutRedirectUrl: config.postLogoutRedirectUrl,
  );
  final manager = SessionManager(
    client: ref.watch(authClientFactoryProvider)(oidc),
    store: ref.watch(sessionStoreProvider),
    revoke: TokenRevoker(publicDio, oidc).revoke,
  );
  ref.onDispose(manager.dispose);
  await manager.restore();
  return manager;
});

final sessionStateProvider = Provider<SessionState>((ref) {
  final manager = ref.watch(sessionManagerProvider).value;
  if (manager == null) return const SessionState.loading();
  void changed() => ref.invalidateSelf();
  manager.state.addListener(changed);
  ref.onDispose(() => manager.state.removeListener(changed));
  return manager.state.value;
});

/// Only read while signed in, when the session exists.
final apiDioProvider = Provider<Dio>((ref) {
  final manager = ref.watch(sessionManagerProvider).requireValue;
  if (manager == null) throw const NotSignedIn();
  final dio = createDio(ref.watch(appConfigProvider).apiBaseUrl, trust: ref.watch(trustProvider));
  dio.interceptors
    ..add(AuthInterceptor(manager, dio))
    ..add(RecheckInterceptor(() => ref.invalidate(meProvider)));
  return dio;
});

final meRepositoryProvider = Provider<MeRepository>((ref) => MeRepository(MeApi(ref.watch(apiDioProvider))));
final usersRepositoryProvider =
    Provider<UsersRepository>((ref) => UsersRepository(UsersApi(ref.watch(apiDioProvider))));

final meProvider = FutureProvider<MeDto>((ref) async {
  final status = ref.watch(sessionStateProvider.select((s) => s.status));
  if (status != SessionStatus.signedIn) throw const NotSignedIn();
  return ref.watch(meRepositoryProvider).get();
});

final pendingCountProvider = FutureProvider.autoDispose<int>((ref) async {
  final page = await ref.watch(usersRepositoryProvider).list(status: StatusDto.pending, page: 1, pageSize: 1);
  return page.total;
});

final activeAdminCountProvider = FutureProvider.autoDispose<int>((ref) async {
  final page = await ref
      .watch(usersRepositoryProvider)
      .list(status: StatusDto.active, role: RoleDto.admin, page: 1, pageSize: 1);
  return page.total;
});

final userProvider =
    FutureProvider.autoDispose.family<UserDto, String>((ref, id) => ref.watch(usersRepositoryProvider).get(id));
```

- [ ] **Step 4: Implement `mobile/lib/state/users.dart`**

```dart
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

typedef UsersFilter = ({StatusDto? status, String search});

class UsersPage {
  const UsersPage(this.items, this.total, this.page);
  final List<UserDto> items;
  final int total;
  final int page;

  bool get hasMore => items.length < total;
}

class UsersListNotifier extends AsyncNotifier<UsersPage> {
  UsersListNotifier(this.filter);

  final UsersFilter filter;
  static const pageSize = 25;
  bool _loadingMore = false;

  @override
  Future<UsersPage> build() async {
    final page = await ref
        .watch(usersRepositoryProvider)
        .list(status: filter.status, search: filter.search, page: 1, pageSize: pageSize);
    return UsersPage(page.data, page.total, 1);
  }

  /// Appends the next page; a no-op while loading or at the end.
  Future<void> loadMore() async {
    final current = state.value;
    if (current == null || !current.hasMore || _loadingMore || state.isLoading) return;
    _loadingMore = true;
    try {
      final next = await ref
          .read(usersRepositoryProvider)
          .list(status: filter.status, search: filter.search, page: current.page + 1, pageSize: pageSize);
      state = AsyncData(UsersPage([...current.items, ...next.data], next.total, current.page + 1));
    } on Object catch (e, st) {
      state = AsyncError(e, st);
    } finally {
      _loadingMore = false;
    }
  }
}

final usersListProvider =
    AsyncNotifierProvider.autoDispose.family<UsersListNotifier, UsersPage, UsersFilter>(UsersListNotifier.new);
```

- [ ] **Step 5: Implement `mobile/lib/state/theme.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';

class ThemeModeNotifier extends Notifier<ThemeMode> {
  @override
  ThemeMode build() => ref.watch(preferencesProvider).themeMode;

  Future<void> set(ThemeMode mode) async {
    state = mode;
    await ref.read(preferencesProvider).setThemeMode(mode);
  }
}

final themeModeProvider = NotifierProvider<ThemeModeNotifier, ThemeMode>(ThemeModeNotifier.new);
```

- [ ] **Step 6: Run the tests**

Run: `flutter test test/state` then `flutter analyze`
Expected: PASS. If the installed Riverpod 3 release spells the family-notifier or `retry` APIs differently, follow its migration guide and keep the behavior.

- [ ] **Step 7: Commit**

```bash
git add mobile/lib/state mobile/test/state mobile/test/helpers/fake_users.dart
git commit -m "feat(app): providers, the sign-in gate, and the paged users list

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Theme: tokens, fonts, logo, launcher icons

**Files:**
- Create: `mobile/lib/ui/theme/tokens.dart`, `mobile/lib/ui/theme/app_theme.dart`, `mobile/lib/ui/widgets/logo.dart`
- Create: `mobile/assets/fonts/*.ttf`, `mobile/assets/fonts/OFL.txt`, `mobile/assets/fonts/SOURCES.md`, `mobile/assets/logo/*`
- Modify: `mobile/pubspec.yaml` (`flutter:` assets, fonts, and the `flutter_launcher_icons` config)
- Test: `mobile/test/ui/theme/contrast_test.dart`, `mobile/test/ui/theme/reduced_motion_test.dart`

**Interfaces:**
- Produces:
  - `PostitTokens` (a `ThemeExtension`, with `light` and `dark` instances).
  - `ThemeData buildTheme(Brightness)`.
  - `TextStyle weighted(TextStyle base, int weight)`.
  - The widgets `PostitWordmark({double height})` and `PostitBadge({double size})`.
  - Font families `BricolageGrotesque`, `Onest`, `JetBrainsMono`.

- [ ] **Step 1: Write the failing contrast test** (`mobile/test/ui/theme/contrast_test.dart`):

```dart
import 'dart:math';
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:postit/ui/theme/tokens.dart';

double contrast(Color a, Color b) {
  final la = a.computeLuminance();
  final lb = b.computeLuminance();
  return (max(la, lb) + 0.05) / (min(la, lb) + 0.05);
}

void main() {
  for (final (name, t) in [('light', PostitTokens.light), ('dark', PostitTokens.dark)]) {
    test('$name text meets WCAG AA', () {
      for (final bg in [t.bg, t.surface, t.raised]) {
        expect(contrast(t.text, bg), greaterThanOrEqualTo(4.5));
        expect(contrast(t.mutedText, bg), greaterThanOrEqualTo(4.5));
        expect(contrast(t.pinkText, bg), greaterThanOrEqualTo(4.5));
      }
      expect(contrast(t.onPink, t.pink), greaterThanOrEqualTo(4.5));
      expect(contrast(t.warnText, t.warnBg), greaterThanOrEqualTo(4.5));
    });

    test('$name focus indicator reaches 3:1', () {
      for (final bg in [t.bg, t.surface, t.raised]) {
        expect(contrast(t.pinkText, bg), greaterThanOrEqualTo(3));
      }
    });
  }
}
```

`mobile/test/ui/theme/reduced_motion_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/ui/theme/app_theme.dart';

void main() {
  // MaterialApp reads MediaQuery from the view, so the OS setting is faked on the dispatcher.
  Future<void> pushSecond(WidgetTester tester, {required bool reduce}) async {
    tester.platformDispatcher.accessibilityFeaturesTestValue = FakeAccessibilityFeatures(disableAnimations: reduce);
    addTearDown(tester.platformDispatcher.clearAccessibilityFeaturesTestValue);
    final nav = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      MaterialApp(navigatorKey: nav, theme: buildTheme(Brightness.light), home: const Text('first')),
    );
    nav.currentState!.push(MaterialPageRoute<void>(builder: (_) => const Text('second')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));
  }

  testWidgets('reduced motion shows the next page without a transition', (tester) async {
    await pushSecond(tester, reduce: true);
    // The route's controller still ticks; what matters is that nothing on screen moves.
    final second = tester.getTopLeft(find.text('second'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(tester.getTopLeft(find.text('second')), second);
  });

  testWidgets('otherwise the platform transition runs', (tester) async {
    await pushSecond(tester, reduce: false);
    expect(tester.hasRunningAnimations, isTrue);
  });
}
```

The reduced-motion test compares positions rather than `hasRunningAnimations`, because the route's own controller still runs; if the 3.47 default transition fades without moving, compare the nearest `FadeTransition`'s opacity value instead.

Run: `flutter test test/ui/theme`
Expected: FAIL, the imports don't exist.

- [ ] **Step 2: Implement `tokens.dart`.** The values are copied from `web/src/index.css:72-147`.

```dart
import 'package:flutter/material.dart';

/// The P7.1 design tokens (`web/src/index.css`), for colors Material has no slot for.
@immutable
class PostitTokens extends ThemeExtension<PostitTokens> {
  const PostitTokens({
    required this.bg,
    required this.surface,
    required this.raised,
    required this.line,
    required this.text,
    required this.mutedText,
    required this.faint,
    required this.inkLogo,
    required this.pink,
    required this.pinkText,
    required this.pinkSoft,
    required this.onPink,
    required this.warnBg,
    required this.warnLine,
    required this.warnText,
    required this.warnIcon,
    required this.track,
    required this.disabled,
    required this.danger,
  });

  final Color bg, surface, raised, line, text, mutedText, faint, inkLogo;
  final Color pink, pinkText, pinkSoft, onPink;
  final Color warnBg, warnLine, warnText, warnIcon, track, disabled, danger;

  static const light = PostitTokens(
    bg: Color(0xFFF6F3F1),
    surface: Color(0xFFFFFFFF),
    raised: Color(0xFFF1ECE9),
    line: Color(0xFFE5DDD9),
    text: Color(0xFF1D1A1C),
    mutedText: Color(0xFF5F565B),
    faint: Color(0xFF6F666B),
    inkLogo: Color(0xFF1D1A1C),
    pink: Color(0xFFE0679F),
    pinkText: Color(0xFFA8336B),
    pinkSoft: Color.fromRGBO(224, 103, 159, 0.12),
    onPink: Color(0xFF1D1A1C),
    warnBg: Color(0xFFFDF3E1),
    warnLine: Color(0xFFECD29F),
    warnText: Color(0xFF5F420F),
    warnIcon: Color(0xFFB7791F),
    track: Color(0xFFECE5E1),
    disabled: Color(0xFFB9B0B4),
    danger: Color(0xFFB3261E),
  );

  static const dark = PostitTokens(
    bg: Color(0xFF121012),
    surface: Color(0xFF1B181A),
    raised: Color(0xFF242022),
    line: Color(0xFF2F2A2D),
    text: Color(0xFFF4EFF1),
    mutedText: Color(0xFFAEA4A9),
    faint: Color(0xFF948B90),
    inkLogo: Color(0xFFF4EFF1),
    pink: Color(0xFFE0679F),
    pinkText: Color(0xFFEC8BB7),
    pinkSoft: Color.fromRGBO(224, 103, 159, 0.14),
    onPink: Color(0xFF1D1A1C),
    warnBg: Color(0xFF2A2112),
    warnLine: Color(0xFF5C4518),
    warnText: Color(0xFFF2D9A8),
    warnIcon: Color(0xFFE8B04A),
    track: Color(0xFF2F2A2D),
    disabled: Color(0xFF6D6468),
    danger: Color(0xFFF2827A),
  );

  @override
  PostitTokens copyWith() => this;

  /// Themes switch instantly (as on the web), so there is nothing to interpolate.
  @override
  PostitTokens lerp(ThemeExtension<PostitTokens>? other, double t) =>
      other is PostitTokens && t >= 0.5 ? other : this;
}

extension PostitTokensX on BuildContext {
  PostitTokens get tokens => Theme.of(this).extension<PostitTokens>() ?? PostitTokens.light;
}
```

- [ ] **Step 3: Implement `app_theme.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:postit/ui/theme/tokens.dart';

/// The platform's page transition, or none when the OS asks for reduced motion.
class ReducedMotionTransitions extends PageTransitionsBuilder {
  const ReducedMotionTransitions();

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) =>
      MediaQuery.disableAnimationsOf(context)
          ? child
          // A PageTransitionsTheme without builders applies the platform defaults.
          : const PageTransitionsTheme().buildTransitions(route, context, animation, secondaryAnimation, child);
}

/// Variable fonts need the weight axis set as well as [FontWeight].
TextStyle weighted(TextStyle base, int weight) => base.copyWith(
      fontWeight: FontWeight.values[(weight ~/ 100 - 1).clamp(0, 8)],
      fontVariations: [FontVariation.weight(weight.toDouble())],
    );

ThemeData buildTheme(Brightness brightness) {
  final t = brightness == Brightness.light ? PostitTokens.light : PostitTokens.dark;
  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.pink,
    onPrimary: t.onPink,
    secondary: t.pinkText,
    onSecondary: t.surface,
    error: t.danger,
    onError: t.surface,
    surface: t.surface,
    onSurface: t.text,
    onSurfaceVariant: t.mutedText,
    surfaceContainerLowest: t.surface,
    surfaceContainerLow: t.surface,
    surfaceContainer: t.raised,
    surfaceContainerHigh: t.raised,
    surfaceContainerHighest: t.raised,
    outline: t.line,
    outlineVariant: t.line,
  );
  final base = ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: t.bg,
    fontFamily: 'Onest',
    materialTapTargetSize: MaterialTapTargetSize.padded,
    extensions: [t],
  );
  TextStyle heading(TextStyle? s, int w) =>
      weighted((s ?? const TextStyle()).copyWith(fontFamily: 'BricolageGrotesque', color: t.text), w);
  TextStyle mono(TextStyle? s) =>
      weighted((s ?? const TextStyle()).copyWith(fontFamily: 'JetBrainsMono', color: t.mutedText), 500);
  final text = base.textTheme;
  return base.copyWith(
    textTheme: text.copyWith(
      displayLarge: heading(text.displayLarge, 700),
      displayMedium: heading(text.displayMedium, 700),
      displaySmall: heading(text.displaySmall, 700),
      headlineLarge: heading(text.headlineLarge, 700),
      headlineMedium: heading(text.headlineMedium, 650),
      headlineSmall: heading(text.headlineSmall, 650),
      titleLarge: heading(text.titleLarge, 600),
      labelSmall: mono(text.labelSmall),
    ),
    cardTheme: CardThemeData(
      color: t.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12), side: BorderSide(color: t.line)),
    ),
    dividerTheme: DividerThemeData(color: t.line),
    pageTransitionsTheme: PageTransitionsTheme(
      builders: {for (final p in TargetPlatform.values) p: const ReducedMotionTransitions()},
    ),
    focusColor: t.pinkSoft,
    navigationBarTheme: NavigationBarThemeData(backgroundColor: t.bg, indicatorColor: t.pinkSoft),
    navigationRailTheme: NavigationRailThemeData(backgroundColor: t.bg, indicatorColor: t.pinkSoft),
    inputDecorationTheme: InputDecorationTheme(
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(10)),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: t.pinkText, width: 2),
      ),
    ),
  );
}
```

- [ ] **Step 4: Add fonts and the logo.**
  - **Fonts.** Download the variable TTFs from the OFL directory of `google/fonts` at one pinned commit, and record that commit SHA and the three URLs in `mobile/assets/fonts/SOURCES.md`:
    - `ofl/bricolagegrotesque/BricolageGrotesque[opsz,wdth,wght].ttf` saved as `BricolageGrotesque-Variable.ttf`
    - `ofl/onest/Onest[wght].ttf` saved as `Onest-Variable.ttf`
    - `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` saved as `JetBrainsMono-Variable.ttf`
  - Copy `OFL.txt` from one of those directories.
  - **Logo.** Copy `assets/postit.svg`, `postit-f.svg`, `postit-sb.svg`, `postit-sp.svg`, and `postit-sb.png` into `mobile/assets/logo/`.
  - **`pubspec.yaml`:**

```yaml
flutter:
  uses-material-design: true
  assets:
    - assets/logo/
    # Kept from Task 4: the dev CA, development flavor only.
    - path: assets/dev/
      flavors:
        - development
  fonts:
    - family: BricolageGrotesque
      fonts:
        - asset: assets/fonts/BricolageGrotesque-Variable.ttf
    - family: Onest
      fonts:
        - asset: assets/fonts/Onest-Variable.ttf
    - family: JetBrainsMono
      fonts:
        - asset: assets/fonts/JetBrainsMono-Variable.ttf

flutter_launcher_icons:
  android: true
  ios: true
  image_path: assets/logo/postit-sb.png
  remove_alpha_ios: true
  # Android 8+ masks launcher icons; without these the badge is shrunk into a legacy icon.
  adaptive_icon_background: "#E0679F"
  adaptive_icon_foreground: assets/logo/postit-sb.png
```

Then run `dart run flutter_launcher_icons` from `mobile/`. Look at the installed icon on the emulator (round and squircle masks): the badge must not be clipped. If it is, add a padded foreground PNG (the badge at 66% of a transparent 432 px canvas) as `assets/logo/postit-adaptive-fg.png`, point `adaptive_icon_foreground` at it, and regenerate.

- [ ] **Step 5: Implement `logo.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:postit/ui/theme/tokens.dart';

/// The full wordmark, tinted with the theme's ink color.
class PostitWordmark extends StatelessWidget {
  const PostitWordmark({super.key, this.height = 28});
  final double height;

  @override
  Widget build(BuildContext context) => SvgPicture.asset(
        'assets/logo/postit-f.svg',
        height: height,
        colorFilter: ColorFilter.mode(context.tokens.inkLogo, BlendMode.srcIn),
        semanticsLabel: 'postit',
      );
}

/// The square pink badge.
class PostitBadge extends StatelessWidget {
  const PostitBadge({super.key, this.size = 56});
  final double size;

  @override
  Widget build(BuildContext context) =>
      SvgPicture.asset('assets/logo/postit-sp.svg', width: size, height: size, semanticsLabel: 'postit');
}
```

- [ ] **Step 6: Run the tests**

Run: `flutter test test/ui/theme` then `flutter analyze`
Expected: PASS. If a pair misses its ratio, the web tokens have the same problem: report it rather than altering colors silently.

- [ ] **Step 7: Commit**

```bash
git add mobile/lib/ui/theme mobile/lib/ui/widgets/logo.dart mobile/assets/fonts mobile/assets/logo mobile/pubspec.yaml mobile/pubspec.lock mobile/android/app/src/main/res mobile/ios/Runner/Assets.xcassets mobile/test/ui/theme
git commit -m "feat(app): postit design tokens, bundled fonts, logo and launcher icons

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: App root, router, and the status screens

**Files:**
- Create: `mobile/lib/ui/app.dart`, `mobile/lib/ui/router.dart`, `mobile/lib/ui/screens/status_screens.dart`, `mobile/lib/ui/screens/sign_in_screen.dart`
- Modify: `mobile/lib/main.dart`
- Test: `mobile/test/helpers/harness.dart`, `mobile/test/ui/screens/status_screens_test.dart`, `mobile/test/ui/screens/sign_in_screen_test.dart`

**Interfaces:**
- Consumes: the Task 11 providers, `buildTheme`, and `PostitWordmark`/`PostitBadge`.
- Produces:
  - The `PostitApp` widget and `routerProvider`.
  - Routes `/startup`, `/error`, `/not-configured`, `/sign-in`, `/pending`, `/unavailable`, and the shell routes `/home`, `/profile`, `/users`, `/users/:id` (filled in by Tasks 14 and 15).
  - Test helper `pumpApp(tester, {overrides, size})` and `signedInManager()`.

- [ ] **Step 1: Write the test harness** (`mobile/test/helpers/harness.dart`):

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/app.dart';

import 'fakes.dart';

final testConfig = AppConfig.parse(apiBaseUrl: 'https://api.test', env: 'development');

Future<SessionManager> signedInManager([FakeAuthClient? client]) async {
  final m = SessionManager(client: client ?? FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  await m.signIn();
  return m;
}

Future<SessionManager> signedOutManager([FakeAuthClient? client]) async {
  final m = SessionManager(client: client ?? FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  await m.restore();
  return m;
}

Future<void> pumpApp(
  WidgetTester tester, {
  required List<Override> overrides,
  Size size = const Size(400, 800),
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(ProviderScope(
    retry: (retryCount, error) => null,
    overrides: [
      appConfigProvider.overrideWithValue(testConfig),
      preferencesProvider.overrideWithValue(PreferencesRepository(null)),
      clockProvider.overrideWithValue(() => DateTime(2026, 10, 16, 9)),
      ...overrides,
    ],
    child: const PostitApp(),
  ));
  await tester.pumpAndSettle();
}
```

- [ ] **Step 2: Write the failing screen tests**

`mobile/test/ui/screens/status_screens_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/harness.dart';
import '../../helpers/fake_users.dart';

void main() {
  testWidgets('startup failure shows retry, and retry recovers', (tester) async {
    var attempts = 0;
    final manager = await signedOutManager();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async {
        attempts++;
        if (attempts == 1) throw const ApiError(0, 'network');
        return manager;
      }),
    ]);
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
    await tester.tap(find.text('Retry'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('an unreachable provider at startup shows retry', (tester) async {
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => throw const AuthUnavailable()),
    ]);
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
    expect(find.text('Retry'), findsOneWidget);
  });

  testWidgets('a platform without a client shows not configured', (tester) async {
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => null)]);
    expect(find.text('Mobile sign-in is not configured'), findsOneWidget);
  });

  testWidgets('pending shows refresh and sign-out', (tester) async {
    final manager = await signedInManager();
    var fetches = 0;
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async {
        fetches++;
        return fakeMe(StatusDto.pending);
      }),
    ]);
    expect(find.text('Waiting for approval'), findsOneWidget);
    await tester.tap(find.text('Check again'));
    await tester.pumpAndSettle();
    expect(fetches, 2);
    await tester.tap(find.text('Sign out'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('a disabled account shows unavailable', (tester) async {
    final manager = await signedInManager();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => throw const ApiError(403, 'account_disabled')),
    ]);
    expect(find.text('Account unavailable'), findsOneWidget);
  });
}
```

`mobile/test/ui/screens/sign_in_screen_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';

void main() {
  testWidgets('cancelled sign-in shows no message', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthCancelled();
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
    expect(find.byKey(const ValueKey('sign-in-error')), findsNothing);
  });

  testWidgets('a provider error is shown', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthFailed('Access denied.');
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text('Access denied.'), findsOneWidget);
  });

  testWidgets('an unreachable provider says so', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw const AuthUnavailable();
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.textContaining("Can't reach postit"), findsOneWidget);
  });

  testWidgets('an unexpected failure is not swallowed', (tester) async {
    final client = FakeAuthClient()..onSignIn = () async => throw StateError('boom');
    final manager = await signedOutManager(client);
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    await tester.tap(find.text('Sign in'));
    await tester.pumpAndSettle();
    expect(find.text('Something went wrong. Try again.'), findsOneWidget);
  });

  testWidgets('an expired session explains itself', (tester) async {
    final manager = await signedInManager();
    await manager.expire();
    await pumpApp(tester, overrides: [sessionManagerProvider.overrideWith((ref) async => manager)]);
    expect(find.text(sessionExpiredNotice), findsOneWidget);
  });
}
```

Add `import 'package:flutter/widgets.dart' show ValueKey;` to the second test file.

Run: `flutter test test/ui/screens`
Expected: FAIL, `package:postit/ui/app.dart` doesn't exist.

- [ ] **Step 3: Implement `status_screens.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/widgets/logo.dart';

/// Shared layout for the full-screen states before the shell.
class StatusLayout extends StatelessWidget {
  const StatusLayout({super.key, required this.title, required this.body, this.actions = const []});

  final String title;
  final String body;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) => Scaffold(
        body: SafeArea(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420),
              child: ListView(
                shrinkWrap: true,
                padding: const EdgeInsets.all(24),
                children: [
                  const Center(child: PostitBadge()),
                  const SizedBox(height: 24),
                  Text(title, style: Theme.of(context).textTheme.headlineSmall, textAlign: TextAlign.center),
                  const SizedBox(height: 12),
                  Text(body, textAlign: TextAlign.center),
                  const SizedBox(height: 24),
                  for (final a in actions) Padding(padding: const EdgeInsets.only(bottom: 12), child: a),
                ],
              ),
            ),
          ),
        ),
      );
}

Future<void> _signOut(WidgetRef ref) async {
  await ref.read(sessionManagerProvider).value?.signOut();
}

class StartupScreen extends StatelessWidget {
  const StartupScreen({super.key});

  @override
  Widget build(BuildContext context) => const Scaffold(body: Center(child: CircularProgressIndicator()));
}

class ErrorScreen extends ConsumerWidget {
  const ErrorScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final startup = ref.watch(sessionManagerProvider);
    final error = startup.error ?? ref.watch(meProvider).error;
    return StatusLayout(
      title: 'Something went wrong',
      body: error == null ? 'Try again.' : messageFor(error),
      actions: [
        FilledButton(
          onPressed: () => startup.hasError ? ref.invalidate(sessionManagerProvider) : ref.invalidate(meProvider),
          child: const Text('Retry'),
        ),
      ],
    );
  }
}

class NotConfiguredScreen extends StatelessWidget {
  const NotConfiguredScreen({super.key});

  @override
  Widget build(BuildContext context) => const StatusLayout(
        title: 'Mobile sign-in is not configured',
        body: 'This postit server has no sign-in client for this platform yet. Use the web app, '
            'or ask your administrator to set it up.',
      );
}

class PendingScreen extends ConsumerWidget {
  const PendingScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => RefreshIndicator(
        onRefresh: () => ref.refresh(meProvider.future),
        child: StatusLayout(
          title: 'Waiting for approval',
          body: 'An administrator has been told about your account. You can use postit once they approve it.',
          actions: [
            FilledButton(onPressed: () => ref.invalidate(meProvider), child: const Text('Check again')),
            OutlinedButton(onPressed: () => _signOut(ref), child: const Text('Sign out')),
          ],
        ),
      );
}

class UnavailableScreen extends ConsumerWidget {
  const UnavailableScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => StatusLayout(
        title: 'Account unavailable',
        body: 'This account is disabled or being deleted. Contact an administrator.',
        actions: [OutlinedButton(onPressed: () => _signOut(ref), child: const Text('Sign out'))],
      );
}
```

- [ ] **Step 4: Implement `sign_in_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/widgets/logo.dart';

class SignInScreen extends ConsumerStatefulWidget {
  const SignInScreen({super.key});

  @override
  ConsumerState<SignInScreen> createState() => _SignInScreenState();
}

class _SignInScreenState extends ConsumerState<SignInScreen> {
  bool _busy = false;
  String? _error;

  Future<void> _signIn() async {
    final manager = ref.read(sessionManagerProvider).value;
    if (manager == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await manager.signIn();
    } on AuthCancelled {
      // The user closed the browser: nothing to report.
    } on AuthFailed catch (e) {
      _error = e.message;
    } on Object catch (e) {
      // AuthUnavailable reads as "can't reach postit"; anything unexpected gets the generic
      // message rather than vanishing.
      _error = messageFor(e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final notice = ref.watch(sessionStateProvider).notice;
    final message = _error ?? notice;
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.all(24),
              children: [
                const Center(child: PostitWordmark(height: 40)),
                const SizedBox(height: 32),
                if (message != null) ...[
                  Text(message, key: const ValueKey('sign-in-error'), textAlign: TextAlign.center),
                  const SizedBox(height: 16),
                ],
                FilledButton(
                  onPressed: _busy ? null : _signIn,
                  child: _busy
                      ? const SizedBox.square(dimension: 20, child: CircularProgressIndicator(strokeWidth: 2))
                      : const Text('Sign in'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
```

- [ ] **Step 5: Implement `router.dart`.** The shell branches use placeholder screens, which Tasks 14 and 15 replace.

```dart
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/gate.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/screens/sign_in_screen.dart';
import 'package:postit/ui/screens/status_screens.dart';
import 'package:postit/ui/screens/shell_screen.dart';
import 'package:postit/ui/screens/home_screen.dart';
import 'package:postit/ui/screens/profile_screen.dart';
import 'package:postit/ui/screens/users_screen.dart';
import 'package:postit/ui/screens/user_detail_screen.dart';

final routerProvider = Provider<GoRouter>((ref) {
  final refresh = ValueNotifier<int>(0);
  void bump(Object? _, Object? __) => refresh.value++;
  ref
    ..listen(sessionManagerProvider, bump)
    ..listen(sessionStateProvider, bump)
    ..listen(meProvider, bump)
    ..onDispose(refresh.dispose);

  final router = GoRouter(
    initialLocation: '/startup',
    refreshListenable: refresh,
    redirect: (context, state) {
      final target = gateTarget(
        startup: ref.read(sessionManagerProvider),
        session: ref.read(sessionStateProvider),
        me: ref.read(meProvider),
      );
      final isAdmin = ref.read(meProvider).value?.role == RoleDto.admin;
      return redirectFor(target, state.matchedLocation, isAdmin: isAdmin);
    },
    routes: [
      GoRoute(path: '/', redirect: (_, __) => '/home'),
      GoRoute(path: '/startup', builder: (_, __) => const StartupScreen()),
      GoRoute(path: '/error', builder: (_, __) => const ErrorScreen()),
      GoRoute(path: '/not-configured', builder: (_, __) => const NotConfiguredScreen()),
      GoRoute(path: '/sign-in', builder: (_, __) => const SignInScreen()),
      GoRoute(path: '/pending', builder: (_, __) => const PendingScreen()),
      GoRoute(path: '/unavailable', builder: (_, __) => const UnavailableScreen()),
      StatefulShellRoute.indexedStack(
        builder: (_, __, shell) => ShellScreen(shell: shell),
        branches: [
          StatefulShellBranch(routes: [GoRoute(path: '/home', builder: (_, __) => const HomeScreen())]),
          StatefulShellBranch(routes: [GoRoute(path: '/profile', builder: (_, __) => const ProfileScreen())]),
          StatefulShellBranch(routes: [
            GoRoute(
              path: '/users',
              builder: (_, __) => const UsersScreen(),
              routes: [
                GoRoute(path: ':id', builder: (_, state) => UserDetailScreen(id: state.pathParameters['id']!)),
              ],
            ),
          ]),
        ],
      ),
    ],
  );
  ref.onDispose(router.dispose);
  return router;
});
```

Create minimal placeholders now so the router compiles: `shell_screen.dart`, `home_screen.dart`, `profile_screen.dart`, `users_screen.dart`, `user_detail_screen.dart`. Each is a `StatelessWidget` returning `const Scaffold()`, except `ShellScreen({required StatefulNavigationShell shell})`, which returns `shell`. Tasks 14 and 15 replace them.

- [ ] **Step 6: Implement `app.dart` and `main.dart`**

`mobile/lib/ui/app.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/theme.dart';
import 'package:postit/ui/router.dart';
import 'package:postit/ui/theme/app_theme.dart';

class PostitApp extends ConsumerWidget {
  const PostitApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => MaterialApp.router(
        title: 'postit',
        debugShowCheckedModeBanner: false,
        theme: buildTheme(Brightness.light),
        darkTheme: buildTheme(Brightness.dark),
        themeMode: ref.watch(themeModeProvider),
        themeAnimationDuration: Duration.zero,
        routerConfig: ref.watch(routerProvider),
      );
}
```

`mobile/lib/main.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/dev_trust.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/app.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final config = AppConfig.fromEnvironment();
  final preferences = await PreferencesRepository.open();
  final trust = await devTrust(config);
  runApp(ProviderScope(
    // Failures surface at once; the error screens own retrying.
    retry: (retryCount, error) => null,
    overrides: [
      appConfigProvider.overrideWithValue(config),
      preferencesProvider.overrideWithValue(preferences),
      trustProvider.overrideWithValue(trust),
    ],
    child: const PostitApp(),
  ));
}
```

- [ ] **Step 7: Run the tests**

Run: `flutter test` then `flutter analyze`
Expected: PASS, no issues.

- [ ] **Step 8: Commit**

```bash
git add mobile/lib mobile/test
git commit -m "feat(app): router with the sign-in gate and the status screens

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Shell, Home, and Profile

**Files:**
- Modify: `mobile/lib/ui/screens/shell_screen.dart`, `home_screen.dart`, `profile_screen.dart`
- Create: `mobile/lib/ui/widgets/theme_control.dart`, `mobile/lib/ui/widgets/account_card.dart`
- Test: `mobile/test/ui/screens/shell_screen_test.dart`, `mobile/test/ui/screens/profile_screen_test.dart`, `mobile/test/ui/widgets/theme_control_test.dart`

**Interfaces:**
- Consumes: `meProvider`, `pendingCountProvider`, `activeAdminCountProvider`, `meRepositoryProvider`, `themeModeProvider`, `clockProvider`, `sessionManagerProvider`, and `context.tokens`.
- Produces:
  - `ShellScreen({required StatefulNavigationShell shell})`: a bottom `NavigationBar` below 600 dp and a `NavigationRail` from 600 dp.
  - `ThemeControl`, a three-segment `SegmentedButton<ThemeMode>`.
  - `AccountCard`.

- [ ] **Step 1: Write the failing tests**

`mobile/test/ui/screens/shell_screen_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

void main() {
  Future<void> pumpShell(WidgetTester tester, {RoleDto role = RoleDto.member, Size size = const Size(400, 800)}) async {
    final manager = await signedInManager();
    final users = FakeUsersRepository([
      fakeUser('p1', status: StatusDto.pending),
      fakeUser('a1', role: RoleDto.admin),
    ]);
    await pumpApp(tester, size: size, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: role)),
      usersRepositoryProvider.overrideWithValue(users),
    ]);
  }

  testWidgets('members get Home and Profile on a bottom bar', (tester) async {
    await pumpShell(tester);
    expect(find.byType(NavigationBar), findsOneWidget);
    expect(find.text('Home'), findsWidgets);
    expect(find.text('Profile'), findsOneWidget);
    expect(find.text('Users'), findsNothing);
  });

  testWidgets('admins also get Users with the pending badge', (tester) async {
    await pumpShell(tester, role: RoleDto.admin);
    expect(find.text('Users'), findsOneWidget);
    expect(find.descendant(of: find.byType(Badge), matching: find.text('1')), findsOneWidget);
  });

  testWidgets('wide screens use a navigation rail', (tester) async {
    await pumpShell(tester, size: const Size(900, 700));
    expect(find.byType(NavigationRail), findsOneWidget);
    expect(find.byType(NavigationBar), findsNothing);
  });

  testWidgets('home greets by time of day and warns the only admin', (tester) async {
    await pumpShell(tester, role: RoleDto.admin);
    expect(find.text('Good morning, Ada'), findsOneWidget);
    expect(find.textContaining('only active admin'), findsOneWidget);
    expect(find.text('Review'), findsOneWidget);
  });
}
```

`mobile/test/ui/screens/profile_screen_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fakes.dart';
import '../../helpers/harness.dart';
import '../../helpers/fake_users.dart';

class _FakeMe implements MeRepository {
  final deleted = <String>[];
  @override
  Future<MeDto> get() async => fakeMe(StatusDto.active);
  @override
  Future<void> delete(String displayName) async => deleted.add(displayName);
}

void main() {
  testWidgets('deleting the account needs the exact display name, then signs out', (tester) async {
    final client = FakeAuthClient();
    final manager = await signedInManager(client);
    final meRepo = _FakeMe();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
      meRepositoryProvider.overrideWithValue(meRepo),
    ]);
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.text('Delete my postit account'), 200);
    await tester.tap(find.text('Delete my postit account'));
    await tester.pumpAndSettle();

    final confirm = find.widgetWithText(FilledButton, 'Delete account');
    await tester.enterText(find.byType(TextField), 'ada');
    await tester.pump();
    expect(tester.widget<FilledButton>(confirm).onPressed, isNull);

    await tester.enterText(find.byType(TextField), 'Ada');
    await tester.pump();
    await tester.tap(confirm);
    await tester.pumpAndSettle();
    expect(meRepo.deleted, ['Ada']);
    expect(client.endedSessions, ['id-signin']);
    expect(find.text('Sign in'), findsOneWidget);
  });

  testWidgets('sign out returns to sign-in', (tester) async {
    final manager = await signedInManager();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
    ]);
    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Sign out'));
    await tester.pumpAndSettle();
    expect(find.text('Sign in'), findsOneWidget);
  });
}
```

`mobile/test/ui/widgets/theme_control_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/theme.dart';
import 'package:postit/ui/widgets/theme_control.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  Future<ProviderContainer> pump(WidgetTester tester, PreferencesRepository prefs) async {
    final container = ProviderContainer(overrides: [preferencesProvider.overrideWithValue(prefs)]);
    addTearDown(container.dispose);
    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const MaterialApp(home: Scaffold(body: ThemeControl())),
    ));
    return container;
  }

  testWidgets('choosing dark applies and persists it', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final container = await pump(tester, prefs);
    expect(container.read(themeModeProvider), ThemeMode.system);
    await tester.tap(find.byTooltip('Dark'));
    await tester.pump();
    expect(container.read(themeModeProvider), ThemeMode.dark);
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.dark);
  });

  testWidgets('without storage the control still switches', (tester) async {
    final container = await pump(tester, PreferencesRepository(null));
    await tester.tap(find.byTooltip('Light'));
    await tester.pump();
    expect(container.read(themeModeProvider), ThemeMode.light);
  });
}
```

Run: `flutter test test/ui`
Expected: FAIL, missing widgets and text.

- [ ] **Step 2: Implement `theme_control.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/theme.dart';

/// System, light and dark, as on the web's segmented control.
class ThemeControl extends ConsumerWidget {
  const ThemeControl({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) => SegmentedButton<ThemeMode>(
        showSelectedIcon: false,
        segments: const [
          ButtonSegment(value: ThemeMode.system, icon: Icon(Icons.brightness_auto_outlined), tooltip: 'System'),
          ButtonSegment(value: ThemeMode.light, icon: Icon(Icons.light_mode_outlined), tooltip: 'Light'),
          ButtonSegment(value: ThemeMode.dark, icon: Icon(Icons.dark_mode_outlined), tooltip: 'Dark'),
        ],
        selected: {ref.watch(themeModeProvider)},
        onSelectionChanged: (s) => ref.read(themeModeProvider.notifier).set(s.single),
      );
}
```

- [ ] **Step 3: Implement `account_card.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:postit/api/export.dart';
import 'package:postit/ui/theme/tokens.dart';

/// Name, email and role of the signed-in user.
class AccountCard extends StatelessWidget {
  const AccountCard({super.key, required this.me});
  final MeDto me;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Row(children: [
          CircleAvatar(
            backgroundColor: context.tokens.pinkSoft,
            foregroundColor: context.tokens.pinkText,
            child: Text(me.displayName.characters.first.toUpperCase()),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(me.displayName, style: text.titleMedium),
              if (me.email != null) Text(me.email!, style: text.bodySmall),
            ]),
          ),
          Text(me.role == RoleDto.admin ? 'ADMIN' : 'MEMBER', style: text.labelSmall),
        ]),
      ),
    );
  }
}
```

- [ ] **Step 4: Implement `shell_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';

/// Home, Profile and (admins) Users. Plan 03 D6 puts Posts and Calendar in slots 2 and 3.
class ShellScreen extends ConsumerWidget {
  const ShellScreen({super.key, required this.shell});
  final StatefulNavigationShell shell;

  static const _wide = 600.0;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final isAdmin = ref.watch(meProvider).value?.role == RoleDto.admin;
    final pending = isAdmin ? (ref.watch(pendingCountProvider).value ?? 0) : 0;
    Widget usersIcon(IconData icon) =>
        Badge(isLabelVisible: pending > 0, label: Text('$pending'), child: Icon(icon));
    final items = [
      (icon: const Icon(Icons.home_outlined), selected: const Icon(Icons.home), label: 'Home'),
      (icon: const Icon(Icons.person_outline), selected: const Icon(Icons.person), label: 'Profile'),
      if (isAdmin) (icon: usersIcon(Icons.group_outlined), selected: usersIcon(Icons.group), label: 'Users'),
    ];
    void go(int i) => shell.goBranch(i, initialLocation: i == shell.currentIndex);
    final index = shell.currentIndex.clamp(0, items.length - 1);

    return LayoutBuilder(builder: (context, constraints) {
      if (constraints.maxWidth >= _wide) {
        return Scaffold(
          body: Row(children: [
            NavigationRail(
              selectedIndex: index,
              onDestinationSelected: go,
              labelType: NavigationRailLabelType.all,
              destinations: [
                for (final i in items)
                  NavigationRailDestination(icon: i.icon, selectedIcon: i.selected, label: Text(i.label)),
              ],
            ),
            const VerticalDivider(width: 1),
            Expanded(child: shell),
          ]),
        );
      }
      return Scaffold(
        body: shell,
        bottomNavigationBar: NavigationBar(
          selectedIndex: index,
          onDestinationSelected: go,
          destinations: [
            for (final i in items) NavigationDestination(icon: i.icon, selectedIcon: i.selected, label: i.label),
          ],
        ),
      );
    });
  }
}
```

- [ ] **Step 5: Implement `home_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:intl/intl.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';

String greetingFor(DateTime now) => now.hour < 12
    ? 'Good morning'
    : now.hour < 18
        ? 'Good afternoon'
        : 'Good evening';

class HomeScreen extends ConsumerWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final me = ref.watch(meProvider).value;
    if (me == null) return const SizedBox.shrink();
    final now = ref.watch(clockProvider)();
    final isAdmin = me.role == RoleDto.admin;
    final text = Theme.of(context).textTheme;
    final tokens = context.tokens;
    final firstName = me.displayName.split(' ').first;

    return Scaffold(
      body: SafeArea(
        child: RefreshIndicator(
          onRefresh: () async {
            ref
              ..invalidate(meProvider)
              ..invalidate(pendingCountProvider)
              ..invalidate(activeAdminCountProvider);
          },
          child: ListView(padding: const EdgeInsets.all(16), children: [
            Text(DateFormat('EEEE, d MMMM').format(now).toUpperCase(), style: text.labelSmall),
            const SizedBox(height: 4),
            Text('${greetingFor(now)}, $firstName', style: text.headlineMedium),
            const SizedBox(height: 16),
            if (isAdmin && ref.watch(activeAdminCountProvider).value == 1)
              Card(
                color: tokens.warnBg,
                shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12), side: BorderSide(color: tokens.warnLine)),
                child: ListTile(
                  leading: Icon(Icons.warning_amber_rounded, color: tokens.warnIcon),
                  title: Text(
                    "You're the only active admin. Make someone else an admin so the team isn't locked out.",
                    style: TextStyle(color: tokens.warnText),
                  ),
                ),
              ),
            if (isAdmin) ...[
              const SizedBox(height: 8),
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Row(children: [
                    Expanded(
                      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                        Text('APPROVAL QUEUE', style: text.labelSmall),
                        Text('${ref.watch(pendingCountProvider).value ?? 0}', style: text.displaySmall),
                        Text('waiting for approval', style: text.bodySmall),
                      ]),
                    ),
                    FilledButton(onPressed: () => context.go('/users'), child: const Text('Review')),
                  ]),
                ),
              ),
            ],
            const SizedBox(height: 8),
            AccountCard(me: me),
          ]),
        ),
      ),
    );
  }
}
```

- [ ] **Step 6: Implement `profile_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/theme/tokens.dart';
import 'package:postit/ui/widgets/account_card.dart';
import 'package:postit/ui/widgets/theme_control.dart';
import 'package:url_launcher/url_launcher.dart';

class ProfileScreen extends ConsumerWidget {
  const ProfileScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final me = ref.watch(meProvider).value;
    if (me == null) return const SizedBox.shrink();
    final text = Theme.of(context).textTheme;
    final accountUrl = me.accountUrl;
    return Scaffold(
      body: SafeArea(
        child: ListView(padding: const EdgeInsets.all(16), children: [
          Text('Profile', style: text.headlineMedium),
          const SizedBox(height: 16),
          AccountCard(me: me),
          const SizedBox(height: 8),
          Text('Your name and email come from your sign-in provider.', style: text.bodySmall),
          if (accountUrl != null)
            TextButton.icon(
              onPressed: () => launchUrl(Uri.parse(accountUrl), mode: LaunchMode.externalApplication),
              icon: const Icon(Icons.open_in_new),
              label: const Text('Manage your account'),
            ),
          const SizedBox(height: 24),
          Text('THEME', style: text.labelSmall),
          const SizedBox(height: 8),
          const ThemeControl(),
          const SizedBox(height: 24),
          OutlinedButton(
            onPressed: () => ref.read(sessionManagerProvider).value?.signOut(),
            child: const Text('Sign out'),
          ),
          const SizedBox(height: 32),
          Text('DANGER ZONE', style: text.labelSmall?.copyWith(color: context.tokens.danger)),
          const SizedBox(height: 8),
          OutlinedButton(
            style: OutlinedButton.styleFrom(foregroundColor: context.tokens.danger),
            onPressed: () => showDialog<void>(
              context: context,
              builder: (_) => _DeleteAccountDialog(displayName: me.displayName),
            ),
            child: const Text('Delete my postit account'),
          ),
        ]),
      ),
    );
  }
}

class _DeleteAccountDialog extends ConsumerStatefulWidget {
  const _DeleteAccountDialog({required this.displayName});
  final String displayName;

  @override
  ConsumerState<_DeleteAccountDialog> createState() => _DeleteAccountDialogState();
}

class _DeleteAccountDialogState extends ConsumerState<_DeleteAccountDialog> {
  final _controller = TextEditingController();
  bool _busy = false;
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _delete() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref.read(meRepositoryProvider).delete(_controller.text);
      if (mounted) Navigator.of(context).pop();
      await ref.read(sessionManagerProvider).value?.signOut();
    } on Object catch (e) {
      if (mounted) setState(() => _error = messageFor(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final matches = _controller.text == widget.displayName;
    return AlertDialog(
      title: const Text('Delete your postit account?'),
      content: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text('This removes your postit account and cannot be undone. Type "${widget.displayName}" to confirm.'),
        const SizedBox(height: 12),
        TextField(controller: _controller, autofocus: true, onChanged: (_) => setState(() {})),
        if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!)),
      ]),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: context.tokens.danger),
          onPressed: matches && !_busy ? _delete : null,
          child: const Text('Delete account'),
        ),
      ],
    );
  }
}
```

- [ ] **Step 7: Run the tests**

Run: `flutter test` then `flutter analyze`
Expected: PASS. A `last_admin` error from `DELETE /me` stays in the dialog with its message.

- [ ] **Step 8: Commit**

```bash
git add mobile/lib/ui mobile/test/ui
git commit -m "feat(app): navigation shell, Home and Profile with account deletion

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Users for admins

**Files:**
- Modify: `mobile/lib/ui/screens/users_screen.dart`, `mobile/lib/ui/screens/user_detail_screen.dart`
- Test: `mobile/test/ui/screens/users_screen_test.dart`

**Interfaces:**
- Consumes:
  - From Task 11: `usersListProvider` / `UsersListNotifier.loadMore`, `userProvider`, `pendingCountProvider`, `activeAdminCountProvider`, `usersRepositoryProvider`, `meProvider`.
  - From Task 7: `messageFor`.
- Produces:
  - `UsersScreen`: search, status chips, infinite scroll; pending first when any exist.
  - `UserDetailScreen(id)`: the actions the user's status allows.

- [ ] **Step 1: Write the failing tests** (`mobile/test/ui/screens/users_screen_test.dart`):

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/state/providers.dart';

import '../../helpers/fake_users.dart';
import '../../helpers/harness.dart';

void main() {
  late FakeUsersRepository users;

  Future<void> openUsers(WidgetTester tester) async {
    final manager = await signedInManager();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: RoleDto.admin)),
      usersRepositoryProvider.overrideWithValue(users),
    ]);
    await tester.tap(find.text('Users'));
    await tester.pumpAndSettle();
  }

  setUp(() {
    users = FakeUsersRepository([
      fakeUser('p1', status: StatusDto.pending, name: 'Pat Pending'),
      fakeUser('a1', name: 'Ann Active'),
      fakeUser('d1', status: StatusDto.disabled, name: 'Dan Disabled'),
    ]);
  });

  testWidgets('starts on pending when someone is waiting', (tester) async {
    await openUsers(tester);
    expect(find.text('Pat Pending'), findsOneWidget);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('approving a pending user', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Pat Pending'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Approve'));
    await tester.pumpAndSettle();
    expect(users.calls, ['approve p1']);
    expect(find.text('Disable'), findsOneWidget);
  });

  testWidgets('disabling asks for confirmation and shows last_admin', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    users.failWith = const ApiError(409, 'last_admin');
    await tester.tap(find.text('Disable'));
    await tester.pumpAndSettle();
    expect(find.text('Disable Ann Active?'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Disable'));
    await tester.pumpAndSettle();
    expect(find.text('That would remove the last active admin.'), findsOneWidget);
  });

  testWidgets('rejecting a pending user deletes them and returns to the list', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Pat Pending'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Reject'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Reject'));
    await tester.pumpAndSettle();
    expect(users.calls, ['delete p1']);
    expect(find.text('Pat Pending'), findsNothing);
  });

  testWidgets('search filters by name', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'dan');
    await tester.testTextInput.receiveAction(TextInputAction.search);
    await tester.pumpAndSettle();
    expect(find.text('Dan Disabled'), findsOneWidget);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('scrolling to the end loads the next page', (tester) async {
    users = FakeUsersRepository(List.generate(40, (i) => fakeUser('u$i', name: 'User $i')));
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    // The search field and the chip row are scrollables too; the list is the last one.
    await tester.scrollUntilVisible(find.text('User 39'), 400, scrollable: find.byType(Scrollable).last);
    expect(users.pagesRequested, containsAllInOrder([1, 2]));
  });

  testWidgets('a tall screen keeps loading until the list scrolls', (tester) async {
    users = FakeUsersRepository(List.generate(40, (i) => fakeUser('u$i', name: 'User $i')));
    final manager = await signedInManager();
    await pumpApp(tester, size: const Size(900, 4000), overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active, role: RoleDto.admin)),
      usersRepositoryProvider.overrideWithValue(users),
    ]);
    await tester.tap(find.text('Users'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    expect(users.pagesRequested, containsAllInOrder([1, 2]));
  });

  testWidgets('enabling a disabled user needs no confirmation', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('Disabled'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Dan Disabled'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Enable'));
    await tester.pumpAndSettle();
    expect(users.calls, ['enable d1']);
    expect(find.text('Disable'), findsOneWidget);
  });

  testWidgets('making an admin is direct; making a member asks first', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Make admin'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1']);
    await tester.tap(find.text('Make member'));
    await tester.pumpAndSettle();
    expect(find.text('Make Ann Active a member?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1']);
    await tester.tap(find.text('Make member'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Make member'));
    await tester.pumpAndSettle();
    expect(users.calls, ['role:admin a1', 'role:member a1']);
  });

  testWidgets('deleting asks first and returns to the list', (tester) async {
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ann Active'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();
    expect(find.text('Delete Ann Active?'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Delete'));
    await tester.pumpAndSettle();
    expect(users.calls, ['delete a1']);
    expect(find.text('Ann Active'), findsNothing);
  });

  testWidgets('you cannot delete yourself from the detail screen', (tester) async {
    users.users.add(fakeUser('u1', name: 'Ada', role: RoleDto.admin));
    await openUsers(tester);
    await tester.tap(find.text('All'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Ada'));
    await tester.pumpAndSettle();
    expect(find.text('Delete'), findsNothing);
  });

  testWidgets('members never reach /users', (tester) async {
    final manager = await signedInManager();
    await pumpApp(tester, overrides: [
      sessionManagerProvider.overrideWith((ref) async => manager),
      meProvider.overrideWith((ref) async => fakeMe(StatusDto.active)),
      usersRepositoryProvider.overrideWithValue(users),
    ]);
    expect(find.text('Users'), findsNothing);
  });
}
```

Run: `flutter test test/ui/screens/users_screen_test.dart`
Expected: FAIL.

- [ ] **Step 2: Implement `users_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';

String statusLabel(StatusDto s) => switch (s) {
      StatusDto.pending => 'Pending',
      StatusDto.active => 'Active',
      StatusDto.disabled => 'Disabled',
      _ => 'Deleting',
    };

class UsersScreen extends ConsumerStatefulWidget {
  const UsersScreen({super.key});

  @override
  ConsumerState<UsersScreen> createState() => _UsersScreenState();
}

class _UsersScreenState extends ConsumerState<UsersScreen> {
  final _scroll = ScrollController();
  StatusDto? _status;
  bool _statusChosen = false;
  String _search = '';

  UsersFilter get _filter => (status: _status, search: _search);

  @override
  void initState() {
    super.initState();
    _scroll.addListener(() {
      if (_scroll.position.extentAfter < 300) ref.read(usersListProvider(_filter).notifier).loadMore();
    });
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  /// When a page does not fill the screen (a tablet, the rail layout) nothing scrolls, so the
  /// next page is requested after layout instead.
  Widget _fillViewport(UsersPage value, Widget list) {
    if (value.hasMore) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _scroll.hasClients && _scroll.position.maxScrollExtent <= 0) {
          ref.read(usersListProvider(_filter).notifier).loadMore();
        }
      });
    }
    return list;
  }

  @override
  Widget build(BuildContext context) {
    final pending = ref.watch(pendingCountProvider);
    if (!_statusChosen) {
      if (pending.isLoading && !pending.hasValue) return const Center(child: CircularProgressIndicator());
      _status = (pending.value ?? 0) > 0 ? StatusDto.pending : null;
      _statusChosen = true;
    }
    final list = ref.watch(usersListProvider(_filter));
    final chips = <(String, StatusDto?)>[
      ('All', null),
      ('Pending', StatusDto.pending),
      ('Active', StatusDto.active),
      ('Disabled', StatusDto.disabled),
    ];
    return Scaffold(
      appBar: AppBar(title: const Text('Users')),
      body: Column(children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
          child: TextField(
            decoration: const InputDecoration(prefixIcon: Icon(Icons.search), hintText: 'Search by name or email'),
            textInputAction: TextInputAction.search,
            onSubmitted: (v) => setState(() => _search = v.trim()),
          ),
        ),
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Row(children: [
            for (final (label, status) in chips)
              Padding(
                padding: const EdgeInsets.only(right: 8),
                child: ChoiceChip(
                  label: Text(label),
                  selected: _status == status,
                  onSelected: (_) => setState(() => _status = status),
                ),
              ),
          ]),
        ),
        Expanded(
          child: RefreshIndicator(
            onRefresh: () async {
              ref
                ..invalidate(usersListProvider(_filter))
                ..invalidate(pendingCountProvider);
            },
            child: switch (list) {
              AsyncData(:final value) when value.items.isEmpty =>
                ListView(children: const [Padding(padding: EdgeInsets.all(32), child: Center(child: Text('No users match.')))]),
              AsyncData(:final value) => _fillViewport(value, ListView.builder(
                  controller: _scroll,
                  physics: const AlwaysScrollableScrollPhysics(),
                  itemCount: value.items.length + (value.hasMore ? 1 : 0),
                  itemBuilder: (context, i) {
                    if (i >= value.items.length) {
                      return const Padding(padding: EdgeInsets.all(16), child: Center(child: CircularProgressIndicator()));
                    }
                    final u = value.items[i];
                    return ListTile(
                      title: Text(u.displayName),
                      subtitle: Text(u.email ?? ''),
                      trailing: Text(statusLabel(u.status).toUpperCase(), style: Theme.of(context).textTheme.labelSmall),
                      onTap: () => context.go('/users/${u.id}'),
                    );
                  },
                )),
              AsyncError(:final error) => ListView(children: [
                  Padding(padding: const EdgeInsets.all(32), child: Center(child: Text(messageFor(error)))),
                ]),
              _ => const Center(child: CircularProgressIndicator()),
            },
          ),
        ),
      ]),
    );
  }
}
```

- [ ] **Step 3: Implement `user_detail_screen.dart`**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:postit/api/export.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/users.dart';
import 'package:postit/ui/errors/messages.dart';
import 'package:postit/ui/screens/users_screen.dart';
import 'package:postit/ui/theme/tokens.dart';

class UserDetailScreen extends ConsumerStatefulWidget {
  const UserDetailScreen({super.key, required this.id});
  final String id;

  @override
  ConsumerState<UserDetailScreen> createState() => _UserDetailScreenState();
}

class _UserDetailScreenState extends ConsumerState<UserDetailScreen> {
  bool _busy = false;
  String? _error;

  Future<bool> _confirm(String title, String body, String action) async =>
      await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(body),
          actions: [
            TextButton(onPressed: () => Navigator.of(context).pop(false), child: const Text('Cancel')),
            FilledButton(
              style: FilledButton.styleFrom(backgroundColor: context.tokens.danger),
              onPressed: () => Navigator.of(context).pop(true),
              child: Text(action),
            ),
          ],
        ),
      ) ??
      false;

  Future<void> _run(Future<void> Function() action, {bool leave = false}) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
      ref
        ..invalidate(usersListProvider)
        ..invalidate(pendingCountProvider)
        ..invalidate(activeAdminCountProvider);
      // Acting on yourself (say, Make member) changes /me: the gate then drops the Users tab.
      if (ref.read(meProvider).value?.id == widget.id) ref.invalidate(meProvider);
      if (leave) {
        if (mounted) context.go('/users');
      } else {
        ref.invalidate(userProvider(widget.id));
      }
    } on Object catch (e) {
      if (mounted) setState(() => _error = messageFor(e));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final meId = ref.watch(meProvider).value?.id;
    final user = ref.watch(userProvider(widget.id));
    return Scaffold(
      appBar: AppBar(title: const Text('User')),
      body: switch (user) {
        AsyncData(value: final u) => ListView(padding: const EdgeInsets.all(16), children: [
            Text(u.displayName, style: Theme.of(context).textTheme.headlineSmall),
            if (u.email != null) Text(u.email!),
            const SizedBox(height: 8),
            Text('${statusLabel(u.status).toUpperCase()} · ${u.role == RoleDto.admin ? 'ADMIN' : 'MEMBER'}',
                style: Theme.of(context).textTheme.labelSmall),
            const SizedBox(height: 24),
            if (_error != null) Padding(padding: const EdgeInsets.only(bottom: 12), child: Text(_error!)),
            ..._actions(u, isSelf: u.id == meId),
          ]),
        AsyncError(:final error) => Center(child: Text(messageFor(error))),
        _ => const Center(child: CircularProgressIndicator()),
      },
    );
  }

  List<Widget> _actions(UserDto u, {required bool isSelf}) {
    final users = ref.read(usersRepositoryProvider);
    Widget button(String label, VoidCallback onPressed, {bool primary = false}) => Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: primary
              ? FilledButton(onPressed: _busy ? null : onPressed, child: Text(label))
              : OutlinedButton(onPressed: _busy ? null : onPressed, child: Text(label)),
        );
    final roleTarget = u.role == RoleDto.admin ? RoleDto.member : RoleDto.admin;
    final roleLabel = u.role == RoleDto.admin ? 'Make member' : 'Make admin';
    return switch (u.status) {
      StatusDto.pending => [
          button('Approve', () => _run(() => users.approve(u.id)), primary: true),
          button('Reject', () async {
            if (await _confirm('Reject ${u.displayName}?', 'Their pending account is deleted.', 'Reject')) {
              await _run(() => users.delete(u.id), leave: true);
            }
          }),
        ],
      StatusDto.active || StatusDto.disabled => [
          if (u.status == StatusDto.active)
            button('Disable', () async {
              if (await _confirm('Disable ${u.displayName}?', 'They lose access until re-enabled.', 'Disable')) {
                await _run(() => users.disable(u.id));
              }
            })
          else
            button('Enable', () => _run(() => users.enable(u.id)), primary: true),
          button(roleLabel, () async {
            if (roleTarget == RoleDto.admin ||
                await _confirm('Make ${u.displayName} a member?', 'They lose the admin screens.', 'Make member')) {
              await _run(() => users.setRole(u.id, roleTarget));
            }
          }),
          if (!isSelf)
            button('Delete', () async {
              if (await _confirm('Delete ${u.displayName}?', 'This cannot be undone.', 'Delete')) {
                await _run(() => users.delete(u.id), leave: true);
              }
            }),
        ],
      _ => [const Text('This user is being deleted.')],
    };
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `flutter test` then `flutter analyze`
Expected: PASS, no issues.

- [ ] **Step 5: Commit**

```bash
git add mobile/lib/ui/screens mobile/test/ui/screens/users_screen_test.dart
git commit -m "feat(app): user management for admins on mobile

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: Opt-in smoke test against the dev stack

**Files:**
- Create: `mobile/integration_test/smoke_test.dart`
- Modify: `mobile/pubspec.yaml` (dev dependency `integration_test: sdk: flutter`)

**Interfaces:**
- Consumes: the whole app; the dev stack; an AVD prepared by Task 3.

- [ ] **Step 1: Add the dependency.** In `mobile/pubspec.yaml` under `dev_dependencies`:

```yaml
  integration_test:
    sdk: flutter
```

- [ ] **Step 2: Write the test.** The Zitadel login runs in Chrome outside the app, so a person types the credentials. The test waits up to two minutes for that.

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:postit/main.dart' as app;

/// Opt-in, not in CI: needs the dev stack (./stack.ps1 up, cargo run) and a prepared AVD.
/// After "Sign in" opens Chrome, sign in as admin@postit.com by hand within two minutes.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('signs in through Zitadel and reaches Home', (tester) async {
    app.main();
    await tester.pumpAndSettle(const Duration(seconds: 5));
    await tester.tap(find.text('Sign in'));
    final deadline = DateTime.now().add(const Duration(minutes: 2));
    while (find.text('Profile').evaluate().isEmpty && DateTime.now().isBefore(deadline)) {
      await tester.pump(const Duration(seconds: 1));
    }
    expect(find.text('Profile'), findsOneWidget);
    expect(find.textContaining('Good '), findsOneWidget);

    await tester.tap(find.text('Profile'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(OutlinedButton, 'Sign out'));
    final signedOut = DateTime.now().add(const Duration(minutes: 1));
    while (find.text('Sign in').evaluate().isEmpty && DateTime.now().isBefore(signedOut)) {
      await tester.pump(const Duration(seconds: 1));
    }
    expect(find.text('Sign in'), findsOneWidget);
  });
}
```

- [ ] **Step 3: Run it once.** With the stack up and the AVD started:

```powershell
flutter test integration_test/smoke_test.dart --flavor development --dart-define-from-file=config/development.json
```

Expected: PASS after a manual sign-in as `admin@postit.com`.

- [ ] **Step 4: Commit**

```bash
git add mobile/integration_test mobile/pubspec.yaml mobile/pubspec.lock
git commit -m "test(app): opt-in sign-in smoke test against the dev stack

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 17: Android release signing from the vault

**Files:**
- Modify: `mobile/android/app/build.gradle.kts`
- Create: `mobile/android/key.properties.example`

**Interfaces:**
- Consumes: `!ref/vault/<env>/android.jks` and `!ref/vault/<env>/android.env` (`ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`), which the maintainer creates.
- Produces: signed `qa` and `production` release builds when `mobile/android/key.properties` exists, with a debug-signing fallback and a warning otherwise.

- [ ] **Step 1: Write `key.properties.example`**

```properties
# Copy to key.properties (git-ignored). The path is relative to mobile/android/.
# Each environment folder holds android.jks and android.env (see docs/deployment.md).
vaultDir=../../!ref/vault
```

- [ ] **Step 2: Implement signing** in `build.gradle.kts`. At the top, add `import java.util.Properties`. Before `android {`, add:

```kotlin
val keyProperties = Properties().apply {
    val file = rootProject.file("key.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}
val vaultDir: File? = keyProperties.getProperty("vaultDir")?.let { rootProject.file(it) }

/// The signing secrets of one environment, or null when the vault is not extracted.
fun vaultSigning(env: String): Properties? {
    val dir = vaultDir ?: return null
    val file = File(dir, "$env/android.env")
    if (!file.exists() || !File(dir, "$env/android.jks").exists()) return null
    return Properties().apply { file.inputStream().use { load(it) } }
}
```

Inside `android { }`, add a `signingConfigs` block:

```kotlin
    signingConfigs {
        for (env in listOf("qa", "production")) {
            val secrets = vaultSigning(env) ?: continue
            create(env) {
                storeFile = File(vaultDir, "$env/android.jks")
                storePassword = secrets.getProperty("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = secrets.getProperty("ANDROID_KEY_ALIAS")
                keyPassword = secrets.getProperty("ANDROID_KEY_PASSWORD")
            }
        }
    }
```

In `buildTypes { release { … } }`, remove the template line `signingConfig = signingConfigs.getByName("debug")`. Then give each flavor its signing config. Add to the `development` flavor:

```kotlin
            signingConfig = signingConfigs.getByName("debug")
```

Add to `qa` and `production` (shown for `qa`):

```kotlin
            signingConfig = signingConfigs.findByName("qa") ?: signingConfigs.getByName("debug").also {
                logger.warn("postit: no qa keystore (mobile/android/key.properties and !ref/vault/qa/); release builds use debug signing")
            }
```

The debug build type keeps its debug signing, so `--debug` builds never need the vault.

- [ ] **Step 3: Verify both paths**

Run from `mobile/`, without `key.properties`:

```powershell
flutter build apk --flavor qa --release --dart-define-from-file=config/qa.json
```

Expected: the build succeeds and prints the "no qa keystore" warning.

If the maintainer has created `!ref/vault/qa/android.jks` and `android.env` and the vault is extracted, copy `key.properties.example` to `key.properties` and repeat. Expected: no warning, and `& "$env:ANDROID_HOME\build-tools\<ver>\apksigner.bat" verify --print-certs build/app/outputs/flutter-apk/app-qa-release.apk` shows the qa certificate. Otherwise skip this half and say so in the task report.

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/build.gradle.kts mobile/android/key.properties.example
git commit -m "build(app): sign qa and production releases with vault keystores

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 18: iOS scaffold (unverified)

**Files:**
- Create: `mobile/ios/Flutter/Development.xcconfig`, `mobile/ios/Flutter/Qa.xcconfig`, `mobile/ios/Flutter/Production.xcconfig`
- Modify: `mobile/ios/Flutter/Debug.xcconfig`, `mobile/ios/Flutter/Release.xcconfig`, `mobile/ios/Runner/Info.plist`, `mobile/ios/Podfile` (if present)

**Interfaces:**
- Produces: per-flavor bundle IDs and URL schemes, ready for a Mac. Xcode build configurations and schemes (`Debug-development` and the like) are created on the first Mac session, following the README steps from Task 20. Editing `project.pbxproj` by hand without Xcode is error-prone, so this is a recorded deviation from the spec's "scheme per flavor".

- [ ] **Step 1: Write the flavor xcconfigs**

`Development.xcconfig`:

```
PRODUCT_BUNDLE_IDENTIFIER = io.github.asm2025.postit.dev
POSTIT_URL_SCHEME = postit-dev
POSTIT_DISPLAY_NAME = postit dev
```

`Qa.xcconfig`:

```
PRODUCT_BUNDLE_IDENTIFIER = io.github.asm2025.postit.qa
POSTIT_URL_SCHEME = postit-qa
POSTIT_DISPLAY_NAME = postit qa
```

`Production.xcconfig`:

```
PRODUCT_BUNDLE_IDENTIFIER = io.github.asm2025.postit
POSTIT_URL_SCHEME = postit
POSTIT_DISPLAY_NAME = postit
```

Append `#include "Development.xcconfig"` to `Debug.xcconfig` and `#include "Production.xcconfig"` to `Release.xcconfig`, after the existing `Generated.xcconfig` include. Until a Mac adds per-flavor configurations, a debug build is the development flavor and a release build is production.

- [ ] **Step 2: Edit `Info.plist`.** Set `CFBundleDisplayName` to `$(POSTIT_DISPLAY_NAME)`, and add inside the top-level `<dict>`:

```xml
	<key>CFBundleURLTypes</key>
	<array>
		<dict>
			<key>CFBundleURLSchemes</key>
			<array>
				<string>$(POSTIT_URL_SCHEME)</string>
			</array>
		</dict>
	</array>
```

- [ ] **Step 3: Set the deployment target.**
  - Read the minimum iOS version from the `flutter_appauth` and `flutter_secure_storage` podspecs and from Flutter 3.47's iOS floor, and take the highest.
  - If `mobile/ios/Podfile` exists, set `platform :ios, '<that version>'` in it. Otherwise record the version for the README; `pod install` on the Mac creates the Podfile.
  - Set `IPHONEOS_DEPLOYMENT_TARGET = <that version>` in all three flavor xcconfigs.

- [ ] **Step 4: Check that the plist is valid XML** and that the Android build is unaffected:

```powershell
[xml](Get-Content mobile/ios/Runner/Info.plist) | Out-Null
flutter build apk --flavor qa --debug --dart-define-from-file=config/qa.json
```

Expected: no XML error; the build succeeds.

- [ ] **Step 5: Commit**

```bash
git add mobile/ios
git commit -m "feat(app): iOS flavor settings and URL schemes (unverified until a Mac)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 19: CI: the `app` job and Flutter in `contract`

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Task 4's flavors, Task 6's generator.

- [ ] **Step 1: Add Flutter to the `contract` job.** After the `install web dependencies` step, add:

```yaml
      - uses: subosito/flutter-action@v2
        with:
          flutter-version: 3.47.6
          channel: stable
          cache: true
      - name: install app dependencies
        # cargo xtask openapi also generates the Dart client in mobile/lib/api.
        working-directory: app
        run: flutter pub get
```

- [ ] **Step 2: Add the `app` job** after `web`:

```yaml
  app:
    name: app
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: app
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-java@v4
        with:
          distribution: temurin
          java-version: "21"
      - uses: subosito/flutter-action@v2
        with:
          flutter-version: 3.47.6
          channel: stable
          cache: true
      - name: install
        run: flutter pub get
      - name: format
        # lib/api is generated and never formatted by hand.
        run: dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test
      - name: analyze
        run: flutter analyze
      - name: test
        run: flutter test
      - name: build qa and production
        run: |
          flutter build apk --flavor qa --debug --dart-define-from-file=config/qa.json
          flutter build apk --flavor production --debug --dart-define-from-file=config/production.json
      - name: no dev trust configuration in qa or production
        # The development flavor's network_security_config, its raw CA resource, and the dev
        # CA Flutter asset (for dio) must never ship.
        run: |
          aapt2="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n 1)/aapt2"
          for flavor in qa production; do
            apk="build/app/outputs/flutter-apk/app-$flavor-debug.apk"
            if "$aapt2" dump xmltree --file AndroidManifest.xml "$apk" | grep -qi networkSecurityConfig; then
              echo "::error::the $flavor APK manifest references a network security config"; exit 1
            fi
            if unzip -l "$apk" | grep -Ei 'network_security_config|postit[-_]dev[-_]ca'; then
              echo "::error::the $flavor APK contains dev trust files"; exit 1
            fi
            echo "$flavor APK carries no dev trust configuration"
          done
```

`integration_test` exists from Task 16, which runs before this task.

- [ ] **Step 3: Validate locally.** Run the same check in Git Bash against the qa and production APKs from Task 4, then with `for flavor in development`. The development run must print the errors for both the manifest and the files (`res/…/postit_dev_ca…` and `assets/flutter_assets/assets/dev/postit-dev-ca.crt`). That's the positive control, proving the patterns match real output. Restore nothing; it's read-only.

- [ ] **Step 4: Push and watch.** Ask the user before pushing: pushing publishes the branch. With their go-ahead: `git push` and `gh run watch`.
Expected: `server`, `server-windows`, `web`, `app`, `contract`, and `docker` are green. If `contract` fails on `mobile/lib/api` only on Linux, apply Task 6 Step 5's fallback.

- [ ] **Step 5: Commit** (before the push in Step 4)

```bash
git add .github/workflows/ci.yml
git commit -m "ci: app job with the dev-trust check, and Flutter in contract

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 20: Docs, plan status, and CLAUDE.md

**Files:**
- Modify: `README.md`, `docs/deployment.md`, `docs/security.md`, `docs/plans/02. foundation.md` (P11 "Built" line), `CLAUDE.md`, `docs/specs/2026-10-16-p11-flutter-app-design.md` (deviations)

- [ ] **Step 1: Add the README mobile quick start**, a "Mobile app (Android)" section after the web app's, covering:
  - **Prerequisites:** Flutter 3.47.6, Android SDK with `ANDROID_HOME` and `ANDROID_USER_HOME`, Java 21, and a **Google APIs** (not Google Play) AVD.
  - **Steps:**
    1. `./cert.ps1`
    2. `./stack.ps1 up`, then `cargo xtask zitadel-bootstrap`
    3. `cargo run`
    4. `./app/tool/android-dev.ps1 prepare <avd>` once, then `start <avd>` each session
    5. `flutter run --flavor development --dart-define-from-file=config/development.json` from `mobile/`
  - **Flavors and app IDs.** The test users and passwords are the ones already in the README.
  - **Physical devices:** `adb reverse tcp:44300 tcp:44300` and `adb reverse tcp:44310 tcp:44310` for USB, plus a hosts entry or LAN DNS record for `postit.local` (the hosts entry only works on rooted devices; LAN DNS works otherwise). Install the dev CA through Settings > Security > Encryption & credentials > Install a certificate > CA certificate.
  - **The opt-in smoke test** (Task 16).
  - **iOS (unverified).** On a Mac:
    1. Run `pod install`.
    2. In Xcode, duplicate the Debug and Release configurations as `Debug-development`/`Release-development`, `-qa` and `-production`, each based on its flavor xcconfig.
    3. Create schemes `development`, `qa` and `production`.
    4. Install the dev CA profile on the simulator (drag `postit-dev-ca.crt` onto it, then Settings > General > About > Certificate Trust Settings). This covers the system browser and native code.
    5. Copy `docker/shared/nginx/certs/postit-dev-ca.crt` to `mobile/assets/dev/postit-dev-ca.crt` (git-ignored). Only the Android Gradle build copies it automatically, and dio needs it on iOS too.
    6. Map `postit.local` in the Mac's `/etc/hosts`.

- [ ] **Step 2: Update `docs/deployment.md`.**
  - Replace the "Mobile custom schemes will then need Zitadel's native application type; that is a P11 concern" sentence and extend the redirect table. Per environment, create two Native applications in the IdP: `postit-android` and `postit-ios`, with PKCE, JWT access tokens, and the refresh grant. Their redirects are `<scheme>://auth/callback`, and their post-logout redirects are `<scheme>://auth/logout`. Set `POSTIT__AUTH__OIDC__ANDROID_CLIENT_ID` and `POSTIT__AUTH__OIDC__IOS_CLIENT_ID`, and include them in `allowed_clients` when that is set explicitly. Record whether dev mode is needed, per Task 5.
  - **Release signing:** the maintainer creates one keystore per environment. Give the command:

    ```
    keytool -genkeypair -v -keystore android.jks -keyalg RSA -keysize 4096 -validity 10000 -alias postit-<env>
    ```

    The keystore goes into `!ref/vault/<env>/`, with `android.env` holding `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`. Then re-pack the vault. The build reads it through `mobile/android/key.properties`.
  - Replace the "Mobile apps … will be extended then" paragraph with "Building the mobile apps from source": `flutter build apk --flavor <env> --release --dart-define-from-file=config/<env>.json`, and the internal distribution of the APK.

- [ ] **Step 3: Update `docs/security.md`.** Add four findings-register rows:
  - **Mobile refresh token** in Keystore/Keychain: accepted risk. Mitigations: revocation on sign-out, IdP rotation, `allowBackup=false`.
  - **Debug-only trust** for the dev CA, in two parts (Android `network_security_config` for native code, and a development-flavor Flutter asset loaded into a dio `SecurityContext`, because Dart ignores `network_security_config`): controlled. The CI `app` job asserts the qa and production APKs carry neither.
  - **IdP unreachable during refresh:** the stored refresh token is kept, and only an OAuth rejection clears it. Controlled; covered by the Task 8 and Task 9 tests.
  - **Separate client IDs** per platform with `allowed_clients`: controlled. Covered by the config, verifier, and end-to-end server tests from Task 1.

- [ ] **Step 4: Plan status and spec deviations.**
  - In `docs/plans/02. foundation.md`, under P11's exit criteria, add: `**Built (2026-10-16 plan):** see docs/plans/2026-10-16-p11-flutter-app.md and docs/specs/2026-10-16-p11-flutter-app-design.md.`
  - In the spec, add a "Deviations" section:
    1. iOS build configurations and schemes are created in Xcode on the first Mac session. Only the xcconfigs and Info.plist are committed.
    2. Any generator, Riverpod, or `flutter_appauth` API name that differed from this plan.

- [ ] **Step 5: Update `CLAUDE.md`.** Change "P11 (the Flutter app) is not done." to describe P11 as built: `mobile/` is a Flutter app (Android verified, iOS scaffolded) with separate web, Android, and iOS OIDC clients, `mobile/tool/android-dev.ps1`/`.sh`, and the `app` CI job. Add an "App (from `mobile/`)" commands block:

```sh
flutter pub get
dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test
flutter analyze && flutter test
flutter run --flavor development --dart-define-from-file=config/development.json
./tool/android-dev.ps1 help     # emulator setup (Google APIs image)
```

and note that `cargo xtask openapi` also regenerates `mobile/lib/api/` (needs Flutter and `flutter pub get` in `mobile/`); `--skip-dart` skips that on a machine without Flutter, but `--check` and CI never skip it. Also note that Dart's `HttpClient` ignores `network_security_config`, so development builds trust the dev CA through the `assets/dev/` flavor asset and `devTrust`.

- [ ] **Step 6: Run the gates and commit.** Run the Rust quality gates; `cargo test -p postit-config` covers the configuration reference.

```bash
git add README.md docs CLAUDE.md "docs/plans/02. foundation.md"
git commit -m "docs: P11 mobile setup, deployment, security notes and status

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 21: Exit verification

**Files:** none (verification only).

- [ ] **Step 1: Run all gates.**
  - From `backend/`: the four Rust quality gates and `cargo xtask openapi --check`.
  - From `web/` (PowerShell): the web gates.
  - From `mobile/`: the app gates (`dart format` check, `flutter analyze`, `flutter test`).

  Expected: all clean.

- [ ] **Step 2: Run the exit flow on the emulator.**
  1. Start fresh: `./stack.ps1 up`, `cargo xtask zitadel-bootstrap`, then `cargo run` from `backend/`, and `./app/tool/android-dev.ps1 start <avd>`.
  2. Run the development flavor. Sign in as `admin@postit.com`. Expected: Home with the greeting, and the Users tab.
  3. Sign out. Expected: back on Sign in. A new Sign in asks for credentials, which confirms the IdP session ended.
  4. Sign in as `member@postit.com`. Expected: Pending.
  5. Sign out, then sign in as `admin@postit.com`. In Users, Pending shows the member. Approve the member.
  6. Sign out and sign in as `member@postit.com`, or tap "Check again" if still signed in as the member on a second emulator. Expected: Home, and no Users tab.
  7. Close the app and reopen it. Expected: still signed in, restored from the stored refresh token.
  8. Turn on airplane mode, close and reopen the app. Expected: "Can't reach postit" with Retry. Turn airplane mode off and tap Retry. Expected: Home, still signed in (the stored session survived).
  9. Build and install the qa flavor in release mode (`flutter build apk --flavor qa --release --dart-define-from-file=config/qa.json`, then `adb install`). Expected: it launches and reaches the network (it shows the qa server's error or sign-in screen, not a crash or a silent hang), which proves the `INTERNET` permission ships in release builds.

- [ ] **Step 3: Report.** Tell the user, with evidence:
  - which steps passed;
  - the CI run link;
  - the spike results;
  - every deviation recorded in the spec;
  - the open follow-ups: iOS on a Mac, and the vault keystores if the maintainer has not created them yet.

Then invoke superpowers:finishing-a-development-branch.
