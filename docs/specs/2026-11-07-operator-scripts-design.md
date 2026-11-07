# Operator scripts: one job per script

**Date:** 2026-11-07. **Status:** design, for approval. **Replaces:** `stack.ps1`/`stack.sh` and `drop-db.ps1`/`drop-db.sh`. **Reference:** the VigilixMambo repository (`vault`, `db`, `compose`, `doctor`, `cert`, with `docs/scripts.md` as their index).

## Intent

The maintainer works across several of their own repositories and wants the same commands in each. postit's operator tooling grew out of an earlier, simpler project into one `stack` launcher that starts containers, checks the vault, opens `psql`, and backs up and restores the database, plus a separate `drop-db`. VigilixMambo splits the same work into small scripts with one concern each, a shared help layout, and one index page. postit adopts that convention, so `./compose.sh up`, `./db.sh backup`, `./vault.sh pack`, and `./doctor.sh` mean the same thing here as there.

The project runs locally for now (about the next month); deployment is the roadmap's last phase (plan 03 G1), and the host is undecided (Hostinger or AWS Lightsail are the likely ones). So the scripts serve development first, and accept `qa` and `production` only where that costs nothing extra.

## Goals

- Five root scripts, each a `.sh` and `.ps1` twin with the same commands, the same help text apart from flag spellings, and the same exit codes: `compose`, `db`, `vault`, `doctor`, `cert`.
- Mambo's verbs wherever postit has the same operation, and Mambo's help layout (`USAGE`, `COMMANDS`, `OPTIONS`, `EXAMPLES`, `EXIT CODES`, `MORE`), printed by `help`, `-h`, `--help`, and by a bare call with no command.
- `docs/scripts.md`: the index of every script, the first-run order, and everyday recipes, as in Mambo.
- Every behavior `stack` and `drop-db` have today survives in its new home, guards included (see "What moves where").
- Every live reference to `stack` or `drop-db` is updated: `README.md`, `CLAUDE.md`, `docs/deployment.md`, the contracts, the compose file headers, the `xtask` hint, the CI job, the agent skill.

## Non-goals

- **No `infra/` folder, no Terraform, no deploy or push scripts.** Their content depends on the host G1 picks (one VPS with Caddy, or AWS services). Written now it would be guesswork, untested until then, and likely rewritten. G1 owns it.
- **No provider choice.** Recorded in the existing memory and roadmap: undecided, chosen at G1.
- **No new qa or production machinery.** The `qa`/`production` compose overlays, vault folders, and `deploy/env/*.env.example` stay as they are.
- **No change to how secrets reach containers.** postit's compose files read the vault through `env_file:`; Mambo writes a `docker/.env` from the vault with `compose env`. postit keeps `env_file:` (it never copies a secret into a second file), so there is no `compose env` command.
- **No `js` script.** Mambo has one because it manages several JavaScript projects; postit has one (`web/`), whose commands are in `web/CLAUDE.md`. It can be added later if the maintainer wants the name for muscle memory.
- No application behavior changes.

## Anti-goals

- A script that grows back into a launcher doing everything.
- A script that prints a secret: every script keeps today's rule that no vault value reaches the terminal (`config` keeps `--no-env-resolution`; `doctor` reports by key name only).
- A destructive command that runs without a typed confirmation, or that can hit production by default.
- Two scripts owning the same operation (for example, both `compose` and `db` dropping the database).

## Constraints

- Bash halves need Bash 4+ (Linux, or Git Bash on Windows); macOS is not a target (same as Mambo). The `pwd -W` / `cygpath -m` handling that lets Git Bash pass `D:/…` paths to `docker.exe` stays.
- PowerShell halves run on PowerShell 7 (`pwsh`), with Mambo's flag spellings (`-Force`, `-Volumes`, `-App`).
- Environments are `development` (default where a command takes one optionally), `qa`, `production`.
- Container and service names, the compose `-f` chain (base file plus exactly one environment file), the project name `postit`, and the vault layout (`!ref/vault/<env>/{postgres,postit,zitadel}.env`) do not change.
- The repository rules in `CLAUDE.md` hold: Claude edits the extracted vault files and never re-packs the archive itself; `vault pack` is the maintainer's command.

## The scripts

### `compose`: the Docker Compose stack

```
./compose.sh <command> [environment] [options] [docker compose arguments]
```

