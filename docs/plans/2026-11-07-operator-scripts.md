# Operator Scripts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `stack` and `drop-db` with five single-purpose root scripts (`compose`, `db`, `vault`, `doctor`, `cert`), each a Bash/PowerShell twin with the VigilixMambo help layout, indexed in `docs/scripts.md`.

**Architecture:** Each root script is a thin command dispatcher over a small shared library (`scripts/lib/common.sh`, dot-sourced `scripts/lib/common.ps1`) that owns the cross-script concerns: repository paths, the environment argument, reading a dotenv value as data, the vault files an environment needs, the compose `-f` chain, typed confirmations, and the `docker` command (overridable for tests). Behavior is ported from `stack.sh`/`stack.ps1` and `drop-db.sh`/`drop-db.ps1`, then those are deleted. One Bash test suite drives both twins against fake `docker` executables.

**Tech Stack:** Bash 4+ (Git Bash on Windows), PowerShell 7 (`pwsh`), Docker Compose, 7-Zip, OpenSSL, `sqlx-cli` 0.9.0.

**Spec:** `docs/specs/2026-11-07-operator-scripts-design.md`

## Global Constraints

- Every script is a twin: `<name>.sh` and `<name>.ps1` at the repository root, same commands, same help text apart from flag spellings (`--force`/`-Force`, `--volumes` `-v`/`-Volumes`, `--app`/`-App`, `--no-stop`/`-NoStop`), same exit codes.
- Help layout, printed by `help`, `-h`, `--help`, and a bare call: sections `USAGE`, `COMMANDS`, `OPTIONS`, `EXAMPLES`, `EXIT CODES`, `MORE`, first line `<name> - <one-line purpose>.`
- Exit codes: `0` success; `1` usage error, refused guard, unconfirmed confirmation, or failure; a pass-through command returns `docker compose`'s own code; `vault check` returns `2` when the archive differs.
- Environments: `development`, `qa`, `production`. No other values, no aliases.
- No vault value is ever printed: `compose config` keeps `--no-env-resolution`; `doctor` names keys only; no secret is passed on a command line.
- Paths resolve from the script's own location, so every script works from any directory; Bash keeps the `pwd -W` / `cygpath -m` handling so Git Bash hands `D:/…` paths to `docker.exe`.
- Confirmations read one line from standard input (`read -r` in Bash, `[Console]::In.ReadLine()` in PowerShell, never `Read-Host`), so tests can pipe the answer; end of input means "no".
- Container, service, volume, and project names, the compose files, and the vault layout do not change.
- Claude never runs `vault pack` against the real vault (the maintainer re-packs `!ref/vault.7z`); tests use throwaway vault folders and archives under `mktemp -d`.
- American English in code, comments, help text, and docs. Commits end with the attribution trailer of the model doing the work.
- `CARGO_TARGET_DIR=K:/cargo-target/postit` for any cargo run during this plan.

## Review Focus

1. **A destructive confirmation with no input** (a closed or empty stdin in CI or a pipe): `compose down --volumes` and `db drop` must abort with exit 1, never proceed. Tests: `down_volumes_aborts_on_empty_input` (Task 1), `drop_aborts_on_empty_input` (Task 2).
2. **Running a script from another directory:** `cd "$tmp" && "$root/compose.sh" config` must still pass the repository's compose files. Test: `compose_works_from_another_directory` (Task 1).
3. **Git Bash path form:** under MSYS the `-f` paths handed to `docker` must be `X:/…`, never `/x/…`. Test: `compose_passes_windows_paths_under_msys` (Task 1; skipped when `cygpath` is absent).
4. **A vault value that contains `$`, quotes, or `=`**: `doctor` must read it as data and never echo it. Test: the existing leak checks carried into `tests/scripts/doctor.sh` (Task 5), plus `doctor_never_prints_a_value_with_dollar_and_equals`.
5. **A failed `vault pack`** (wrong password, 7-Zip error) must leave the old `vault.7z` byte-for-byte unchanged, and `vault check` must leave no extracted copy behind, also on failure. Tests: `pack_failure_keeps_the_old_archive`, `check_removes_its_temporary_copy` (Task 4).

---

### Task 1: Shared library, test harness, and `compose`

