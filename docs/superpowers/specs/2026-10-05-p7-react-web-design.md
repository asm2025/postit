# P7: React web app — design

Date: 2026-10-05. Source: `!ref/plans/02. foundation.md` phase P7 (and its "Web client" and "Web client hosting" sections). This spec records the decisions made when P7 started and the one addition to the P6 API. Where this spec is silent, plan 02 governs.

## Goal

A working React web client at `https://postit.local:44315`, served both by the Vite dev server and by the Docker build.

- `admin@postit.com` signs in through Zitadel and lands on the dashboard.
- `member@postit.com` signs in, sees the pending screen, the admin gets the email and approves in Users, and the member reaches the dashboard after a refresh.
- `tsc --noEmit`, lint, and the unit and component tests are clean.

Out of scope: Jobs screens (P8), Playwright in CI, mobile (P11), publishing features (plan 03).

## Decisions

- **Stack** (confirmed as written in plan 02): React 19, TypeScript strict, Vite, React Router, TanStack Query, Tailwind CSS with shadcn/ui, `openapi-fetch` with `openapi-typescript`, `oidc-client-ts` through `react-oidc-context`, Vitest, Testing Library, MSW. Versions are verified at the start of implementation and pinned in `package.json` and the lockfile. `engines.node` pins a current LTS (the local Node 26 is not an LTS release).
- **Session restore after reload:** tokens stay in memory only. On load with no session the app attempts `signinSilent` (authorization code with `prompt=none` in a hidden iframe) against the Zitadel session. Failure leaves the app signed out with no redirect loop. In-session renewal uses `automaticSilentRenew` with the in-memory refresh token (`offline_access`). The iframe returns to the existing `/auth/callback` route; when the page detects it is framed it calls `signinSilentCallback()` and renders nothing else, so no extra static page or redirect URI is needed. The proxy CSP for the web origin therefore uses `frame-ancestors 'self'`.
- **Role filter added to `GET /api/v1/users`:** an optional `role` query parameter (`admin` | `member`). It is an additive change within `/api/v1`. It touches the users repository filter in `postit-data`, the handler and its OpenAPI parameter in `postit-api`, and the regenerated `api/openapi.json`. Tests cover the filter alone and combined with `status` and `search`. It exists so Home can show an "only one active admin" warning.

## Server and tooling

- **Static hosting:** replace the stub in `server/crates/server/src/lib.rs` (the `server.web.enabled` warning). A `web` module serves `server.web.root` with `tower-http` `ServeDir` and an `index.html` fallback. It binds `server.web.bind` as its own listener (44315 in development, 8082 in the container). When `bind` is unset it is nested at `/` on the API router, with `/api`, `/docs`, `/health`, and `/ready` excluded. Not embedded in the binary, so the Rust build stays independent of Node.
- **Caching:** `/assets/*` is `public, max-age=31536000, immutable`; `index.html` and `config.json` are `no-cache`.
- **`GET /config.json`:** returns `{ "API_BASE_URL", "ENV" }` from `server.web.api_base_url` and the environment.
- **`cargo xtask openapi`:** writes `api/openapi.json` and also runs `openapi-typescript` into `web/src/api/schema.d.ts`. `--check` diffs both. If Node dependencies are missing the command fails with a message naming the step to run.
- **Docker:** a Node stage runs `npm ci` and the production build from the lockfile, and copies `dist` to `/srv/web` in the runtime stage. `postit-nginx-app` maps 44315 to the server's web port 8082. Development compose sets `server.web.enabled`, `root`, and `bind` through environment overrides. Native development keeps `server.web.enabled` off and uses a `web/public/config.json` served by Vite.
- **Rust tests:** SPA fallback for deep links, cache headers per path class, `/config.json` content, and that excluded prefixes are not shadowed.

## `web/` structure

`web/src/`:

- `api/`: generated `schema.d.ts` (never edited by hand) and `client.ts` (the `openapi-fetch` instance with the auth middleware).
- `config/`: loads `/config.json`, then `GET /api/v1/auth/config`, before the app renders. A load failure shows a plain error screen.
- `auth/`: `UserManager` factory, `AuthProvider`, `useSession`, route guards.
- `features/`: `me`, `users`, `audit`, `home`, each with query hooks and screens.
- `components/ui/` (shadcn), `routes.tsx`, `theme/`, `errors/`.

