---
name: scripts
description: Run, inspect, back up, restore, or reset the postit Docker stack and its database, check the machine, and handle the vault and dev certificate through the root operator scripts compose, db, vault, doctor, and cert (.sh/.ps1 twins; compose layout, container naming, environment hostnames).
---

# postit operator scripts

Docker lives under `docker/`: base `docker-compose.yml` plus exactly one
`docker-compose.<environment>.yml` (`development`, `qa`, `production`), project name pinned
to `postit`, every service/container `postit-`-prefixed (`postit-postgres`, `postit-zitadel`,
`postit-server`, …) on network `postit-net` with volume `postit-pgdata`, and the image built
from `docker/server.Dockerfile` (context: repo root). Never type the `-f` chain by hand.

Six scripts at the repo root, each a `.sh` / `.ps1` twin with one job; flags are
`--force` / `-Force`, `--volumes` / `-Volumes`, `--app` / `-App`, `--no-stop` / `-NoStop`.
Each prints its help with `help`:

| Script    | For                                   | Commands                                        |
| --------- | ------------------------------------- | ----------------------------------------------- |
| `compose` | the stack, one environment at a time  | `up`, `down`, `restart`, `ps`, `logs`, `config`, `build` |
| `db`      | the postit database                   | `psql`, `backup`, `restore`, `drop`, `prepare`  |
| `vault`   | `!ref/vault.7z`                       | `unpack`, `pack`, `check`                       |
| `doctor`  | is this machine / vault ready         | `check [environment]` (the default)             |
| `cert`    | the dev TLS certificate               | `setup` (the default), `gen`, `trust`           |
| `js`      | the web client's pnpm packages (`web/`) | `install`, `update`, `build`, `check`, `clean`, `list`, `audit`, `outdated` (`-c` frozen lockfile, `-l` cross majors) |

```sh
./compose.sh up [environment] [--app]    # ./compose.ps1 up [environment] [-App] on Windows
./compose.sh config qa                   # resolved compose config, vault files by path
./compose.sh down --volumes              # asks for the environment name; dev also drops the cached Zitadel key
./doctor.sh [check] [environment]        # PASS/WARN/FAIL rows with fixes; read-only
./db.sh backup <environment>             # dump to !ref/backups/
./db.sh restore <environment> <file>     # restore a dump; the server must be stopped
./db.sh drop development                 # empty the postit database (not zitadel's)
./db.sh prepare                          # refresh every .sqlx cache on a scratch database
./vault.sh check                         # which vault files differ from the archive
```

Claude never runs `vault pack` against the real vault (the maintainer re-packs), and never
answers a destructive confirmation for the user. The qa and production hostnames are
`{api,app,auth}.qa.postit.com` and `{api,app,auth}.postit.com`. In development,
`./compose.sh up --app` runs `postit-server` behind `postit-nginx-app` (rebuild with
`./compose.sh build development --app`).

The full index, the first-run order, and everyday recipes are in `docs/scripts.md`. The
scripts share `scripts/lib/common.{sh,ps1}`; `bash tests/scripts/run.sh` tests both halves
of every script against fake executables. Secrets come from the vault — see the **Vault**
section of the root `CLAUDE.md`.