**Files:**
- Create: `scripts/lib/common.sh`, `scripts/lib/common.ps1`, `compose.sh`, `compose.ps1`
- Create: `tests/scripts/lib.sh` (harness), `tests/scripts/fakes/docker` (Bash fake, executable), `tests/scripts/fakes/docker.ps1` (PowerShell fake), `tests/scripts/compose.sh`, `tests/scripts/run.sh` (runs every `tests/scripts/*.sh` except `lib.sh` and `run.sh`, exit 1 if any fails)

**Interfaces:**
- Produces in `scripts/lib/common.sh` (PowerShell names in parentheses, same behavior):
  - `repo_root` (`$RepoRoot`): absolute, Windows form under Git Bash.
  - `die <message>` (`Stop-Script <message>`): prints `<script>: <message>` to stderr, exits 1; the script name comes from `POSTIT_SCRIPT_NAME`, which each root script sets first.
  - `parse_environment <word>` (`Test-Environment`): prints the word when it is `development|qa|production`, else nothing.
  - `env_value <file> <key>` (`Get-EnvValue`): today's `stack.sh` `env_value` (last matching line, CR stripped, one pair of surrounding single or double quotes removed), moved here unchanged.
  - `vault_files <env>` (`Get-VaultFiles`): `postgres.env postit.env zitadel.env` for development, `postgres.env postit.env` otherwise.
  - `vault_dir` (`$VaultDir`): `POSTIT_VAULT_DIR` or `<root>/!ref/vault`.
  - `docker_cmd` (`$Docker`): `POSTIT_DOCKER` or `docker`; every script calls Docker only through it.
  - `compose_args <env>` (`Get-ComposeArgs`): `compose -f <root>/docker/docker-compose.yml -f <root>/docker/docker-compose.<env>.yml`.
  - `confirm <expected> <prompt>` (`Confirm-Typed`): prints the prompt to stderr, reads one line from stdin, returns 0 only on an exact match.
- Produces in `tests/scripts/lib.sh`:
  - `twin <script> <args...>`: runs `bash <root>/<script>.sh <args>` with `POSTIT_DOCKER=<fakes>/docker`, then, when `pwsh` is on `PATH`, `pwsh -NoProfile -File <root>/<script>.ps1 <args translated>` with `POSTIT_DOCKER=<fakes>/docker.ps1`; asserts both give the same exit code and that each fake log (one line per call, arguments space-joined, written to `$POSTIT_FAKE_DOCKER_LOG`) matches; exposes the Bash run's stdout, stderr, exit code, and log as `$out`, `$err`, `$code`, `$log`.
  - Flag translation for the PowerShell run: `--force`→`-Force`, `--volumes`/`-v`→`-Volumes`, `--app`→`-App`, `--no-stop`→`-NoStop`.
  - Fake control: `FAKE_DOCKER_CONFIG_FILES` (printed for `inspect … config_files`), `FAKE_DOCKER_PS` (printed for `ps --format`), `FAKE_DOCKER_FAIL` (an extended regex; a call matching it exits 1).
  - `write_vault <dir> <env>`: a complete fixture vault (moved from `docker/tests/stack-doctor.sh`).
  - `ok <name>` / `fail <name> <why>`: counting reporters; the file exits 1 when any test failed.
- Produces: `compose.sh <up|down|restart|ps|logs|config|build|help> [environment] [--app] [--volumes|-v] [--force] [docker compose arguments]`, behavior per the spec's `compose` table.