## Auth and HTTP

- Authorization code with PKCE as a public client. Redirect URI `/auth/callback`; after sign-in the saved return path replaces the URL. Only short-lived PKCE state touches sessionStorage (oidc-client-ts needs it across the redirect). Tokens are never persisted.
- Sign-out calls the end-session endpoint with the web origin as post-logout redirect.
- The `openapi-fetch` middleware attaches `Authorization: Bearer`. On 401 it requests one renewal through a shared single-flight promise, retries the request once, then signs out.
- Problem+json responses become `ApiError { status, code, detail }`. A `code` to message map covers the P6 codes; an unknown code shows the server `detail`.

## Gating (from `GET /me`)

- Signed out: Sign-in page.
- `pending`: pending screen with a refresh button and sign-out. `disabled`: disabled screen. `deleting`: the same kind of screen with a deletion message.
- `active`: dashboard shell. `/users` and `/audit` require `role === 'admin'`; members get a 403 page.
- An `account_pending` or `account_disabled` response mid-session invalidates the `/me` query and moves the user to the matching screen.

## Screens

- **Shell:** sidebar on wide screens, drawer on narrow. Admin navigation shows a pending-count badge (`GET /users?status=pending&page_size=1`, reading `total`).
- **Home (role-aware, one component):**
  - Admin panels:
    1. *Needs approval:* up to 5 pending users (name, email, time waiting) with inline Approve and Reject. Reject confirms because it deletes. Empty state "No one waiting". "View all" opens Users filtered to pending.
    2. *Users at a glance:* counters for active, pending, disabled, each linking to Users with that filter, from `page_size=1` calls reading `total`.
    3. *Recent activity:* the last 8 audit events (kind, actor, subject, relative time), with "View all" to Audit.
    4. *Warning:* shown only when exactly one active admin exists (`role=admin&status=active`, `total === 1`): "Only one active admin".
  - Member: a single *Your account* card (display name, email, role, status, IdP account link when configured, environment badge). Nothing else; plan 03 phases D4 and D5 add their panels to this page.
  - Each panel loads independently with its own skeleton and error-with-retry state. Queries poll every 30 s while the tab is visible. Approve and Reject invalidate the pending list, the counters, the badge, and recent activity. Members never call admin endpoints.
- **Profile:** IdP-owned name and email read-only, an account-URL link when configured, sign-out, and "Delete my postit account" behind a type-your-display-name dialog (`DELETE /me`, 202, then sign out).
- **Users (admin):** table with search, status filter, role filter, pagination. Actions: approve, disable, re-enable, change role, delete (reject for pending). Destructive actions confirm. `last_admin` and `user_deleting` appear as inline messages.
- **Audit (admin):** paginated events with filters for kind, time range, and user ID. Pseudonymized IDs render as "deleted user".
- **Theme:** light, dark, system, via a class on `<html>`; the preference (not a token) is kept in localStorage with try/catch.

## Testing

- **Vitest:** auth state, middleware (single-flight renewal, retry once, sign-out), config loader, error mapping, with mocked fetch and a fake `UserManager`.
- **Testing Library + MSW:** route guards (signed out, pending, disabled, active, admin); Home for member and admin, approving from Home updating list and counters, one panel failing in isolation, empty states, the single-admin warning; Users actions and filters; Audit filters; the Profile delete dialog.
- **Rust:** the web hosting tests above and the `role` filter tests.
- **Playwright smoke** against the dev stack with Zitadel: written, opt-in, not in CI.
- **Gates:** `cargo fmt --check`, clippy with `-D warnings`, `cargo test --workspace`, `cargo xtask openapi --check`; in `web/`: `tsc --noEmit`, lint, Vitest, `vite build`. Docker build serves the app on 44315 behind `postit-nginx-app`.

## Exit criteria

As plan 02 P7, plus: Home shows the panels above for each role, and `GET /users?role=` works and is in the committed `api/openapi.json`.
