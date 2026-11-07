# Operator scripts

postit's operator tooling is five small scripts at the repository root, each with one job.
Every script is a twin: `<name>.sh` for Bash (Linux, or Git Bash on Windows) and
`<name>.ps1` for PowerShell 7, with the same commands, the same exit codes, and the same
help apart from flag spellings (`--force` / `-Force`, `--volumes` / `-Volumes`, `--app` /
`-App`, `--no-stop` / `-NoStop`). Each prints its help with `help`, `-h`, `--help`, or no
command, in the same layout: `USAGE`, `COMMANDS`, `OPTIONS`, `EXAMPLES`, `EXIT CODES`,
`MORE`. They run from any directory.

The layout follows the maintainer's VigilixMambo repository, so the same command means the
same thing in both. The examples below use the Bash names; on Windows type `.\compose.ps1`
for `./compose.sh`, and so on.

## First run

In this order, on a new machine:

1. `./vault.sh unpack` — extract `!ref/vault.7z` into `!ref/vault/` (7-Zip asks for the
   password, which comes from the maintainer).
2. `./cert.sh` — generate the development certificate and trust its CA. Then add
   `127.0.0.1 postit.local` to the hosts file (`C:\Windows\System32\drivers\etc\hosts` on
   Windows, `/etc/hosts` elsewhere; it needs an administrator).
3. `./compose.sh up` — start Postgres, Zitadel, and the nginx front door. Zitadel's first
   boot takes 15–30 seconds (`./compose.sh logs development postit-zitadel`).
4. `cargo xtask zitadel-bootstrap`, from `backend/` — create the OIDC applications and write
   `backend/config/local.toml`.
5. `./doctor.sh` — every row should be `PASS` (a `WARN` is advice); a `FAIL` row names its fix.

The README's [Getting started](../README.md#getting-started) has the same steps with more
detail, and what follows them (running the server, the web app, and the mobile app).

## The scripts

| Script               | For                                                                 | Commands                                                       |
| -------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------- |
| `compose.sh` / `.ps1` | The Docker Compose stack of one environment: the base file plus exactly one environment file, always | `up`, `down`, `restart`, `ps`, `logs`, `config`, `build` |
| `db.sh` / `.ps1`      | The postit database                                                 | `psql`, `backup`, `restore`, `drop`, `prepare`                 |
| `vault.sh` / `.ps1`   | The encrypted vault archive, `!ref/vault.7z`                        | `unpack`, `pack`, `check`                                      |
| `doctor.sh` / `.ps1`  | Is this machine ready, and is an environment's vault complete; read-only | `check [environment]` (the default)                     |
| `cert.sh` / `.ps1`    | The development TLS certificate for `https://postit.local`          | `setup` (the default), `gen`, `trust`                          |

Environments are `development` (the default wherever the environment is optional), `qa`,
and `production`. `db` needs the environment named, except `db prepare`, which always uses
the development server.

Exit codes are the same everywhere: `0` success; `1` a usage error, a refused guard, an
unconfirmed question, or a failure; a `compose` command that runs `docker compose` returns
its code; `vault check` returns `2` when the archive and `!ref/vault/` differ.

Rules every script keeps:

