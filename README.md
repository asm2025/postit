# postit

## Overview

postit is a self-hosted social publishing service for a small team: a Rust REST API and
background worker with a React web client and a Flutter mobile client (Android, iOS). Users sign in through an
OIDC provider (Zitadel in development), connect their own TikTok, Facebook, Instagram,
YouTube and X accounts, draft or AI-generate posts, and publish them immediately or on a
schedule. Owners can delegate scoped access to other users.

It is **not** a CLI. The implementation plans in `!ref/plans/` are the specification — read
`01. vision and architecture.md`.

The repository is early: the server (`postit`: API, worker, identity, jobs, mail) and the
Zitadel development stack work end to end (plan 02 through phase P6); the React web app
(P7), the admin job console (P8) and the Flutter mobile app (P11) are still to come. Follow [Getting started](#getting-started)
to run it.

What lives where:

| Path                         | What it is                                                                                                     |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `server/`                    | Cargo workspace (`server/crates/*`, package names `postit-*`), `server/config/*.toml` settings layers, `xtask` |
| `web/`                       | React web client (Vite, TypeScript; from plan 02 P7)                                                           |
| `app/`                       | Flutter mobile client, Android and iOS only (from plan 02 P11)                                                 |
| `docker/`                    | Compose files, `server.Dockerfile`, and the dev-only nginx, Postgres init and Zitadel configuration            |
| `deploy/env/`                | Environment-variable reference per environment, for deployments that are not this compose                      |
| `!ref/vault.7z`              | Encrypted per-environment secrets, committed; extracted to `!ref/vault/` (git-ignored)                         |
| `stack.ps1` / `stack.sh`     | The compose launcher — see [Multi-environment usage](#multi-environment-usage)                                 |
| `cert.ps1` / `cert.sh`       | Development TLS certificate                                                                                    |
| `drop-db.ps1` / `drop-db.sh` | Reset an environment's postit database to empty                                                                |

## Prerequisites

- Docker Desktop (Windows, macOS) or Docker Engine with the Compose plugin (Linux).
  Tested with Compose 5.5.1.
- Rust stable, 1.98 or newer — `server/rust-toolchain.toml` tracks the stable channel and
  the workspace sets `rust-version = "1.98"` as the floor.
- OpenSSL, for the development certificate. Already present on macOS and Linux; on Windows
  it ships with Git for Windows (`C:\Program Files\Git\usr\bin\openssl.exe`), which
  `cert.ps1` finds by itself when `openssl` is not on `PATH`.
- 7-Zip, to extract the vault. Every environment needs it, development included: the
  Postgres password and the other secrets are in the vault, never in the repository.
- A local SMTP tool listening on `localhost:25` (for example Papercut). Development mail,
  including the approval emails and Zitadel's own, goes there; nothing is bundled.
- `psql`, only for `drop-db` with `ADMIN_DATABASE_URL` (a database outside this stack).

### Hosts file configuration

Every development URL is `https://postit.local:<port>`, and the certificate is issued for
that name, so it has to resolve.

**Windows** — edit `C:\Windows\System32\drivers\etc\hosts` as Administrator:

```text
127.0.0.1 postit.local
```

**macOS / Linux** — add the same line to `/etc/hosts` with `sudo`.

## Getting started

1. **Extract the vault (first time, and after every change to `!ref/vault.7z`).** Get the
   password out of band, then:

    ```bash
    cd "!ref"
    7z x vault.7z      # prompts for the password, produces !ref/vault/<environment>/*.env
    cd ..
    ```

    Windows without a `7z` CLI: open `!ref/vault.7z` in the 7-Zip GUI and extract into
    `!ref/`. What the files hold: [Environment files and secrets](#environment-files-and-secrets).

2. **Generate and trust the development certificate (first time only).**

    **Windows:**

    ```powershell
    .\cert.ps1
    ```

    **macOS / Linux:**

    ```bash
    ./cert.sh
    ```

    Both create a development CA and a `postit.local` certificate (SAN `postit.local`,
    `*.postit.local`) in `docker/shared/nginx/certs/`, which is git-ignored, and reuse the
    CA on later runs. `cert.ps1` trusts the CA in the current user's Windows Root store;
    `cert.sh` trusts it in the login keychain on macOS and prints the
    `update-ca-certificates` command on Linux. Restart the browser afterwards.

3. **Start the development stack.**

    **Windows:**

    ```powershell
    .\stack.ps1 up
    ```

    **macOS / Linux:**

    ```bash
    ./stack.sh up
    ```

    `development` is the default environment, so it needs no argument. The launcher builds
    the compose `-f` chain and refuses to start if the vault files from step 1 or the
    certificate from step 2 are missing.
    This brings up `postit-postgres`, `postit-zitadel` and
    `postit-nginx-infra`. Zitadel's first boot takes 15–30 seconds; follow it with
    `stack logs development postit-zitadel`.

4. **Bootstrap Zitadel and write `server/config/local.toml`.**

    ```bash
    cd server
    cargo xtask zitadel-bootstrap
    ```

    Creates, idempotently, the `postit` project, the `postit-app` OIDC application and the
    `member@postit.com` user, then writes the generated client ID and audience into
    `server/config/local.toml`. Re-run it after any database wipe — the IDs belong to one
    Zitadel instance, which is why `local.toml` is never shared or put in the vault.

5. **Sign in** at <https://postit.local:44300/ui/console> with one of the
   [development accounts](#development-accounts). Zitadel's own mail (verification,
   password reset) lands in your local SMTP tool (e.g. Papercut) at `localhost:25`.

6. **Run the server.** From `server/`:

    ```bash
    cargo run
    ```

    `cargo run` starts the `postit` binary (`default-run`); `xtask` is only a helper you call
    as `cargo xtask <task>`. `POSTIT_ENV` defaults to `development` in a debug build, and
    `POSTIT_ROLE` defaults to `all`: the API on 44310 and the worker on 44311 in one process.
    Set `POSTIT_ROLE=api` or `worker` to run them separately. Secrets come from the vault
    through `POSTIT_SECRETS_FILE`, and the Zitadel client ID and audience from `local.toml`
    (step 4). Migrations run at startup.

7. **Check that it works.**

    ```powershell
    curl.exe https://postit.local:44310/health
    curl.exe https://postit.local:44310/ready      # 200 once the database and JWKS are loaded
    curl.exe https://postit.local:44311/ready      # the worker
    ```

    Then open <https://postit.local:44310/docs> and use **Authorize**; it signs in through
    Zitadel. Both seeded accounts use the password in the [development accounts](#development-accounts) table.
    Run `GET /api/v1/me` as `admin@postit.com`: you get `role: admin`, `status: active`.
    Sign out of Zitadel (or use a private window), authorize as `member@postit.com` and
    `GET /api/v1/me` returns `status: pending`; every other route returns 403
    `account_pending`. The admin gets a `user_pending_approval` mail in your SMTP tool.
    Approve the member with `PATCH /api/v1/users/{id}` and `{"status":"active"}`; allow up
    to `auth.principal_cache_ttl` (60 s) for the change to show.

8. **Optional: the app containers.** `./stack.ps1 up -App` (`./stack.sh up --app`) now
   builds and runs `postit-server` behind `postit-nginx-app`, serving the API on 44310 and
   the worker on 44311 (44315 stays a placeholder until plan 02 P7). It is behind the `app`
   compose profile so those ports stay free for `cargo run` and the web dev server (`pnpm dev`).
   `./stack.ps1 build development -App` rebuilds the image after code changes. The cert
   files must be the current `postit.local.*` / `postit-dev-ca.crt` names from
   `./cert.ps1`. The first run after this change needs `./stack.ps1 down` so `postit-net`
   is recreated with its fixed subnet (172.30.0.0/24):

    ```powershell
    .\stack.ps1 up -App        # ./stack.sh up --app
    ```

## Development accounts

Seeded into Zitadel. Every value here is fixed, insecure and development-only.

| Account             | Password   | Created by                                                  | Role in postit                                                                                 |
| ------------------- | ---------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `admin@postit.com`  | `P@$$w0rd` | Zitadel's first-instance setup, `docker/zitadel/steps.yaml` | Bootstrap admin (`auth.bootstrap.admin_email` in `server/config/development.toml`)             |
| `member@postit.com` | `P@$$w0rd` | `cargo xtask zitadel-bootstrap`                             | Ordinary user — signs in as `pending` until an admin approves, which is what it exists to test |

`steps.yaml` also creates the `postit-bootstrap` machine user. Zitadel prints its key
**once**, on the boot that creates the instance; the bootstrap task captures it from
`docker logs postit-zitadel` and caches it at
`docker/zitadel/machinekey/postit-bootstrap.json` (git-ignored). That key belongs to the
Zitadel instance inside the database volume, so `stack down -v` and `stack reset` delete it
along with the volume.

Every other development secret — the Postgres password, the Zitadel master key, the audit
pseudonym key — is random and lives in `!ref/vault/development/`, exactly like qa's and
production's. The two account passwords above are the exception: test fixtures, published
here on purpose.

## Multi-environment usage

| Environment           | Command                          | What runs                                                                           |
| --------------------- | -------------------------------- | ----------------------------------------------------------------------------------- |
| Development (default) | `stack.ps1 up` / `./stack.sh up` | Postgres, Zitadel, nginx TLS front doors; the server runs natively with `cargo run` |
| QA                    | `stack.ps1 up qa`                | Postgres, `postit-api`, `postit-worker` — from P6                                   |
| Production            | `stack.ps1 up production`        | Same as QA, with a promoted image — from P6                                         |

Each resolves to the same thing, with nothing left to remember:

```text
docker compose -f docker/docker-compose.yml -f docker/docker-compose.<environment>.yml up -d
```

The full command surface — `up`, `down`, `restart`, `ps`, `logs`, `config`, `build`, `psql`,
`reset` — is in `stack.ps1 help` / `./stack.sh help`. Every command takes an optional
environment in second position (`stack.ps1 logs qa postit-api`), and anything after it —
service names, or compose flags such as `--tail=100` — is passed through to
`docker compose`.

> **One host runs one environment.** All three environments are the same Compose project
> (`name: postit`) with the same container names and the same named volume
> `postit_postit-pgdata`. They cannot run side by side, and switching between them on one
> machine reuses the previous environment's **database**. Run `stack reset <environment>`
> when switching.

### Hostnames

| Service                             | Development                                     | QA                           | Production                |
| ----------------------------------- | ----------------------------------------------- | ---------------------------- | ------------------------- |
| API (`/api/v1`, `/docs`, `/health`) | `https://postit.local:44310`                    | `https://api.qa.postit.com`  | `https://api.postit.com`  |
| Web app                             | `https://postit.local:44315`                    | `https://app.qa.postit.com`  | `https://app.postit.com`  |
| OIDC issuer                         | `https://postit.local:44300` (bundled Zitadel)  | `https://auth.qa.postit.com` | `https://auth.postit.com` |
| SMTP                                | local SMTP tool (e.g. Papercut), `localhost:25` | `smtp.qa.postit.com:587`     | `smtp.postit.com:587`     |

Where each value is owned:

- **QA** — `server/config/qa.toml`, which is baked into the image. The qa compose file does
  not repeat it.
- **Production** — `docker/docker-compose.production.yml`, because
  `server/config/production.toml` deliberately bundles no issuer or hostname. Each value is a
  `${VAR:-default}`, so a different deployment overrides `POSTIT_API_URL`,
  `POSTIT_APP_URL`, `POSTIT_OIDC_ISSUER`, `POSTIT_OIDC_AUDIENCE`,
  `POSTIT_BOOTSTRAP_ADMIN_EMAIL` or `POSTIT_SMTP_HOST` in `docker/.env` (copy
  `docker/.env.example`) without editing the file.
- **Development** — `server/config/development.toml` and the compose file; the host is
  always `postit.local`.

`auth.*` is the operator's OIDC provider and is **not** in any compose file: plan 02 has
qa and production run or subscribe to one rather than bundle it. The provider must issue
JWT access tokens with postit's audience and allow a PKCE public client with postit's
redirect URIs.

### Environment files and secrets

Per-environment secrets are packed into one encrypted archive, `!ref/vault.7z`, which **is**
committed. Extracted ([Getting started](#getting-started), step 1), it is one folder per
environment and **one file per project**, holding that project's secrets and nothing else:

```text
!ref/vault/
    development/
        postgres.env      POSTGRES_USER, POSTGRES_PASSWORD
        postit.env        POSTIT__DATABASE__USERNAME / __PASSWORD, POSTIT__AUDIT__PSEUDONYM_KEY
        zitadel.env       ZITADEL_MASTERKEY, ZITADEL_DATABASE_POSTGRES_{ADMIN,USER}_{USERNAME,PASSWORD}
    qa/
        postgres.env
        postit.env        ...plus POSTIT__MAIL__SMTP__USERNAME / __PASSWORD, once the relay exists
    production/
        postgres.env
        postit.env
```

`!ref/vault/` is git-ignored except for the `.gitkeep` in each folder — never commit the
extracted files in the clear.

**Secrets only.** Usernames, passwords, keys and client secrets go in the vault; URLs,
hostnames, ports and every other setting are config and stay with their project —
`server/config/<environment>.toml` for postit, the compose files for the containers. That is
why `database.url` is `postgres://postit-postgres:5432/postit` in `qa.toml` and carries no
credentials: the config loader refuses a `database.url` that does.

**How each file is read.** Every file is `KEY=value` lines named after the variables its
project already understands, so nothing translates them:

| File           | Read by                                                                                                                                                                                                    |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `postgres.env` | `postit-postgres`, through `env_file:` in every environment file                                                                                                                                           |
| `postit.env`   | `postit-api` and `postit-worker` through `env_file:` (qa, production); a native `cargo run` through `POSTIT_SECRETS_FILE`, which `server/.cargo/config.toml` points at `!ref/vault/development/postit.env` |
| `zitadel.env`  | `postit-zitadel`, through `env_file:` (development only — qa and production bundle no IdP)                                                                                                                 |

A missing `env_file` is a hard error to Compose, but only for the first one it meets;
`stack up` checks every file of the environment first and names the missing ones.

**Quote any value that contains `$`.** Compose interpolates `$` in an `env_file:` (`$$`
becomes `$`), so an unquoted `POSTGRES_PASSWORD=ab$$cd` reaches the container as `ab$cd` and
Postgres initialises with a password the native server, which reads the file literally,
never sends. Wrap the value in single quotes — `POSTGRES_PASSWORD='ab$$cd'` — which Compose
takes literally and `postit-config` strips the same way. Double quotes still interpolate.

**One database role, the default `postgres` superuser, in every environment.** postit and
Zitadel connect as the same role `postgres.env` creates, so its password appears in
`postgres.env`, `postit.env` and (development) `zitadel.env` — each project file stays
self-contained. They have to agree, and nothing checks it. Postgres applies
`POSTGRES_PASSWORD` only when it initialises an empty volume, so **rotating it** means
`ALTER ROLE postgres PASSWORD '…'` through `stack psql <environment>`, then the same value in
every file that holds it, then a re-pack.

There is no `local.toml` in the vault — its client ID and audience belong to one Zitadel
instance, so every clone runs `cargo xtask zitadel-bootstrap`.

**Re-pack after editing any vault file**, from the repository root; nothing enforces it, and
an un-packed change reaches nobody else:

```bash
7z a -p -mhe=on "!ref/vault.7z" "!ref/vault/*"
```

`-mhe=on` encrypts the file names as well. Every committed version of the archive stays
decryptable with the password it was made with, forever — **rotating a leaked secret means
rotating the secret itself**, not re-packing or changing the archive password.

### Compose file layering

All compose files live in `docker/` and **nothing is auto-merged**. Compose auto-merges
`docker-compose.override.yml` only when it discovers `docker-compose.yml` in the working
directory; every invocation here names its files with `-f`, so development is passed exactly
like qa and production and a forgotten flag cannot leak development settings into another
environment.

```text
docker/
    docker-compose.yml               base: postit-postgres, postit-net, postit-pgdata
    docker-compose.development.yml   + ports, Zitadel, nginx front doors
    docker-compose.qa.yml            + postit-api, postit-worker
    docker-compose.production.yml    + the same, with production hostnames
    server.Dockerfile                the postit-server image
    .env.example                     optional local overrides (copy to docker/.env)
    postgres/init/                   dev only: creates the zitadel database
    shared/nginx/                    dev only: TLS front-door config, certs/ (git-ignored)
    zitadel/                         dev only: first-instance seed, machinekey/ (git-ignored)
```

Conventions, all deliberate:

- **One file per environment, named after it** — `docker-compose.<environment>.yml`, the
  same word `POSTIT_ENV` uses — so the launcher and the scripts derive it and never map
  `dev` to `development`.
- **The project name is pinned** (`name: postit`). Compose otherwise names the project after
  the directory of the first `-f` file, `docker`, and every container, network and volume
  would change name with it.
- **Every service is `postit-`-prefixed and `container_name` equals the service name**, so
  `docker logs postit-zitadel`, `docker exec postit-postgres …` and the nginx upstreams work
  without knowing the compose file chain.
- **One explicit network, `postit-net`, and one named volume, `postit-pgdata`.**
- **Relative paths resolve against `docker/`**, the directory of the first `-f` file,
  wherever the command runs from. The image build is the exception on purpose: its context
  is `..`, the repository root, because the image needs `server/` and, from P7, `web/`.
- **The base file is never run alone.** It holds no credentials: each environment file
  adds `env_file: ../!ref/vault/<environment>/<project>.env` for its own vault folder, and
  no compose file contains a secret value.

### The server image

`docker/server.Dockerfile` builds `postit-server` from the repository root:

```bash
docker build -f docker/server.Dockerfile -t postit-server:local .
```

`stack build qa` does the same through compose. It compiles the `dist` profile on
`rust:1-slim-trixie` (latest stable), runs on `debian:trixie-slim` as the unprivileged `postit` user,
and ships `server/config/{default,qa,production}.toml` at `/app/config/` — `local.toml`
never enters the build context (`.dockerignore`, which also excludes `!ref/`), and secrets
arrive only at run time, from the vault's `postit.env`. Plan 02 promotes **the same image** from qa to production; production pins it
with `POSTIT_IMAGE` in `docker/.env` instead of building.

Still to come from plan 02: cargo-chef dependency layers, `SQLX_OFFLINE=true`, the Node
web stage (P7), and a `HEALTHCHECK` on `postit healthcheck` (P6).

### Reverse proxy (qa, production)

qa and production publish **nothing to the network**: `postit-api` binds the API on
`127.0.0.1:8080` and the web app on `127.0.0.1:8082`, the worker publishes no port at all,
and Postgres is reachable only over `postit-net`. TLS on 443 belongs to the operator's
reverse proxy on the same host. A Caddy example, not bundled:

```caddyfile
api.qa.postit.com {
    reverse_proxy 127.0.0.1:8080
}

app.qa.postit.com {
    reverse_proxy 127.0.0.1:8082
}
```

Production is the same with `api.postit.com` and `app.postit.com`. The 443xx port convention
is development-only.

## Web app

The React client lives in `web/` (Vite, TypeScript, Tailwind, shadcn/ui; see `web/README.md`).

- Prerequisites: Node 24 (`web/.nvmrc`). Run `pnpm install` in `web/` before `cargo xtask openapi`, which now also needs Node (and so does `cargo xtask openapi --check`).
- `pnpm dev` in `web/` serves `https://postit.local:44315` with the dev certificate (`cert.ps1` / `cert.sh` first).
- `cargo xtask openapi` regenerates `api/openapi.json` and `web/src/api/schema.d.ts`; never edit the latter by hand.
- Existing dev stacks must re-run `cargo xtask zitadel-bootstrap` once so Zitadel allows the web origin to frame its pages for the silent sign-in iframe.
- The Docker image includes the web build; `./stack.ps1 up development -App` serves it on 44315 behind `postit-nginx-app`.
- Session restore after a reload needs the IdP and the web app to be same-site (true for `postit.local:44300` / `:44315`, and for `auth.*` / `app.*` under one domain). With another OIDC provider, allow the web origin to frame its login pages; expect a one-click sign-in after reload if it is cross-site and the browser blocks third-party cookies.
- Playwright smoke (opt-in): `POSTIT_E2E=1 POSTIT_ADMIN_PASSWORD=... POSTIT_MEMBER_PASSWORD=... pnpm e2e` in `web/`. The member test needs `member@postit.com` to be pending, so run it on a freshly reset stack.

## PostgreSQL named volume

- Data lives in the Docker-managed named volume `postit_postit-pgdata`, on the
  Linux-native filesystem inside the Docker Desktop VM. A Windows host bind-mount would
  route every I/O through the gRPC-FUSE/9p layer, a documented cause of slow
  Postgres-in-Docker on Windows.
- The image is `postgres:18`, matching plan 01 and CI. The volume mounts at
  `/var/lib/postgresql` — not `.../data` — because postgres 18 keeps its data in
  `/var/lib/postgresql/18/docker`.
- `stack down` preserves the data. **`stack down -v` destroys it**, and on production
  refuses without `--force` / `-Force`. Nothing here backs it up yet (plan 02 P9).
- In development the same server also holds Zitadel's own `zitadel` database, created on
  first init by `docker/postgres/init/01-zitadel-db.sql`. qa and production do not mount
  that script.

## Database

`drop-db.ps1` / `drop-db.sh` reset an environment's postit database to empty (Postgres only
— the server recreates the schema on its next boot):

```bash
./drop-db.sh development --force     # dev: stop app services, drop, recreate, restart
./drop-db.sh qa --no-stop            # qa/production: docker stop postit-api postit-worker first
```

`production` always prompts by retyping the database name and refuses `--force`. The
`zitadel` database is never touched. The script runs `psql` inside `postit-postgres` on this
host, as the container's own `POSTGRES_USER` over the local socket, which the postgres image
trusts — so it handles no password, needs no host `psql`, and works in qa and production
where no database port is published. To reset a database somewhere else, set
`ADMIN_DATABASE_URL=postgres://user:password@host:port/postit`; that path uses a host `psql`.

`migrate.ps1` / `migrate.sh` (a thin `sqlx-cli` wrapper for authoring migrations and
refreshing the offline query cache) land once `postit-data`'s embedded migrations exist
(plan 02 P4). Install `sqlx-cli` at the version matching the workspace's `sqlx` dependency:

```bash
cargo install sqlx-cli --version <x> --no-default-features --features rustls,postgres --locked
```

For an interactive shell on the running stack's database, `stack psql [environment]`.

## Ports

Development ports sit in 44300–44399, except Postgres, which uses its default 5432.

| Port  | Service                                                             | Development                                      | qa / production                |
| ----- | ------------------------------------------------------------------- | ------------------------------------------------ | ------------------------------ |
| 44300 | Zitadel (issuer, login UI, console)                                 | `postit-nginx-infra`                             | —                              |
| 44310 | API: `/api/v1/*`, `/docs`, `/api/openapi.json`, `/health`, `/ready` | `cargo run`, or `postit-nginx-app` placeholder   | —                              |
| 44311 | Worker: `/health`, `/ready`                                         | same                                             | —                              |
| 44315 | React web app                                                       | `pnpm dev`, or `postit-nginx-app` placeholder | —                              |
| 5432  | Postgres, for SQLx tooling and `cargo sqlx prepare`                 | `postit-postgres`                                | not published                  |
| 8080  | API, plain HTTP                                                     | —                                                | `postit-api`, `127.0.0.1` only |
| 8082  | Web app, plain HTTP                                                 | —                                                | `postit-api`, `127.0.0.1` only |

**Nothing plaintext is published in development**: nginx terminates TLS for
Zitadel, and a natively run server terminates its own TLS with the same certificate. The
only exception is Postgres, which tooling reaches directly. SMTP is not bundled — point
`server/config/development.toml` at your own local SMTP tool (e.g. Papercut,
`localhost:25`).

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `main` and `stage` and on pull requests:

- **`server (linux)`** — `cargo fmt --check`, `cargo clippy … -D warnings`,
  `cargo check`, `cargo test --workspace`.
- **`server (windows)`** — `cargo check` and `cargo clippy` with `SQLX_OFFLINE=true`.

Both run from `server/`. Plan 02 adds the `contract`, `app` and `docker` jobs (image build
plus container smoke test) in P9. CI never touches the vault and never publishes live.

## Common commands

| Action                                     | Command                                                                                                               |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- |
| Generate and trust the dev certificate     | `cert.ps1` / `./cert.sh`                                                                                              |
| Start (development infrastructure)         | `stack.ps1 up` / `./stack.sh up`                                                                                      |
| Start with the app placeholders            | `stack.ps1 up -App` / `./stack.sh up --app`                                                                           |
| Stop                                       | `stack.ps1 down`                                                                                                      |
| Stop and wipe the database                 | `stack.ps1 down -Volumes` / `./stack.sh down -v`                                                                      |
| Wipe and restart                           | `stack.ps1 reset`                                                                                                     |
| Container status                           | `stack.ps1 ps`                                                                                                        |
| Follow all logs                            | `stack.ps1 logs`                                                                                                      |
| Zitadel logs only                          | `stack.ps1 logs development postit-zitadel`                                                                           |
| Resolved compose config for an environment | `stack.ps1 config qa`                                                                                                 |
| Build the server image                     | `stack.ps1 build qa`                                                                                                  |
| Postgres shell                             | `stack.ps1 psql`                                                                                                      |
| Bootstrap Zitadel, write `local.toml`      | `cargo xtask zitadel-bootstrap` (from `server/`)                                                                      |
| Reset the postit database only             | `drop-db.ps1 development -Force`                                                                                      |
| Everything else                            | `stack.ps1 help`                                                                                                      |
| Run the server (API and worker)            | `cargo run` (from `server/`)                                                                                          |
| Build                                      | `cargo build` (from `server/`)                                                                                        |
| Quality gates                              | `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` |

## Troubleshooting

**`stack: no dev certificate at docker/shared/nginx/certs/postit.local.crt`.** Run
`cert.ps1` / `./cert.sh` first. The launcher checks before compose does, because without the
certificate `postit-nginx-infra` starts and then fails to load its TLS configuration.

**The browser warns about the certificate.** Expected until the CA is trusted — re-run the
certificate script, then restart the browser completely. Also check the URL says
`postit.local`: the certificate names `postit.local` and `*.postit.local`, not `localhost`.

**`stack: development needs these vault files, and they are missing`** (or `qa`,
`production`). The vault is not extracted, or predates the one-file-per-project layout. See
[Environment files and secrets](#environment-files-and-secrets). Run through compose
directly, the same condition is `env file …/!ref/vault/… not found`, for the first missing
file only.

**`postit-zitadel` restarts with `password authentication failed for user "postgres"`**, or
postit cannot connect after a vault change. The volume was initialised with a different
`POSTGRES_PASSWORD` — Postgres reads it only once, on an empty volume. In development,
`stack reset`; elsewhere, rotate as described under
[Environment files and secrets](#environment-files-and-secrets).

**`cargo run` fails with `password authentication failed for user "postgres"`.** The
`.pgpass ... not found` debug line above it is harmless. The server did read the vault and
connect; the database rejected the password. Postgres applies `POSTGRES_PASSWORD` only when
it initialises an empty volume, so the volume was created before the vault password
changed. In development run `stack reset` (it wipes the database, Zitadel's data included),
then re-run `cargo xtask zitadel-bootstrap`. To confirm first, connect inside the
container over its network address (not `127.0.0.1`, which the image trusts) with the vault
password; it fails the same way.

**`cargo run` says it could not determine which binary to run.** You are on a checkout from
before `default-run` was set; use `cargo run --bin postit`.

**Signing in through Swagger UI ends on `{"code":5,"message":"Not Found"}` at
`/ui/v2/login/login`.** Zitadel v4 sends sign-in to a separate `zitadel-login` app that this
stack does not run. `cargo xtask zitadel-bootstrap` sets the instance feature
`loginV2.required = false` so Zitadel serves its built-in login at `/ui/login`; re-run it,
then open `/docs` again (an old auth request still points at the v2 page).

**Swagger UI `Authorize` fails with `The requested redirect_uri is missing in the client
configuration`.** The `postit-app` application lacks `https://postit.local:44310/docs/oauth2-redirect.html`.
Re-run `cargo xtask zitadel-bootstrap`: it updates an existing app's redirect URIs.

**`cargo xtask zitadel-bootstrap`: "Zitadel never became reachable".** Zitadel is still on
its first boot, or the stack is not up. `stack ps` should show `postit-zitadel` and
`postit-nginx-infra` running; `stack logs development postit-zitadel` shows it reach
`server is listening`.

**`cargo xtask zitadel-bootstrap`: "no machine key found in `docker logs postit-zitadel`".**
Zitadel prints the machine key only on the boot that creates the instance, and the cached
copy is gone — typically the container was recreated over an existing volume after the
cache file was deleted. Start over with `stack reset`, then run the bootstrap again.

**The bootstrap authenticates and is refused, after a database wipe done by hand.** The
cached `docker/zitadel/machinekey/postit-bootstrap.json` belongs to the previous Zitadel
instance. `stack down -v` and `stack reset` delete it; a raw `docker compose down -v` does
not. Delete the file and re-run the bootstrap.

**`./stack.sh` from PowerShell fails with `failed to connect to the docker API at
unix:///var/run/docker.sock`.** `bash` on a Windows `PATH` is usually WSL's, which has no
Docker socket of its own unless Docker Desktop's WSL integration is on. Use `stack.ps1`, or
run `stack.sh` from Git Bash — where it passes Windows paths (`D:/…`) to `docker.exe` itself.

**Upgrading a checkout from before the `postit-` rename.** The old stack's containers
(`postit-postgres-1`, `postit-zitadel-1`, …) belong to the same `postit` project, so compose
reports them as orphans: `stack down --remove-orphans` removes them. The old volume,
`postit_postgres-data`, is left alone and is also a Postgres 16 data directory, which
postgres 18 cannot open — the new stack starts on a fresh `postit_postit-pgdata` instead.
Delete `docker/zitadel/machinekey/`, bring the stack up, re-run
`cargo xtask zitadel-bootstrap`, and remove the old volume with
`docker volume rm postit_postgres-data` once nothing in it is needed.

**Port conflicts.** Everything in development is in 44300–44399 (Postgres on its default 5432). If one
is taken, `docker compose` reports `port is already allocated` on `up`; `docker ps` shows the
holder when it is a container.

**qa or production `stack up` fails or restarts in a loop.** `postit-api` and
`postit-worker` refuse to start without an OIDC issuer and audience, a database password, SMTP
settings and `audit.pseudonym_key`. `stack config qa` shows the resolved configuration, and
`stack logs qa postit-api` the refusal.