- [ ] **Step 1: Write the harness, both fakes, and the failing tests** in `tests/scripts/compose.sh`:
  - `help_and_bare_call_print_the_layout`: `twin compose help` and `twin compose` exit 0; `$out` starts with `compose - ` and contains `USAGE`, `COMMANDS`, `OPTIONS`, `EXAMPLES`, `EXIT CODES`, `MORE`.
  - `unknown_command_is_exit_1`: `twin compose frobnicate` exits 1, `$err` contains `unknown command: frobnicate`.
  - `up_builds_the_chain`: with a fixture vault and a fixture cert, `twin compose up qa` logs exactly one call, `compose -f <root>/docker/docker-compose.yml -f <root>/docker/docker-compose.qa.yml up -d`.
  - `up_development_app_adds_the_profile`: `twin compose up --app` logs `… --profile app up -d`.
  - `app_outside_development_is_refused`: `twin compose up qa --app` exits 1, nothing logged.
  - `up_without_vault_files_names_them`: an empty vault dir; `twin compose up qa` exits 1, `$err` lists `!ref/vault/qa/postgres.env` and `!ref/vault/qa/postit.env`, nothing logged.
  - `up_development_without_cert_is_refused`: `$err` names `./cert.sh`.
  - `config_never_resolves_env`: `twin compose config qa` logs `… config --no-env-resolution`.
  - `ps_logs_restart_see_every_profile`: each logs `--profile *`; `logs` adds `-f`; `twin compose logs development postit-zitadel --tail=10` passes `postit-zitadel --tail=10` through after `logs -f`.
  - `down_volumes_needs_the_typed_environment`: `printf 'development\n' | twin compose down --volumes` logs `… down -v` and removes a fixture `docker/zitadel/machinekey/postit-bootstrap.json` (pointed at a temp copy through `POSTIT_MACHINE_KEY`); `printf 'nope\n' | …` exits 1, nothing logged.
  - `down_volumes_aborts_on_empty_input` (Review Focus 1): `twin compose down --volumes </dev/null` exits 1, nothing logged.
  - `down_volumes_force_skips_the_prompt_outside_production`: `twin compose down qa --volumes --force </dev/null` logs `down -v`.
  - `production_down_volumes_needs_force_and_the_prompt`: without `--force` exit 1; with `--force` and `printf 'production\n'` it logs `down -v`; with `--force` and empty input exit 1.
  - `compose_works_from_another_directory` (Review Focus 2): run from `$tmp`, the logged `-f` paths are under the repository.
  - `compose_passes_windows_paths_under_msys` (Review Focus 3): when `cygpath` exists, every logged `-f` path matches `^[A-Za-z]:/`.
- [ ] **Step 2: Run** `bash tests/scripts/compose.sh` → FAIL (`compose.sh` missing).
- [ ] **Step 3: Implement** `scripts/lib/common.{sh,ps1}` and `compose.{sh,ps1}`: port `stack.sh`'s compose commands, `preflight`, and `forget_machine_key` (path overridable through `POSTIT_MACHINE_KEY` for tests), with the confirmation rules above. `reset`, `psql`, `doctor`, `backup`, `restore` are not compose commands. `stack` and `drop-db` stay until Task 7.
- [ ] **Step 4: Run** `bash tests/scripts/compose.sh` → PASS (both twins when `pwsh` is present; locally in Git Bash and with `pwsh`).
- [ ] **Step 5: Smoke against the real stack:** `./compose.sh ps` and `./compose.sh config qa` from the repo root; `config` shows `env_file` paths, no values.
- [ ] **Step 6: Commit:** `feat(scripts): shared library, twin test harness, and compose`.

---

### Task 2: `db psql`, `backup`, `restore`, `drop`

**Files:**
- Create: `db.sh`, `db.ps1`, `tests/scripts/db.sh`

**Interfaces:**
- Consumes: Task 1's library and harness (`twin`, the fakes, `FAKE_DOCKER_CONFIG_FILES`, `FAKE_DOCKER_PS`, `confirm`, `compose_args`).
- Produces: `db.sh <psql|backup|restore|drop|prepare|help> <environment> [options]`, `db.sh restore <environment> <file> [--force]`, `db.sh drop <environment> [--force] [--no-stop]`. `prepare` arrives in Task 3; `backup` keeps `POSTIT_BACKUP_DIR`; `drop` keeps `ADMIN_DATABASE_URL`.

- [ ] **Step 1: Write the failing tests** in `tests/scripts/db.sh`:
  - `help_layout` (as Task 1) and `environment_is_required`: `twin db backup` exits 1 with a usage line naming `<environment>`.
  - `psql_runs_in_the_container`: `twin db psql development` logs `exec -it postit-postgres sh -c exec psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"`.
  - `backup_refuses_another_environment`: `FAKE_DOCKER_CONFIG_FILES=…/docker-compose.qa.yml`, `twin db backup development` exits 1, `$err` contains `belongs to qa`.
  - `backup_refuses_an_unignored_repo_path`: `POSTIT_BACKUP_DIR=<root>/docs/tmp-dumps`, exits 1, the directory was not created.
  - `backup_writes_and_cleans_up`: with a git-ignored temp `POSTIT_BACKUP_DIR` outside the repository, the log holds the `pg_dump` exec, the `docker cp` out, and the in-container `rm -f /tmp/postit.dump`; the reported file name matches `postit-development-[0-9]{8}T[0-9]{6}Z\.dump`.
  - `restore_refuses_a_dump_named_for_another_environment`, `restore_refuses_while_the_server_runs` (`FAKE_DOCKER_PS=postit-api`), `production_restore_needs_force`.
  - `drop_qa_needs_no_stop`: `twin db drop qa --force` exits 1, `$err` contains `docker stop postit-api postit-worker`, nothing logged.
  - `drop_production_refuses_force`: exits 1.
  - `drop_development_stops_drops_recreates_restarts`: `printf 'postit\n' | twin db drop development` logs, in order, the compose `stop` of the app services, `DROP DATABASE IF EXISTS "postit" WITH (FORCE)`, `CREATE DATABASE "postit" OWNER …`, and the compose `start`; it never names `zitadel`.
  - `drop_aborts_on_empty_input` (Review Focus 1): `twin db drop development </dev/null` exits 1 and logs no `DROP`.
