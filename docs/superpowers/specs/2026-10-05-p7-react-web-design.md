# P7: React web app — design

Date: 2026-10-05, revised 2026-10-08 after review. Source: `!ref/plans/02. foundation.md` phase P7 (and its "Web client" and "Web client hosting" sections). This spec records the decisions made when P7 started and the additive changes to the P6 API. Where this spec is silent, plan 02 governs.

## Goal

A working React web client at `https://postit.local:44315`, served both by the Vite dev server and by the Docker build.

- `admin@postit.com` signs in through Zitadel and lands on the dashboard.
- `member@postit.com` signs in, sees the pending screen, the admin gets the email and approves in Users, and the member reaches the dashboard after a refresh.
- `tsc --noEmit`, lint, and the unit and component tests are clean.

Out of scope: Jobs screens (P8), Playwright in CI, the `web` CI job (P9), mobile (P11), publishing features (plan 03).

## Decisions

- **Stack** (confirmed as written in plan 02): React 19, TypeScript strict, Vite, React Router, TanStack Query, Tailwind CSS with shadcn/ui, `openapi-fetch` with `openapi-typescript`, `oidc-client-ts` through `react-oidc-context`, Vitest, Testing Library, MSW, ESLint (flat config, `typescript-eslint` strict type-checked, `react-hooks`) and Prettier. Versions are verified at the start of implementation and pinned exactly in `package.json` and the lockfile. `engines.node` and `.nvmrc` pin the Node release that is Active LTS at implementation start: Node 24 until Node 26 enters LTS (expected late October 2026), then Node 26.
- **Session restore after reload:** tokens stay in memory only. On load with no session the app attempts `signinSilent` (authorization code with `prompt=none` in a hidden iframe, 5 s timeout) against the IdP session. `login_required`, `interaction_required`, or a timeout leaves the app signed out with no redirect loop. In-session renewal uses `automaticSilentRenew` with the in-memory refresh token (`offline_access`, already in the default `auth.oidc.scopes`). The iframe returns to the existing `/auth/callback` route; when the page detects it is framed (`window.self !== window.top`) it calls `signinSilentCallback()` and renders nothing else, so no extra static page or redirect URI is needed.
    - **IdP requirements.** The IdP's login pages must allow framing by the web origin, and its session cookie must reach the iframe. Zitadel blocks iframe embedding by default, so `cargo xtask zitadel-bootstrap` also sets the instance security policy idempotently (`enableIframeEmbedding: true`, `allowedOrigins: ["https://postit.local:44315"]`). This is verified against the dev stack before the auth work starts. The IdP and the web app must be same-site (true for `postit.local:44300`/`:44315` and for `auth.*`/`app.*` under one domain); with a cross-site IdP, browsers that block third-party cookies make restore fail, and the user signs in again with one click. The README states this for generic OIDC providers.
    - **CSP amendment.** Plan 02 ("QA and production hosting") specifies `frame-ancestors 'none'` for the web app. P7 changes it to `frame-ancestors 'self'` (the app frames its own callback) and requires `frame-src` and `connect-src` to include the IdP origin, and `connect-src` the API origin. P7 updates that plan 02 line. In development, `postit-nginx-app` sets the same CSP on 44315 so the Docker build exercises it; the Vite dev server sets none.