| Command | Does |
| --- | --- |
| `up` | Pre-flight (the environment's vault files exist; in development the dev certificate exists, and with `--app` the dev CA), then `up -d`. |
| `down` | Stop and remove the containers. `--volumes` also deletes the database volume: a typed confirmation (the environment name), skipped by `--force` except in production, where `--force` is required **and** the confirmation is still asked. In development it also removes the cached Zitadel machine key, as `stack down -v` does today. |
| `restart`, `ps`, `logs` | As today; across every profile, so `--app` containers are always seen. `logs` follows. |
| `config` | The resolved configuration with `--no-env-resolution`, so vault files stay paths. |
| `build` | Build the `postit-server` image (development: with `--app`). |
| `help` | The help. |

Options: `--app` (development only), `--volumes` / `-v`, `--force`; anything else is passed to `docker compose` unchanged, as today.

`stack reset` goes away: it is `down --volumes` then `up`, two commands, and the confirmation belongs to the destructive half.

### `db`: the postit database

```
./db.sh <command> <environment> [options]
./db.sh restore <environment> <file> [options]
./db.sh prepare
```

| Command | Does |
| --- | --- |
| `psql` | Interactive `psql` inside `postit-postgres`, as the container's own `POSTGRES_USER` over the local socket (no password handled). |
| `backup` | Today's `stack backup`: refuses unless the running `postit-postgres` belongs to the named environment, writes `!ref/backups/postit-<env>-<UTC>.dump` (or `POSTIT_BACKUP_DIR`), refuses a destination inside the repository that git does not ignore, owner-only file and directory, removes the in-container copy. |
| `restore` | Today's `stack restore`: the dump's name must name the environment, the running container must belong to it, the server must be stopped, one transaction, production needs `--force`. |
| `drop` | Today's `drop-db`: stop the app services (development) or require `--no-stop` (qa, production), retype the database name (`--force` skips it in development and qa; production always asks and refuses `--force`), `DROP … WITH (FORCE)` and recreate, restart the dev app services and wait for them. `ADMIN_DATABASE_URL` still reaches a database elsewhere. The `zitadel` database is never touched. |
| `prepare` | Refresh the sqlx offline caches (every `backend/crates/*/.sqlx/`; today only `postit-data` has one) against a throwaway database built from the current migration: create `postit_prepare` on the development server, apply `backend/crates/data/migrations`, run `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from each crate that has a `.sqlx/`, with `DATABASE_URL` pointing at it, then drop it, also on failure. This is the step the D3 work had to do by hand because the development database lagged the migration. Takes no environment. |
| `help` | The help. |

Mambo's `migrate add|remove|list|update|clear` has no postit equivalent yet: the single-migration policy (one `0001_init.sql`, edited in place until G1) means there is nothing to add or remove, and the server applies migrations at boot. Plan 04's unbuilt `migrate` wrapper (V4) becomes `db migrate` at G1, when the policy ends; the contract notes that.

### `vault`: the encrypted archive

```
./vault.sh <command> [--force]
```

| Command | Does |
| --- | --- |
| `unpack` | Extract `!ref/vault.7z` into `!ref/`. Refuses when `!ref/vault/` already holds files, unless `--force`. 7-Zip asks for the password; it never becomes an argument. |
| `pack` | Re-pack `!ref/vault/` into `!ref/vault.7z` (AES-256, encrypted file names, `-mhe=on`), leaving out `.gitkeep`, all three environments included (see "Decided" below). It writes a temporary archive and replaces `vault.7z` only when 7-Zip succeeds. |
| `check` | Compare the archive with `!ref/vault/` by SHA-256 per file; exit 2 when they differ. Lists the archive with the password prompt, prints file names only. |
| `help` | The help. |

### `doctor`: is this machine ready

```
./doctor.sh [check] [environment]
```

Read-only. One row per check, `PASS`, `WARN`, `FAIL`, or `SKIP`, and every `FAIL` or `WARN` row ends with `->` and the exact fix, as in Mambo. It writes nothing, prompts for nothing, starts nothing, and prints no secret.

- **development (the default):** tools (`docker`, `7z`, `openssl`, `cargo`, `sqlx`, `pnpm`; `flutter` and a host `psql` as `WARN` only), vault files and values (today's `stack doctor` rules, unchanged), the dev certificate and whether the CA is trusted, the `postit.local` hosts entry, Postgres reachable with the vault's `DATABASE_URL`, the database's applied migration matches `0001_init.sql` (the checksum in `_sqlx_migrations`, so a lagging development database is reported with "reset it" as the fix), Zitadel's discovery document reachable on `https://postit.local:44300`, `cargo xtask zitadel-bootstrap` run (`backend/config/local.toml` present), web packages installed.
- **qa, production:** vault files and values only, as `stack doctor` checks today (no Docker, no network), plus the production `docker/.env` variables.

### `cert`: the development certificate

```
./cert.sh [setup|gen|trust|help]
```

Today's certificate generation unchanged (dev CA plus the `postit.local` leaf in `docker/shared/nginx/certs/`), split into Mambo's verbs: `gen` writes them, `trust` installs the CA into the OS trust store (or prints the command that needs `sudo` / an elevated shell), `setup` (the default) does both. The help moves to the shared layout.

## What moves where

| Today | After |
| --- | --- |
| `stack up/down/restart/ps/logs/config/build [env] [--app]` | `compose …`, same arguments |
| `stack down -v` | `compose down --volumes` (now with a typed confirmation) |
| `stack reset` | `compose down --volumes`, then `compose up` |
| `stack psql` | `db psql development` |
| `stack backup [env]` | `db backup <env>` |
| `stack restore [env] <file>` | `db restore <env> <file>` |
| `stack doctor [env]` | `doctor check <env>` (development also gets the machine checks) |
| `drop-db <env> [--force] [--no-stop]` | `db drop <env> [--force] [--no-stop]` |
| `7z a …` / `7z x …` by hand | `vault pack` / `vault unpack` |
| (by hand, D3) scratch database for `cargo sqlx prepare` | `db prepare` |

`docker/tests/stack-doctor.sh` becomes `docker/tests/doctor.sh` (the same fixture-vault tests, against `doctor.sh`), and the CI `docker` job runs the new name. The agent skill `.claude/skills/stack/` becomes `.claude/skills/scripts/`, describing all five scripts and pointing at `docs/scripts.md`.

## Decided: `vault pack` keeps packing `production/`

Mambo's `pack` leaves out every `production/` folder, because its production secrets live in AWS Secrets Manager and an archive in git history keeps every value it ever held. postit has no secret store yet, so (maintainer's decision, 2026-11-07) `pack` keeps all three environments, as plan 04 does today, with the encryption mandatory for that reason. G1 revisits this when it picks a host and a secret store; the help's `MORE` section says so.

## Left to the builder

- The order of the work, and whether each script lands in its own commit.
- Exact help wording, within the shared layout.
- How `doctor` reads the migration checksum (`psql` in the container, or a host `psql` when present) and how it tests the CA trust on each OS.
- The PowerShell idioms for each Bash construct, keeping behavior identical.