- [ ] **Step 2: Run** `bash tests/scripts/db.sh` → FAIL.
- [ ] **Step 3: Implement** by porting `stack.sh`'s `backup`, `restore`, `running_environment`, `require_running_environment`, `drop_container_dump`, `abs_path`, and `psql`, and `drop-db.sh`/`drop-db.ps1` whole, onto the library.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Smoke:** `./db.sh backup development` against the running dev stack writes a dump under `!ref/backups/`; delete it afterwards.
- [ ] **Step 6: Commit:** `feat(scripts): db psql, backup, restore, and drop`.

---

### Task 3: `db prepare`

**Files:**
- Modify: `db.sh`, `db.ps1`, `tests/scripts/db.sh`

**Interfaces:**
- Consumes: Task 2's dispatcher; `env_value` for `DATABASE_URL` in `!ref/vault/development/postgres.env`.
- Produces: `db.sh prepare` (no environment): the scratch database `postit_prepare`, then for each `backend/crates/*/.sqlx/` the command `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` run from that crate with `DATABASE_URL` set to the vault URL with its host swapped to `127.0.0.1` and its database to `postit_prepare`. The URL goes through the environment, never the command line. `sqlx` and `cargo` come from `POSTIT_SQLX` and `POSTIT_CARGO` (default `sqlx`, `cargo`) so the test can fake them.

- [ ] **Step 1: Write the failing tests:**
  - `prepare_creates_migrates_prepares_and_drops`: fakes for docker, `sqlx`, and `cargo` that log their calls and the `DATABASE_URL` database name only; the log shows, in order, the in-container `CREATE DATABASE "postit_prepare"`, `sqlx migrate run --source <root>/backend/crates/data/migrations`, `cargo sqlx prepare -- --all-targets --all-features` from `backend/crates/data`, and `DROP DATABASE IF EXISTS "postit_prepare" WITH (FORCE)`.
  - `prepare_drops_the_scratch_database_on_failure`: the cargo fake exits 1; `db prepare` exits 1 and the log still ends with the `DROP`.
  - `prepare_needs_sqlx`: `POSTIT_SQLX=<missing path>` exits 1 with the install hint `cargo install sqlx-cli --version 0.9.0 --no-default-features --features rustls,postgres --locked`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Real run:** `./db.sh prepare` from the repo root against the dev Postgres; `git status backend/crates/data/.sqlx` shows no change, and `postit_prepare` no longer exists afterwards.
- [ ] **Step 6: Commit:** `feat(scripts): db prepare refreshes the sqlx cache on a scratch database`.

---

### Task 4: `vault`

**Files:**
- Create: `vault.sh`, `vault.ps1`, `tests/scripts/vault.sh`

**Interfaces:**
- Consumes: the library (`vault_dir`, `die`).
- Produces: `vault.sh <unpack|pack|check|help> [--force]`. The archive is `POSTIT_VAULT_ARCHIVE` or `<root>/!ref/vault.7z`; 7-Zip is `POSTIT_7Z`, else `7z` on `PATH`, else `C:/Program Files/7-Zip/7z.exe` on Windows; missing, it exits 1 naming the 7-Zip download and the GUI fallback the README describes.
  - `unpack`: `7z x` into the archive's folder; refuses when `!ref/vault/` holds any file other than `.gitkeep`, unless `--force`.
  - `pack`: `7z a -p -mhe=on -xr!.gitkeep <tmp archive> <vault>/*`, every environment included; on success move the temporary archive over `vault.7z`; on failure delete it and leave `vault.7z` as it was.
  - `check`: extract into a `mktemp -d` directory outside the repository (owner-only), compare every file with `!ref/vault/` by SHA-256 both ways, print the differing relative paths only, remove the directory on every exit path; exit 0 equal, 2 different.
  - Passwords are always 7-Zip's own prompt; tests pass one with `POSTIT_7Z_TEST_PASSWORD`, which the scripts forward as `-p<password>` only when it is set (test-only; the help does not mention it).