- **No secret reaches the terminal.** `compose config` keeps vault files as paths, `doctor`
  names keys and never values, and no password is passed on a command line (`vault` reads the
  archive password without echoing it and hands it to 7-Zip on its input; `db` works inside
  `postit-postgres` as the container's own user).
- **Destructive commands ask first.** `compose down --volumes` wants the environment name
  typed and `db drop` the database name; production needs `--force` for `down --volumes`
  and still asks, and `db drop production` refuses `--force`. An empty answer, or no input
  at all, is a no. Both also refuse when the running `postit-postgres` belongs to another
  environment than the one you name: every environment shares the container and the volume.
- **One owner per operation.** `compose` starts and stops the stack (`db drop` only pauses the
  running app services it needs out of the way, and starts them again), only `db` touches the
  database, and only `vault` touches the archive.

## Everyday recipes

**Start and stop.** `./compose.sh up` (add `--app` to run `postit-server` and the built web
app in containers), `./compose.sh down`, `./compose.sh ps`.

**Logs.** `./compose.sh logs` follows everything; `./compose.sh logs development
postit-zitadel --tail=100` one service.

**Reset the development database.** `./db.sh drop development` empties the postit database
and keeps Zitadel's; the server recreates the schema on its next boot. To wipe everything,
Zitadel's instance included: `./compose.sh down --volumes`, `./compose.sh up`, then
`cargo xtask zitadel-bootstrap` from `backend/` again.

**After pulling a change to `0001_init.sql`.** `./doctor.sh` reports a `migration` `FAIL`;
reset as above.

**Back up and restore.** `./db.sh backup development` writes
`!ref/backups/postit-development-<UTC>.dump` (owner-only, unencrypted user data: delete it
when done). `./db.sh restore development <file>` restores one in a single transaction; stop
the server first (`docker stop postit-server`, or `postit-api postit-worker` in qa and
production). See [docs/deployment.md](deployment.md#backups) for the rules outside
development.

**Refresh the sqlx offline cache.** After editing a `query!` or the migration:
`./db.sh prepare`, then commit the changed files under `backend/crates/*/.sqlx/`. It builds
a scratch database, `postit_prepare`, from the current migration, so a development database
that lags the migration cannot leak into the cache, and drops it afterwards, also on
failure. It needs `sqlx-cli` 0.9.0 (`doctor` prints the install command).

**Re-pack the vault after an edit.** `./vault.sh check` lists the files that differ from the
archive, by name; `./vault.sh pack` re-packs every environment, production included, and
replaces `!ref/vault.7z` only when 7-Zip succeeds. It asks for the password once and first
checks that it opens the current archive, so a typo cannot re-key the vault;
`./vault.sh pack --new-password` changes the password on purpose (asked twice). Commit the
archive.

**Is a qa or production vault complete?** `./doctor.sh check qa`; for production it also
checks the variables `docker-compose.production.yml` requires.

## What moved

`stack` and `drop-db` were replaced on 2026-11-07:

| Before                                      | Now                                                       |
| ------------------------------------------- | --------------------------------------------------------- |
| `stack up/down/restart/ps/logs/config/build [env] [--app]` | `compose …`, same arguments                 |
| `stack down -v`                             | `compose down --volumes` (now with a typed confirmation)  |
| `stack reset`                               | `compose down --volumes`, then `compose up`               |
| `stack psql`                                | `db psql development`                                     |
| `stack backup [env]`                        | `db backup <env>`                                         |
| `stack restore [env] <file>`                | `db restore <env> <file>`                                 |
| `stack doctor [env]`                        | `doctor check <env>` (development also gets the machine checks) |
| `drop-db <env> [--force] [--no-stop]`       | `db drop <env> [--force] [--no-stop]`                     |
| `7z x …` / `7z a …` by hand                 | `vault unpack` / `vault pack`                             |
| a scratch database by hand for `cargo sqlx prepare` | `db prepare`                                      |
| `cert.sh <directory>`                       | `POSTIT_CERT_DIR=<directory> ./cert.sh gen`               |

The design is [docs/specs/2026-11-07-operator-scripts-design.md](specs/2026-11-07-operator-scripts-design.md).

## For contributors

The scripts share `scripts/lib/common.sh` and `scripts/lib/common.ps1`: the repository
paths, the environment argument, reading a vault value as data, the compose `-f` chain, the
typed confirmation, and the `docker` command (`POSTIT_DOCKER` overrides it). Keep a change
to one half matched in the other.

`bash tests/scripts/run.sh` runs every test file in `tests/scripts/`. Each test runs the
Bash half and, when `pwsh` is on `PATH`, the PowerShell half against fake `docker`, `sqlx`,
`cargo`, `psql`, and `7z` executables (`tests/scripts/fakes/`), and checks that both exit
alike and call Docker alike. Bash runs as a stock Git Bash does, with MSYS path conversion
on. The `vault` tests need a real 7-Zip and the `cert` tests a real OpenSSL; both skip
without one, except in CI (`CI` set), where a missing tool fails. CI runs the suite in the
`docker` job.
