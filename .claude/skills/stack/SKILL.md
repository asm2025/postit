---
name: stack
description: Run, inspect, back up, restore, or reset the postit Docker stack via stack.sh / stack.ps1 (compose layout, container naming, environment hostnames).
---

# postit Docker stack

Docker lives under `docker/`: base `docker-compose.yml` plus exactly one
`docker-compose.<environment>.yml` (`development`, `qa`, `production`), project name pinned
to `postit`, every service/container `postit-`-prefixed (`postit-postgres`, `postit-zitadel`,
`postit-server`, …) on network `postit-net` with volume `postit-pgdata`, and the image built
from `docker/server.Dockerfile` (context: repo root). Never type the `-f` chain by hand —
use the launcher from the repo root:

```sh
./stack.sh up [environment] [--app]      # ./stack.ps1 up [environment] [-App] on Windows
./stack.sh config qa                     # resolved compose config, no side effects
./stack.sh down -v                       # dev: also wipes the DB and the cached Zitadel key
./stack.sh doctor [environment]          # check vault files and required values
./stack.sh backup [environment]          # dump the database to a file
./stack.sh restore [environment] <file>  # restore a dump into the stack
./stack.sh help                          # ps, logs, restart, build, psql, reset
```

The qa and production hostnames are `{api,app,auth}.qa.postit.com` and `{api,app,auth}.postit.com`. In
development, `./stack.sh up --app` runs `postit-server` behind `postit-nginx-app` (rebuild with
`./stack.sh build development --app`).

Secrets come from the vault — see the **Vault** section of the root `CLAUDE.md`.