- [ ] **Step 1: Write the failing tests** (skip the file with a message when no 7-Zip is found): `pack_then_check_is_equal`, `check_reports_a_changed_file` (exit 2, `$out` names `qa/postit.env`, never its contents), `unpack_refuses_an_existing_vault`, `unpack_force_overwrites`, `pack_leaves_out_gitkeep`, `pack_includes_production`, `pack_failure_keeps_the_old_archive` (Review Focus 5: a fake 7-Zip that exits 2 during `a`; `vault.7z`'s SHA-256 is unchanged and no temporary archive is left), `check_removes_its_temporary_copy` (Review Focus 5: `TMPDIR` pointed at a fresh directory, empty again after `check`, also after a forced failure).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit:** `feat(scripts): vault unpack, pack, and check`.

---

### Task 5: `doctor`

**Files:**
- Create: `doctor.sh`, `doctor.ps1`, `tests/scripts/doctor.sh` (from `docker/tests/stack-doctor.sh`)
- Delete: `docker/tests/stack-doctor.sh`
- Modify: `.github/workflows/ci.yml` (the `docker` job's `stack doctor tests` step becomes `script tests`, running `bash tests/scripts/run.sh`; `pwsh` is on `ubuntu-latest`, so both twins run)

**Interfaces:**
- Consumes: the library (`env_value`, `vault_files`, `vault_dir`, `docker_cmd`).
- Produces: `doctor.sh [check] [environment]`, read-only. Rows are `PASS|WARN|FAIL|SKIP  <check>  <finding>`; a `FAIL` or `WARN` row ends with ` -> <fix>`; `SKIP (needs <check>)` when a prerequisite failed. Exit 1 when any row is `FAIL`, else 0.
  - Check names, in order, development: `tools`, `vault`, `certificate`, `certificate-trusted`, `hosts`, `postgres`, `migration`, `identity`, `zitadel-bootstrap`, `web-packages`. qa and production: `vault`, and for production `compose-variables`.
  - `vault` carries every rule of today's `stack doctor` unchanged (one finding per problem, key names only).
  - `tools`: `FAIL` for a missing `docker`, `openssl`, `cargo`, `pnpm`; `WARN` for `7z`, `sqlx`, `flutter`, host `psql`.
  - `migration`: the `checksum` of version 1 in `_sqlx_migrations` (read with `psql` inside `postit-postgres`) equals the SHA-384 of `backend/crates/data/migrations/0001_init.sql`; else `FAIL … -> reset the development database (./compose.sh down --volumes, then ./compose.sh up)`.
  - `identity`: `https://postit.local:44300/.well-known/openid-configuration` answers 200 (curl with the dev CA, 5 s timeout). `zitadel-bootstrap`: `backend/config/local.toml` exists. `web-packages`: `web/node_modules` exists.
  - The network and Docker checks honor `POSTIT_DOCTOR_OFFLINE=1` (they report `SKIP (offline)`), so the tests stay offline.

- [ ] **Step 1: Move the tests** from `docker/tests/stack-doctor.sh` into `tests/scripts/doctor.sh` on the harness, calling `twin doctor check <env>` with `POSTIT_DOCTOR_OFFLINE=1`; each old assertion keeps its meaning, rewritten from `stack: … is not ready` lines to `FAIL  vault  …` rows. Add `doctor_never_prints_a_value_with_dollar_and_equals` (Review Focus 4: a password `p$a=ss'"w` absent from `$out` and `$err`), `rows_have_the_fixed_shape` (every line matches `^(PASS|WARN|FAIL|SKIP)  [a-z-]+  `), `fail_rows_name_a_fix`, `qa_runs_vault_checks_only`, `migration_checksum_mismatch_names_the_reset` (a fake docker printing a wrong checksum, offline flag off for that check only through `POSTIT_DOCTOR_ONLY=migration`).
- [ ] **Step 2: Run** `bash tests/scripts/doctor.sh` → FAIL. **Step 3: Implement.** **Step 4: Run** `bash tests/scripts/run.sh` → PASS.
- [ ] **Step 5: Real run:** `./doctor.sh` on this machine; every row either passes or names a fix that is true.
- [ ] **Step 6: Commit:** `feat(scripts): doctor checks this machine and an environment's vault`.

---

### Task 6: `cert`

**Files:**
- Modify: `cert.sh`, `cert.ps1`
- Create: `tests/scripts/cert.sh`

**Interfaces:**
- Produces: `cert.sh [setup|gen|trust|help]`, `setup` the default. `gen` keeps today's output in `docker/shared/nginx/certs/` (`postit-dev-ca.{key,crt}`, `postit.local.{key,crt}`, SAN `DNS:postit.local,DNS:*.postit.local`), writing to `POSTIT_CERT_DIR` when set; `trust` installs `postit-dev-ca.crt` (Windows: the current user's Root store through `Import-Certificate`, from either twin via `pwsh`; Linux: prints the `sudo` command); `setup` = `gen` then `trust`. An existing CA is reused, as today.

- [ ] **Step 1: Write the failing tests** (need `openssl`): `help_layout`, `gen_writes_the_four_files` (with `POSTIT_CERT_DIR=$tmp`), `gen_keeps_an_existing_ca` (the CA's SHA-256 unchanged after a second `gen`), `leaf_has_the_san` (`openssl x509 -noout -ext subjectAltName` lists both names), `unknown_command_is_exit_1`. `trust` is not run in tests.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS.
- [ ] **Step 5: Commit:** `feat(scripts): cert gen, trust, and setup`.

---

### Task 7: Remove `stack` and `drop-db`, and the docs

**Files:**
- Delete: `stack.sh`, `stack.ps1`, `drop-db.sh`, `drop-db.ps1`
- Create: `docs/scripts.md`
- Rename: `git mv .claude/skills/stack .claude/skills/scripts` (frontmatter `name: scripts`, description naming all five scripts; body: the compose layout and naming it has today, the script table, and a pointer to `docs/scripts.md`)
- Modify: `README.md` (the root file table, "Multi-environment usage", the quick start, "Resetting the development stack", the command table near line 574, the database section that says `migrate` is not written yet, the troubleshooting entry for `stack.sh` from PowerShell, the vault extraction steps that use `7z` by hand), `CLAUDE.md` (the Docker launcher paragraph and the Vault paragraph), `docs/deployment.md` (doctor, up, backup, restore), `docs/contracts/01. vision and architecture.md` (the repository layout lines), `docs/contracts/02. foundation.md` (Docker dev and the P9 line), `docs/contracts/04. infrastructure and tooling.md` (objective, V2 example, V3 renamed to `db drop`, V4 now "`db migrate`, at G1 when the single-migration policy ends", and a dated note that the scripts follow VigilixMambo's one-job-per-script layout), the header comments of `docker/docker-compose{,.development,.qa,.production}.yml`, the `stack` mentions in `docker/.env.example` and `docker/shared/nginx/nginx.conf`, and the hint in `backend/xtask/src/zitadel_bootstrap.rs` (`./compose.ps1 up` or `./compose.sh up`)

**Interfaces:**
- `docs/scripts.md` follows Mambo's page: a short intro, "First run" (in order: `vault unpack`, `cert setup` plus the hosts entry, `compose up`, `cargo xtask zitadel-bootstrap` from `backend/`, `doctor`), "The scripts" (one table row per script: what it is for, commands, help), "Everyday recipes" (start, stop, logs, reset the database, back up and restore, refresh the sqlx cache, re-pack the vault after an edit), and "What moved" (the spec's table from `stack`/`drop-db` to the new commands).

- [ ] **Step 1: Delete, rename, and write** the files above. Historical plans and specs under `docs/plans/` and `docs/specs/` keep their old command names.
- [ ] **Step 2: Check no live reference is left:** `git grep -nE 'stack\.(sh|ps1)|drop-db|stack (up|down|doctor|backup|restore|psql|reset)' -- ':!docs/plans' ':!docs/specs' ':!.superpowers'` prints nothing.
- [ ] **Step 3: Gates:** `bash tests/scripts/run.sh` → PASS (both twins); `cargo fmt --check` and `cargo clippy -p xtask --all-targets -- -D warnings` (from `backend/`) → PASS; `./doctor.sh` and `./compose.sh help` run from another directory.
- [ ] **Step 4: Fix every issue the gates or the final review find**, minor ones included; an agent-doable item that cannot be finished goes to `docs/plans/2026-11-07-operator-scripts-deferred.md`, a human-only one (for example, trusting the CA on a real machine with `cert trust`) to `docs/plans/2026-11-07-operator-scripts-verification.md`.
- [ ] **Step 5: Commit:** `docs(scripts): one job per script, stack and drop-db removed`.
