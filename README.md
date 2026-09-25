# postit

A self-hosted social publishing service for a small team. See `!ref/plans/` for the
implementation plans; `01. vision and architecture.md` first.

## Extract configuration files (vault)

Per-environment secrets are packed into a single encrypted `!ref/vault.7z`, committed to
the repo. Get the password out of band (password manager), then extract it in place:

```sh
cd "!ref"
7z x vault.7z      # prompts for the password, produces !ref/vault/<environment>/...
cd ..
```

Windows without a `7z` CLI: open `!ref/vault.7z` in the 7-Zip GUI and extract into `!ref/`.

Development needs only `!ref/vault/development/postit-database-url.txt` (the fixed dev
URL also baked into `server/config/development.toml`); qa and production hold the secrets
behind the `POSTIT__…_FILE` variables in `deploy/env/qa.env.example` and
`deploy/env/production.env.example`. There is no `local.toml` in the vault — every clone
runs `cargo xtask zitadel-bootstrap` (step 3 below) to get its own.

## Quick start (Windows)

Requires Docker Desktop, Rust 1.98.1, and Git for Windows (bundles `openssl`).

1. Generate and trust the dev certificate:

   ```powershell
   ./cert.ps1
   ```

   Then add this to `C:\Windows\System32\drivers\etc\hosts` as Administrator:

   ```
   127.0.0.1 postit.local
   ```

2. Bring up Postgres, Mailpit, Zitadel, and the TLS front door:

   ```powershell
   cd docker
   docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d
   cd ..
   ```

   Zitadel's first-run setup takes 15-30 seconds. Watch it with
   `docker compose logs -f zitadel`.

3. Bootstrap the Zitadel project, OIDC app, and `member@postit.local` user, and write
   `server/config/local.toml`:

   ```powershell
   cd server
   cargo xtask zitadel-bootstrap
   ```

   This is idempotent — re-run it any time after `docker compose down` and back `up`.

4. Sign in at `https://postit.local:44330/ui/console`:

   - `admin@postit.local` / `PostitDev1!`
   - `member@postit.local` / `PostitDev1!`

   Zitadel's own emails (verification, password reset) land in Mailpit at
   `https://postit.local:44320`.

5. `nginx-app` (the API/worker/web placeholders on 44300/44305/44310) is behind the
   `app` compose profile:

   ```powershell
   cd docker
   docker compose -f docker-compose.yaml -f docker-compose.dev.yaml --profile app up -d
   cd ..
   ```

   These serve a placeholder page until plan 02 phases P6 (API/worker) and P7 (web)
   replace them.

All dev-only secrets in `docker/docker-compose.dev.yaml`, `docker/zitadel/steps.yaml`, and
`server/config/development.toml` are fixed, insecure, and clearly marked as such — never
reused outside development.

## Quick start (Linux / macOS)

Same steps, with `cert.sh` instead of `cert.ps1` and `/etc/hosts` instead of the Windows
hosts file (`cert.sh` prints the exact trust command for your OS). `cargo xtask
zitadel-bootstrap` is identical.

## Database

`drop-db.ps1` / `drop-db.sh` reset an environment's postit database to empty (Postgres
only — the server recreates the schema on its next boot):

```sh
./drop-db.sh development --force     # dev: stop app services, drop, recreate, restart
./drop-db.sh qa --no-stop            # qa/production: you stop api/worker yourself first
```

`production` always prompts by retyping the database name and refuses `--force`. The
connection comes from `!ref/vault/<environment>/postit-database-url.txt` (extracted
above); override it with `ADMIN_DATABASE_URL` when the vault role lacks `CREATEDB` /
superuser rights (qa/production run postit as the database owner, not superuser — grant
`CREATEDB` and `pg_signal_backend`, or point the script at an admin connection string).

`migrate.ps1` / `migrate.sh` (a thin `sqlx-cli` wrapper for authoring migrations and
refreshing the offline query cache) lands once `postit-data`'s embedded migrations exist
(plan 02 phase P4) — not yet in this repo.

Install `sqlx-cli` at the version matching the workspace's `sqlx` dependency:

```sh
cargo install sqlx-cli --version <x> --no-default-features --features rustls,postgres --locked
```

## Everyday commands

```sh
cargo build                     # dev build
cargo test --workspace          # all tests
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml down    # from docker/, stop the dev stack
```