- **Additive changes to the P6 API** (all within `/api/v1`, regenerated into `api/openapi.json`):
    1. **`role` filter on `GET /api/v1/users`:** optional `role` (`admin` | `member`). Touches the users repository filter in `postit-data`, the handler and its OpenAPI parameter in `postit-api`. Tests cover the filter alone and combined with `status` and `search`. Home uses it for the "only one active admin" warning.
    2. **`sort` on `GET /api/v1/users`:** optional `sort` (`created_at` | `-created_at`, default `-created_at`, today's order, with `id` as the tie-breaker in the same direction). Unknown values are 422 `validation_failed`. Tests cover both directions and the default. Home uses `sort=created_at` to show the longest-waiting pending users.
    3. **`display_name` on `UserRef`:** `UserRef` (in `actor`, `owner`, `subject`, and every `*_user_id` details value of an audit event) gains `display_name: string | null`. The audit repository resolves names with one `LEFT JOIN`/batch lookup per page, never per row. It is `null` for pseudonymized IDs and for users no longer present. Tests cover a live user, a pseudonymized ID, and a details key.
    4. **`AuditEventKind` in OpenAPI:** the `kind` query parameter and `AuditEventDto.kind` use a `ToSchema` string enum generated from `AuditEventKind`, so the web filter is typed and cannot drift. Clients must tolerate unknown values (plan 02, API compatibility), so the web app renders an unknown kind as its raw string.

## Server and tooling

- **Static hosting:** replace the stub in `server/crates/server/src/lib.rs` (the `server.web.enabled` warning) with a `web` module.
    - Only roles `all` and `api` serve it. Role `worker` ignores `server.web`.
    - It serves `server.web.root` with `tower-http` `ServeDir` and an `index.html` fallback for any path that is not a file.
    - When `server.web.bind` is set (`host:port`; `0.0.0.0:44315` would be native, `0.0.0.0:8082` in the container) it gets its own listener, with the same rustls config as the API when `server.tls.enabled`, and joins graceful shutdown like the API listener.
    - When `bind` is unset it is merged at `/` on the API port as a fallback. Paths under `/api`, `/docs`, `/health`, and `/ready` are never answered by it: an unknown `/api/v1/...` path still gets the API's problem+json 404.
    - The web router sits **outside** the API middleware stack in both modes: no rate limiting, auth, CORS, body limit, or request timeout applies to static files or `/config.json`. Request ID and tracing still apply.
- **Config validation** (in `postit-config`, failing startup with the config key in the message): `server.web.enabled = true` requires `root`; `root/index.html` must exist at startup for roles that serve the web app; `bind` must parse as a socket address. `api_base_url` defaults to `server.public_url` when unset.
- **Caching:** `/assets/*` is `public, max-age=31536000, immutable`; `index.html` (including the fallback) and `config.json` are `no-cache`; other root files (favicon and the like) get `no-cache` too.
- **`GET /config.json`:** returns `{ "API_BASE_URL": <api_base_url, no trailing slash>, "ENV": "development" | "qa" | "production" }`. It is generated, never read from `root`, so a `config.json` file in the build output is ignored.
- **`cargo xtask openapi`:** writes `api/openapi.json` and then runs `openapi-typescript` from `web/node_modules` into `web/src/api/schema.d.ts`.
    - It runs `node web/node_modules/openapi-typescript/bin/cli.js` directly (no `npx`, so Windows needs no `.cmd` shim). It finds `node` on `PATH`.
    - The generator version is pinned exactly in `web/package.json`, and its output is written with LF line endings, so the file is byte-stable across platforms. `schema.d.ts` is excluded from Prettier and ESLint.
    - `--check` generates both files in memory (the TypeScript one through a temp file) and diffs them after normalizing CRLF to LF, as the JSON check already does.
    - If `node` or `web/node_modules` is missing, the command fails with a message naming the fix (`npm ci` in `web/`). This makes `cargo xtask openapi --check` depend on Node; the README's prerequisites and the P9 `contract` CI job note it.
- **Docker:**
    - `docker/server.Dockerfile` gains a `node:<pinned LTS>-slim` stage that copies `web/package.json` and `web/package-lock.json`, runs `npm ci`, then copies `web/` and runs `npm run build`. The runtime stage copies `dist` to `/srv/web` and sets `POSTIT__SERVER__WEB__ROOT=/srv/web`, so any deployment that enables the web app (qa's compose file already sets `POSTIT__SERVER__WEB__ENABLED`) passes the `root` validation without a compose change; the qa and production compose files stay P9's. The Dockerfile header comments drop "still to come".
    - `.dockerignore` adds `web/node_modules`, `web/dist`, and `web/coverage`; `app` stays excluded.
    - `docker-compose.development.yml` sets on `postit-server`: `POSTIT__SERVER__WEB__ENABLED: "true"`, `POSTIT__SERVER__WEB__ROOT: /srv/web`, `POSTIT__SERVER__WEB__BIND: "0.0.0.0:8082"`, and `POSTIT__SERVER__WEB__API_BASE_URL: https://postit.local:44310`. It drops the `html` mount from `postit-nginx-app`, and `docker/shared/nginx/html/placeholder.html` is deleted.
    - `docker/shared/nginx/app.conf`: the 44315 server proxies to `http://postit-server:8082` with the same proxy headers as 44310 and adds the CSP above and HSTS. The comments that call 44315 a placeholder are updated.
- **Native development:** `development.toml` keeps `server.web.enabled` off. `web/public/config.json` holds `{ "API_BASE_URL": "https://postit.local:44310", "ENV": "development" }`. `vite.config.ts` serves HTTPS with `../docker/shared/nginx/certs/postit.local.crt` and `.key`, `host: "postit.local"`, `port: 44315`, `strictPort: true`; a missing certificate fails with a message pointing at `cert.ps1` / `cert.sh`.
- **Rust tests:** SPA fallback for deep links (`/auth/callback`, `/users/abc`); cache headers per path class; `/config.json` content, its default `API_BASE_URL`, and that a `config.json` file in `root` is ignored; excluded prefixes are not shadowed (`/api/v1/nope` is problem+json 404, `/health` still answers); static requests are not rate-limited (more requests than the unauthenticated burst all succeed); role `worker` serves nothing; config validation errors for a missing `root` and a missing `index.html`.

## `web/` structure

`web/src/`:

- `api/`: generated `schema.d.ts` (never edited by hand) and `client.ts` (the `openapi-fetch` instance with the auth middleware).
- `config/`: loads `/config.json`, then `GET /api/v1/auth/config`, before the app renders. A load failure shows a plain error screen with a retry button.
- `auth/`: `UserManager` factory, `AuthProvider`, `useSession`, route guards, the callback route.
- `features/`: `me`, `users`, `audit`, `home`, each with query hooks and screens.
- `components/ui/` (shadcn), `routes.tsx`, `theme/`, `errors/`.

`web/e2e/` holds the Playwright smoke test, run with `npm run e2e` against the dev stack (opt-in, needs the stack up and the seeded users' passwords from the README).

## Auth and HTTP

- Authorization code with PKCE as a public client. Redirect URI `/auth/callback`; after sign-in the saved return path replaces the URL. Only short-lived PKCE state touches sessionStorage (oidc-client-ts needs it across the redirect). Tokens are never persisted.
- **Callback errors:** an IdP error response (`access_denied` and the like) or a state mismatch shows the Sign-in page with a short message and the IdP's `error_description` when present, then clears the URL. It never retries automatically.
- **Sign-out** (user-initiated, and after `DELETE /me`) clears local state and calls the end-session endpoint with `post_logout_redirect_uri` set to the web origin **with a trailing slash** (`${origin}/`), which is exactly what `zitadel-bootstrap` registers (`https://postit.local:44315/`). The IdPs compare it exactly.
- **HTTP middleware:** the `openapi-fetch` middleware attaches `Authorization: Bearer`. On 401 it requests one renewal through a shared single-flight promise and retries the request once. If renewal fails or the retry is also 401, it removes the local user (`removeUser`, no end-session redirect, so the IdP session survives) and the app shows the Sign-in page with "Your session expired".
- Problem+json responses become `ApiError { status, code, detail }`. A `code` to message map covers the P6 codes; an unknown code shows the server `detail`.

## Gating (from `GET /me`)

`GET /me` answers 200 for `pending` and `active` users and 403 `account_disabled` for `disabled` and `deleting` ones (the `Auth` extractor), so the web app gates on the error code for those.

- Signed out: Sign-in page.
- `/me` 200 with `pending`: pending screen with a refresh button and sign-out.
- `/me` 403 `account_disabled`: one "Account unavailable" screen ("This account is disabled or being deleted. Contact an administrator.") with sign-out. There is no separate deleting screen: the only way a signed-in user reaches `deleting` themselves is `DELETE /me`, which signs out.
- `/me` 200 with `active`: dashboard shell. `/users` and `/audit` require `role === 'admin'`; members get a 403 page.
- An `account_pending` or `account_disabled` response mid-session invalidates the `/me` query and moves the user to the matching screen. A `forbidden` response on an admin route invalidates `/me` too, so a just-demoted admin loses the admin navigation.

## Screens

- **Shell:** sidebar on wide screens, drawer on narrow. An environment badge (from `ENV`) shows outside production. Admin navigation shows a pending-count badge (`GET /users?status=pending&page_size=1`, reading `total`).
- **Home (role-aware, one component):**
  - Admin panels:
    1. *Needs approval:* the 5 longest-waiting pending users (`status=pending&sort=created_at&page_size=5`: name, email, time waiting) with inline Approve and Reject, and the total in the header. Reject confirms because it deletes. Empty state "No one waiting". "View all" opens Users filtered to pending.
    2. *Users at a glance:* counters for active, pending, disabled, each linking to Users with that filter, from `page_size=1` calls reading `total`.
    3. *Recent activity:* the last 8 audit events (kind, actor and subject by `display_name`, "deleted user" when `deleted`, the short ID when the name is `null` otherwise, relative time), with "View all" to Audit.
    4. *Warning:* shown only when exactly one active admin exists (`role=admin&status=active&page_size=1`, `total === 1`): "Only one active admin".
  - Member: a single *Your account* card (display name, email, role, status, IdP account link when configured). Nothing else; plan 03 phases D4 and D5 add their panels to this page.
  - Each panel loads independently with its own skeleton and error-with-retry state. Queries poll every 30 s while the tab is visible. Approve and Reject invalidate the pending list, the counters, the badge, the single-admin warning, and recent activity. Members never call admin endpoints.
- **Profile:** IdP-owned name and email read-only, an account-URL link when configured, sign-out, and "Delete my postit account" behind a type-your-display-name dialog. The confirm button enables only on an exact match (no trimming or case folding, as the server compares). `DELETE /me` 202 then signs out; 409 `last_admin` shows inline.
- **Users (admin):** table with search, status filter (including `deleting`), role filter, and pagination; filters live in the URL query so Home links deep-link. Actions follow the server rules, and the UI only offers the valid ones:
  - `pending`: approve, reject (delete). No role change.
  - `active`: disable, change role, delete.
  - `disabled`: re-enable, change role, delete.
  - `deleting`: no actions, row shown muted.
  - The signed-in admin's own row has no delete (the API returns 403; the row links to Profile instead). Demoting or disabling oneself confirms with a warning that admin access ends, then invalidates `/me`.
  - Destructive actions confirm. `last_admin` and `user_deleting` appear as inline messages.
- **Audit (admin):** paginated events with filters for kind (typed from the generated enum), time range, actor user ID, and subject user ID; filters live in the URL query. Actor and subject show `display_name`; pseudonymized IDs render as "deleted user". Clicking a user sets the matching filter.
- **Theme:** light, dark, system, via a class on `<html>`; the preference (not a token) is kept in localStorage with try/catch. A small external script (`/theme-init.js`, loaded before the bundle, no inline script, so the CSP needs no `unsafe-inline` or hash) applies the class before first paint.

## Testing

- **Vitest:** auth state; middleware (single-flight renewal, retry once, `removeUser` without end-session on failure); config loader (success, `/config.json` failure, auth-config failure); error mapping; the framed-callback path calling `signinSilentCallback` and rendering nothing; callback error handling. Mocked fetch and a fake `UserManager`.
- **Testing Library + MSW:** route guards (signed out, pending, `account_disabled` from `/me`, active member, active admin, mid-session `account_pending`/`account_disabled`/`forbidden`); Home for member and admin, approving from Home updating list and counters, one panel failing in isolation, empty states, the single-admin warning; Users actions per status, own-row rules, and filters in the URL; Audit filters and name rendering (live, deleted, `null`); the Profile delete dialog (exact match, `last_admin`).
- **Rust:** the web hosting and config tests above; the `role` and `sort` filter tests; `UserRef.display_name` tests; the OpenAPI document exposes `AuditEventKind` as an enum.
- **Playwright smoke** (`web/e2e/`, `npm run e2e`) against the dev stack with Zitadel: admin sign-in, member pending then approved, reload keeps the session (silent restore). Written, opt-in, not in CI.
- **Gates:** `cargo fmt --check`, clippy with `-D warnings`, `cargo test --workspace`, `cargo xtask openapi --check`; in `web/`: `tsc --noEmit`, `eslint`, `prettier --check`, Vitest, `vite build`. Docker build serves the app on 44315 behind `postit-nginx-app`.

## Exit criteria

As plan 02 P7, plus:

- Home shows the panels above for each role.
- `GET /users?role=` and `?sort=`, `UserRef.display_name`, and the `AuditEventKind` enum work and are in the committed `api/openapi.json` and `web/src/api/schema.d.ts`.
- A page reload on the Docker build and on the Vite dev server keeps the admin signed in (silent restore through Zitadel).
