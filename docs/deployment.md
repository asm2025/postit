# Deployment guide

This guide is for the person who runs postit. It does not assume a particular host. What postit ships is a container image and a set of environment variables; where the containers run is up to you. The Compose files in `docker/` are one reference deployment (a single machine) and the way to reproduce qa or production locally. Cloud-specific infrastructure is planned separately, once a provider is chosen.

For every setting and variable, see the [configuration reference](configuration.md). This guide does not repeat it. For the threat model and the accepted risks, see [security](security.md).

Contents:

1. [What you deploy](#what-you-deploy)
2. [Where it can run](#where-it-can-run)
3. [First boot (production)](#first-boot-production)
4. [Identity provider](#identity-provider)
5. [TLS, reverse proxy and security headers](#tls-reverse-proxy-and-security-headers)
6. [Web hosting options](#web-hosting-options)
7. [Database](#database)
8. [Backups](#backups)
9. [Upgrades](#upgrades)
10. [Reproducing qa or production locally](#reproducing-qa-or-production-locally)
11. [Building from source](#building-from-source)

## What you deploy

One image, `postit-server`, built from `docker/server.Dockerfile` with the repository root as the build context. Tag it with a version and the git commit, for example `postit-server:<version>` and `postit-server:<git-sha>`. The same image goes from development to qa to production; only configuration changes.

The process runs in one of three roles, chosen by `POSTIT_ROLE`:

- `api` serves the HTTP API on port 8080. The image sets `POSTIT__SERVER__API_PORT=8080` and `POSTIT__SERVER__WORKER_PORT=8081`, so these are the ports whether or not Compose is involved (a binary run outside the image defaults to 44310 and 44311). When `server.web.enabled` is true it also serves the web app, on port 8082 if `server.web.bind` is set. Serving the web app also needs `server.web.root`: the image sets it to `/srv/web`, and `production.toml` sets `server.web.enabled`.
- `worker` runs background jobs and serves `/health` and `/ready` on port 8081.
- `all` runs both in one process. It exists for development.

Outside development, run `api` and `worker` as two services (or tasks) from the one image. Both are required: the API accepts work, the worker does it.

**Database URL.** `qa.toml` and `production.toml` set `database.url` to `postgres://postit-postgres:5432/postit`, the Compose service name. A host without Compose must set `POSTIT__DATABASE__URL` to its own Postgres, without credentials.

The container speaks plain HTTP. Something in front of it must terminate TLS (see [TLS, reverse proxy and security headers](#tls-reverse-proxy-and-security-headers)).

**Health checks.** The image's `HEALTHCHECK` runs `postit healthcheck`. For a load balancer's target health, use the API's `/ready` on port 8080. The two probes mean different things:

- `/health` says the process is alive.
- `/ready` says the process can do its job. On the API, that is: the database is reachable and the identity provider's signing keys (JWKS) are loaded. On the worker: the database is reachable and the job storage is migrated. The worker does not need the JWKS.

**Graceful shutdown.** On SIGTERM the server stops accepting work and drains in-flight requests and jobs for up to `server.shutdown_timeout`, which defaults to 30 seconds. Give your orchestrator a stop timeout above that so it does not kill the process mid-drain. The Compose files use 40 seconds (`stop_grace_period`). On ECS the equivalent is `stopTimeout`; on Azure Container Apps it is the termination grace period.

## Where it can run

Any host that can run containers:

- A single VM using the qa or production Compose files. This is the reference deployment.
- AWS ECS with Fargate, or AWS App Runner.
- Azure Container Apps.
- Kubernetes.

Each one needs the same four things:

1. Two services from the one image, `api` and `worker`.
2. A managed or self-run Postgres 18.
3. Secrets delivered as environment variables or as files (the `_FILE` variables described in the configuration reference).
4. TLS in front of the API and the web app.

Vercel and similar static hosts can at most serve the web build. The image already serves it, so they are not needed.

For a host that does not use the Compose files, [`deploy/env/qa.env.example`](../deploy/env/qa.env.example) and [`deploy/env/production.env.example`](../deploy/env/production.env.example) list the environment variables each role needs, with the `_FILE` forms for secrets.

### Single host with Compose

1. Provide the secrets. Extract `!ref/vault.7z` (the password comes from the maintainer), or create `!ref/vault/<env>/postgres.env` and `!ref/vault/<env>/postit.env` yourself with the keys `./stack.sh doctor` checks. The README's [Environment files and secrets](../README.md#environment-files-and-secrets) section describes the layout.
2. Copy `docker/.env.example` to `docker/.env` and fill in the values. Production requires six of them (see [Production Compose variables](configuration.md#production-compose-variables)).
3. Set `POSTIT_IMAGE` in `docker/.env` to the image tag you are deploying.
4. `./stack.sh doctor <env>` lists anything still missing. It prints no values.
5. `./stack.sh up <env>`.
6. Configure the reverse proxy and TLS (see [TLS, reverse proxy and security headers](#tls-reverse-proxy-and-security-headers)).
7. Run the trusted-proxy check described in that section.

To stop only the server, for example before a restore, run `docker stop postit-api postit-worker`. `./stack.sh down` stops Postgres too. To roll an upgrade, recreate the API first and then the worker: `./stack.sh up production postit-api`, then `./stack.sh up production postit-worker`. The launcher passes service names through to Compose.

## First boot (production)

Production refuses to start unless the required values are present. They are listed in the configuration reference under [Required outside development](configuration.md#required-outside-development): the OIDC issuer (an `https` URL), at least one accepted audience, a database password, an SMTP host, a mail sender, TLS for SMTP, and the audit pseudonymisation key. A blank value counts as missing.

**The first admin.** A new user signs in as `pending` until an admin approves them, so someone has to be admin from the start. Set one of these on the first boot:

- `auth.bootstrap.admin_email` (`POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL`), the address of an account whose email the identity provider reports as verified.
- `auth.bootstrap.admin_subject` (`POSTIT__AUTH__BOOTSTRAP__ADMIN_SUBJECT`), the identity provider's `sub` for that person.

Set at most one of them. Production also refuses to start when the database has no active admin and neither setting is set. Once an admin exists you may leave the variable blank. On the compose deployment the variable is `POSTIT_BOOTSTRAP_ADMIN_EMAIL` in `docker/.env`.

**Migrations.** Every start runs postit's migrations and then the job storage's migrations, each under a Postgres advisory lock. When `api` and `worker` start at the same time, one waits for the other, so concurrent starts are safe.

**What the startup log shows.** Each process logs a `starting postit` line carrying the configuration it loaded, with the password and key settings shown as `[redacted]`, then `postit is listening` with the bound addresses. Check the dump against what you meant to set. Two things print in clear text there: `database.username` and `mail.smtp.username`. Those are secrets by policy but are not masked, so treat startup logs accordingly.

## Identity provider

postit does not bundle an identity provider outside development. Run or subscribe to any OIDC provider that meets these requirements.

**Generic requirements**

- OIDC discovery works at the issuer URL.
- Access tokens are JWTs, not opaque tokens, and their `aud` claim includes a value listed in `auth.oidc.audiences`.
- A public client (no client secret) that uses the authorization code flow with PKCE and can obtain refresh tokens. postit requests the `offline_access` scope, among others.
- Redirect URIs registered for each client surface:
    - Web app: `https://<app host>/auth/callback`
    - Swagger UI, where `/docs` is enabled: `https://<api host>/docs/oauth2-redirect.html`
    - Mobile (the native applications below): `postit-qa://auth/callback` and `postit://auth/callback`
- Post-logout redirect URIs: the web origin (with a trailing slash), and the mobile schemes.
- The provider must support `prompt=none` on the authorization endpoint and the refresh-token grant with `offline_access`. The web app never frames the provider: a reload restores the session with one top-level redirect, so third-party cookie blocking does not affect it.
- Every client that calls the API (the web and mobile apps share `auth.oidc.client_id`) must be in `auth.oidc.allowed_clients`; the API refuses tokens issued to any other client of the same project.
- `email` and `email_verified` claims, if you use `admin_email` for the bootstrap admin.

**Zitadel production setup**

These are the steps for a production Zitadel. They are derived from what `cargo xtask zitadel-bootstrap` does for development; read `server/xtask/src/zitadel_bootstrap.rs` for the exact API calls.

1. Create or choose an organization.
2. In it, create a project named `postit`. Use the project's ID as the audience: it goes in `POSTIT_OIDC_AUDIENCE` (or `auth.oidc.audiences`).
3. In the project, create an application of type user agent (a browser-style public client):
    - Response type code, grant types authorization code and refresh token.
    - Authentication method none (PKCE, no secret).
    - Access token type **JWT**. The default is opaque, which postit cannot verify.
    - The redirect and post-logout URIs below.
    - Its client ID is `auth.oidc.client_id` (the default is `postit-app`).
4. Turn development mode off for the application. Zitadel only accepts non-HTTPS redirect URIs while it is on; the mobile apps need no such exception, because they are native applications (next step).
5. For the mobile apps, create two more applications of type **Native** in the same project: `postit-android` and `postit-ios`. Each uses authorization code with PKCE (no secret), grant types authorization code and refresh token, and access token type **JWT**. Their redirect URI is `<scheme>://auth/callback` and their post-logout redirect URI is `<scheme>://auth/logout`, with the scheme from the table below. Set `POSTIT__AUTH__OIDC__ANDROID_CLIENT_ID` and `POSTIT__AUTH__OIDC__IOS_CLIENT_ID` to their client IDs, and list them in `auth.oidc.allowed_clients` when you set that explicitly. Native applications did not need development mode in the spike (`devMode: false` with a custom-scheme redirect was accepted), so qa and production keep it off.
6. Create the admin user with a verified email address, and set `auth.bootstrap.admin_email` to it.
7. In the organization's login policy, set "User registration allowed" to match whether people may sign up on their own (they still start `pending` until an admin approves them), and decide whether to force a second factor. The development bootstrap turns registration on unless `ZITADEL_ALLOW_REGISTRATION=false`, never forces a second factor, and never prompts to set one up.

Redirect URIs, per environment and platform (using the default hostnames; substitute yours):

| Environment | Web redirect | Swagger redirect | Mobile redirect | Post-logout |
| --- | --- | --- | --- | --- |
| qa | `https://app.qa.postit.com/auth/callback` | `https://api.qa.postit.com/docs/oauth2-redirect.html` | `postit-qa://auth/callback` | `https://app.qa.postit.com/`, plus the mobile scheme |
| production | `https://app.postit.com/auth/callback` | `https://api.postit.com/docs/oauth2-redirect.html` | `postit://auth/callback` | `https://app.postit.com/`, plus the mobile scheme |

`cargo xtask zitadel-bootstrap` automates the development equivalent against the bundled Zitadel. It sets development mode on, which production must not.

**If the identity provider goes down.** Nobody can sign in, but tokens already issued keep working until they expire, because the signing keys are cached.

## TLS, reverse proxy and security headers

Put a TLS-terminating proxy or load balancer in front of the API and the web app. The server sends the web app's security headers itself (`Content-Security-Policy`, `X-Content-Type-Options`, `X-Frame-Options` and `Referrer-Policy`), so the proxy adds only HSTS. Here is a Caddy example for one host serving `api.` and `app.`:

```caddyfile
api.postit.com {
    reverse_proxy 127.0.0.1:8080
}

app.postit.com {
    header Strict-Transport-Security "max-age=31536000"
    reverse_proxy 127.0.0.1:8082
}
```

The server derives the Content-Security-Policy from `server.web.api_base_url` and the issuer; set `server.web.content_security_policy` to replace it. With the default hostnames it is:

```text
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' https://api.postit.com https://auth.postit.com; frame-src 'none'; frame-ancestors 'none'; form-action 'self'; base-uri 'self'; object-src 'none'
```

What each part allows:

- `connect-src 'self' https://api.postit.com https://auth.postit.com`: the web app calls its own origin, the API, and the identity provider (token, refresh and revocation requests).
- `frame-src 'none'` and `frame-ancestors 'none'`: sign-in never uses an iframe (renewal is by refresh token, and a reload restores the session with a top-level redirect), so the app frames nothing and nothing may frame it.
- `form-action 'self'`: forms submit only to the app itself.
- `style-src 'unsafe-inline'`: the UI components set style attributes. No inline script is needed, so `script-src` stays `'self'`.

On a cloud load balancer or CDN (for example ALB, Front Door or Application Gateway, depending on your provider) add HSTS on the listener rules or the CDN.

**Trusted proxies.** `server.trusted_proxies` must list exactly the hop that sets `X-Forwarded-For`, because the server uses it for rate limiting and the audit log. If the list is too narrow, every client shares one rate-limit bucket and one audit IP. If it is too wide, clients can spoof their address.

- On the compose deployment, the proxy runs on the host and reaches the published loopback ports through the gateway of the `postit-net` network, `172.30.0.1/32`. That is the default (`POSTIT_TRUSTED_PROXIES` overrides it).
- Behind a cloud load balancer, list the load balancer's subnet CIDRs.

Two caveats on the compose default:

- The gateway address is reached by any process on the host, not only your reverse proxy. Any local process that can connect to the published ports can therefore set `X-Forwarded-For` and be believed. This is acceptable only if you trust every process on that machine.
- Docker setups where connections do not arrive from the gateway, such as rootless Docker or `userland-proxy=false`, can present a different source address. If the real address is different, set `POSTIT_TRUSTED_PROXIES` to it. The server does not log client addresses, so check another way. Run `docker network inspect postit_postit-net` and compare its gateway with `POSTIT_TRUSTED_PROXIES`. Then, through your proxy, do something that writes an audit event: approve a pending user (`PATCH /api/v1/users/{id}` with `{"status":"active"}`) or let a new user sign in for the first time. A returning user's sign-in writes no audit event, so it would show a stale address. Read that event specifically, for example `GET /api/v1/admin/audit?kind=user_approved` (admin only) and its `ip` field, or the `ip` column of `audit_events` through `stack psql`. It should be your real client address. If it is the gateway or your proxy's address, the trusted-proxy list is wrong. (The web Audit screen does not show the address.) Audit addresses are kept for `audit.ip_retention`.

## Web hosting options

The web app can be served in two ways.

1. **A separate app host** (the default in the compose files). The API process listens for the web app on its own port, 8082, set by `server.web.bind`. Your proxy sends `app.<domain>` to that port and `api.<domain>` to port 8080. This needs CORS: `cors.allowed_origins` must contain the app origin. The production compose file sets it from `POSTIT_APP_URL`.
2. **The web app on the API origin.** Leave `server.web.bind` unset (with `server.web.enabled` true and `server.web.root` set, the image sets `server.web.root` and `production.toml` sets `server.web.enabled`) and the web app is served at `/` on the API port, with `/api`, `/docs`, `/health` and `/ready` left to the API. No CORS is needed because the app and API share an origin.

Either way, the page loads `/config.json` at runtime, which carries `API_BASE_URL` (from `server.web.api_base_url`) and `ENV`. One web build therefore serves every environment. Caching is set by the server: files under `/assets` are immutable with a one-year lifetime, while `index.html` and `config.json` are `no-cache`. Unknown paths fall back to `index.html`, so deep links work.

## Database

postit needs PostgreSQL 18. It connects as the owner of its database and runs its migrations with that role.

**Privileges.** The role needs `CREATE` on the database, because the job storage keeps its tables in an `apalis` schema, and the ability to create the `citext` extension. `citext` is a trusted extension, so the database owner can create it without superuser rights.

**Managed services.**

- Amazon RDS and Azure Database for PostgreSQL Flexible Server both offer `citext`.
- On Azure, add `citext` to the server parameter `azure.extensions` before the first start.
- If your platform does not let the owner create it, create it once with the admin role, in the postit database: `CREATE EXTENSION IF NOT EXISTS citext;`. The migration's own `IF NOT EXISTS` then passes.

**Credentials.** `database.url` carries no credentials; the server rejects one that does. The username and password are secrets, supplied as `POSTIT__DATABASE__USERNAME` and `POSTIT__DATABASE__PASSWORD` (or their `_FILE` forms).

**Connections.** Each process opens up to `database.max_connections` connections (default 10). Size the database server's connection limit for (api tasks + worker tasks) x 10, plus headroom for your own tools.

## Backups

**Compose deployments.** Two commands, in `stack.sh` and `stack.ps1`:

```sh
./stack.sh backup [environment]
./stack.sh restore [environment] <file>
```

(`./stack.ps1` takes the same arguments.) Both have safety rails:

- Both act on the running `postit-postgres` container, whatever environment you typed. The launcher reads the environment that container was started for (from its Compose config-file label) and refuses when it differs from the one you asked for, or when it cannot tell. The production `--force` check on restore follows the running environment too.
- A backup is written to `!ref/backups/postit-<environment>-<UTC timestamp>.dump`. `POSTIT_BACKUP_DIR` changes the directory. A dump is unencrypted user data, so a directory inside this repository must be ignored by git; the launcher refuses otherwise, checked on the absolute path. A directory outside the repository's work tree (or when there is no git work tree) is allowed, because git cannot commit it; the launcher prints a one-line note and still makes it owner-only.
- The directory and the dump are created with owner-only permissions.
- A restore runs `pg_restore` as one transaction (`--single-transaction --exit-on-error`), so a failed restore leaves the database as it was.
- A restore only accepts a file whose name is `postit-<environment>-*.dump`, so a qa dump cannot be restored into production by accident.
- A restore refuses to run while a postit server container is running.
- On production, restore needs `--force` (`-Force` in PowerShell). It does not start anything afterwards; start the stack yourself.

**A natively run server is invisible to the check.** The running-server check only looks at Docker containers (`docker ps`). If you started a server with `cargo run`, stop it by hand before restoring, or it will keep writing to a half-restored database.

**A dump is unencrypted user data.** Keep it out of git, store it somewhere access-controlled, and delete it when you are done with it.

**Managed Postgres.** Rely on the provider's automated snapshots and point-in-time recovery. Still take a logical dump before an upgrade.

**What a dump does not contain:**

- The vault, that is, your secrets. Back those up separately.
- The identity provider's data. Zitadel has its own database.
- Media files, once plan 03 adds them.

**Restore order.** Stop `api` and `worker` (on Compose, `docker stop postit-api postit-worker`; `stack down` would stop Postgres too), restore, then start them. Test a restore regularly; a backup you have never restored is not yet a backup.

## Upgrades

**Promote one image.** Build once, run it in qa, and promote the same tag to production. On Compose that is `POSTIT_IMAGE` in `docker/.env`; elsewhere it is the image in the task definition or revision. Never rebuild for production.

**Order.**

1. Back up (see above).
2. Roll the `api` service first. It runs the migrations.
3. Roll the `worker`.

**Why migrations only add.** While the roll is in progress, the new `api` has migrated the schema but the old `worker` is still running against it. So the previous release must keep working with the new schema. That is the expand-then-contract rule: a release only adds things (tables, nullable or defaulted columns, indexes), and removals or renames happen in a later release, once no running version reads the old shape. The migration review checklist in the [pull request template](../.github/pull_request_template.md) enforces it.

**API compatibility.** Changes under `/api/v1` are additive. The CI `contract` job compares `api/openapi.json` with the newest `v*` tag and fails on a breaking change. With no such tag yet, it skips the comparison and prints a notice.

**Rollback.** Roll the image back. That is safe because the previous release works with the expanded schema. Restore a backup only if data itself went wrong.

## Reproducing qa or production locally

On a machine with Docker, the compose files reproduce either environment:

```sh
./stack.sh doctor production     # checks the vault files and required values; prints no values
./stack.sh up production
```

For production, set the required variables in `docker/.env` first (copy `docker/.env.example`). `stack doctor` lists what is missing. `qa` works the same way with `stack up qa`.

One host runs one environment. They share the same Compose project, container names and database volume, so run `stack reset <env>` when you switch from one to another. That wipes the database.

The CI `docker` job is the automated version of this: it runs the image as separate `api` and `worker` containers against Postgres and a stub identity provider, and probes `/health`, `/ready` and `/config.json`.

## Building from source

**Server.** From `server/`:

```sh
cargo build --profile dist -p postit-server
```

`SQLX_OFFLINE=true` is the default, set in `server/.cargo/config.toml`, so no database is needed to build.

**Image.** From the repository root:

```sh
docker build -f docker/server.Dockerfile -t postit-server:<version> -t postit-server:$(git rev-parse --short HEAD) .
```

**Web app alone.** You need Node 24. In `web/`:

```sh
pnpm install --frozen-lockfile
pnpm build
```

The output is `web/dist`. Serve it from any static host that falls back to `index.html` for unknown paths and answers `/config.json` as described under [Web hosting options](#web-hosting-options).

**Mobile apps from source.** From `app/`, with Flutter 3.47.6 and Java 21:

```sh
flutter build apk --flavor <env> --release --dart-define-from-file=config/<env>.json
```

`<env>` is `qa` or `production`. The APK is `app/build/app/outputs/flutter-apk/app-<env>-release.apk`; distribute it internally (for example through a download link or an MDM). There is no store publishing yet, and the iOS build needs a Mac (see the README).

**Release signing.** The maintainer creates one keystore per environment:

```
keytool -genkeypair -v -keystore android.jks -keyalg RSA -keysize 4096 -validity 10000 -alias postit-<env>
```

The keystore goes into `!ref/vault/<env>/` next to `android.env`, which holds `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`. Then the maintainer re-packs the vault. The build reads it through `app/android/key.properties` (copy `key.properties.example`, git-ignored). Without it, qa and production builds are signed with the debug key and print a warning; never distribute those.
