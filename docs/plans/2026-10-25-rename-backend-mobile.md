# Rename top-level folders: `server/` → `backend/`, `app/` → `mobile/`, `api/` folded into `backend/` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The top-level folders say what they hold: `backend/` (the Rust workspace: API, worker, mail webhook, xtask), `mobile/` (the Flutter app), `web/` (unchanged), and the lone `api/openapi.json` moves to `backend/openapi.json`, so `api/` disappears.

**Architecture:** A pure move-and-repoint change with no behavior change. Move with `git mv` so history follows; fix every path that crosses a folder boundary (xtask codegen, web and Dart codegen, Docker build contexts, stack scripts, CI, docs); then prove nothing still points at the old paths with a repo-wide grep and the full gates.

**Tech Stack:** git, Cargo workspace, Docker Compose, GitHub Actions, pnpm (PowerShell on Windows), Flutter 3.47.6.

**Spec:** the user's request in the 2026-10-25 session ("do the rename as a plan, use backend"; `app/` → `mobile/` accepted as the suggestion). No design document.

## Global Constraints

- New names are exactly `backend/`, `mobile/`, `web/`. `api/` ceases to exist; the OpenAPI document lives at `backend/openapi.json`.
- Not renamed (names that are not folders): crate and package names (`postit-*`, bin `postit`), the crate directory `backend/crates/server` (crate `postit-server`), Docker Compose service and container names, `docker/server.Dockerfile`, the Flutter package name, application IDs and bundle IDs, the `app` CI job name only if renaming it breaks branch-protection required checks (see Task 2).
- `!ref/vault/` stays at `!ref/vault/`; `POSTIT_SECRETS_FILE` default must still resolve to it from `backend/`. `backend/` and `mobile/` sit at the same depth as `server/` and `app/` did, so `../` prefixes to `!ref/` and `docker/` stay valid. Only paths that went through `api/` change depth.
- Every document and plan is updated to the new structure, so nothing a reader opens still names `server/`, `app/` or `api/` as a folder: root `CLAUDE.md`, `web/CLAUDE.md`, `README.md`, `docs/**` (including `docs/adr/` and `docs/agents/`), `CONTEXT.md`, `docs/plans/**` including `archive/`, `docs/design/**`, every dated plan under `docs/plans/` and any spec beside them, `.github/*`, and any project skill under `.claude/`. Historical plans keep their meaning: only folder paths change, never prose such as "web app" or "the server", and no dated plan's tasks, dates or decisions are reworded.
- American English. Use pnpm, never npm; run pnpm through PowerShell on Windows.
- Move with `git mv` in a single commit per folder so `git log --follow` works. Close editors, `rust-analyzer`, Flutter, Gradle daemons and any process holding the folders first (Windows refuses to move a locked directory).
- Per the user's standing rule, any issue found at the end is fixed, not deferred; only a truly blocked issue goes to `docs/plans/2026-10-25-rename-backend-mobile-deferred.md`.
- No behavior change: no test is edited except for paths.

## Review Focus

1. **Paths that change depth.** Everything that read `../api/openapi.json` or `api/openapi.json` now needs `openapi.json` relative to `backend/` (xtask), `../backend/openapi.json` (web, mobile). A wrong prefix passes `cargo check` and fails only when the generator runs. Pinned by `cargo xtask openapi --check` (Task 1 Step 6).
2. **Docker build contexts and `COPY` lines.** A stale `server/` in `docker/server.Dockerfile`, a compose `build.context`, or `.dockerignore` builds an empty or wrong image and only fails in `docker build`/CI. Pinned by Task 2 Step 5 (build the image) and the `docker` CI job.
3. **CI path filters and working directories.** `.github/workflows/ci.yml` can silently stop running a job when a `paths:` filter or `working-directory:` still names the old folder. Pinned by Task 2 Step 3 (grep plus reading every `paths`/`working-directory`/`cache` key).
4. **Generated-file headers and caches.** `web/src/api/schema.d.ts`, `mobile/lib/api/`, and `.dart_tool/`, `build/` hold absolute or old relative paths. Pinned by regenerating (`cargo xtask openapi`) and `flutter pub get`, and by `git diff --stat` showing the regeneration changed nothing semantic.
5. **Stale references that cannot fail a build.** Docs, scripts' help text, `.gitignore` entries, the memory file `test-env-gotchas.md`, and `CLAUDE.md` guidance. Pinned by the leftover-path grep in Task 3 Step 6.

---

### Task 1: Move the folders and repoint the in-tree codegen paths

**Files:**
- Move: `server/` → `backend/`; `app/` → `mobile/`; `api/openapi.json` → `backend/openapi.json` (then the empty `api/` is gone)
- Modify: `backend/xtask/src/openapi.rs`, `backend/xtask/src/zitadel_bootstrap.rs` (paths that mention `../web`, `../docker`, `server/`, `api/`), `backend/.cargo/config.toml`, `mobile/swagger_parser.yaml`, `web/package.json` and `web/*.config.*` if they name `../api` or `../server`, `.gitignore`, `.gitattributes`, `.dockerignore`
- Test: existing tests only; add one xtask unit test if the OpenAPI output path is computed (see Step 4)

**Interfaces:**
- Consumes: nothing.
- Produces: the new layout every later task assumes: `backend/` (workspace root with `Cargo.toml`, `crates/`, `xtask/`, `.cargo/`, `config/`, `openapi.json`), `mobile/`, `web/`. `cargo xtask openapi` reads/writes `backend/openapi.json`, `web/src/api/schema.d.ts`, `mobile/lib/api/`.

- [ ] **Step 1: Preflight.** From a clean `stage`-based branch `rename-backend-mobile` (`git switch -c rename-backend-mobile`), confirm `git status --short` is empty and no process holds the folders (close IDEs; stop `cargo`, `flutter`, `gradle --stop`, `pnpm dev`, the postit process). Record the baseline: run `git grep -nE '(^|[^A-Za-z0-9_./-])(server|app|api)/' -- . ':!web/node_modules' | wc -l` and keep the number for Task 3.
- [ ] **Step 2: Move.** `git mv server backend`, `git mv app mobile`, `git mv api/openapi.json backend/openapi.json` (then `rmdir api` if left). Commit as `refactor: move server/ to backend/, app/ to mobile/, api/openapi.json into backend/` with the move only, so rename detection is clean (no content edits in this commit).
- [ ] **Step 3: Find every in-tree reference.** `git grep -nE '(\.\./|\./|^|[ "'"'"'=(])(server|app|api)/(openapi|crates|xtask|\.cargo|config|lib|pubspec|tool|android|ios|swagger)' -- .` and list the hits; each is fixed in the next steps or in Tasks 2–3.
- [ ] **Step 4: Repoint xtask.** In `backend/xtask/src/openapi.rs` the OpenAPI document path becomes `backend/openapi.json` relative to the repo root (computed from the workspace root, i.e. `openapi.json` next to the workspace `Cargo.toml`); the web generator target stays `web/src/api/schema.d.ts` and the Dart generator target becomes `mobile/`. In `zitadel_bootstrap.rs` and any other xtask module, replace the repo-relative strings (`../web/public/...`, `../!ref/vault/...`, `../docker/...`) only where they named `server`/`app`/`api`; the `../` depth did not change for those that start from `backend/`. If a function builds the OpenAPI path, extract it as `fn openapi_path(root: &Path) -> PathBuf` and add a unit test `openapi_path_is_backend_openapi_json` asserting `root.join("backend/openapi.json")` semantics for the workspace-root-relative form it uses.
- [ ] **Step 5: Repoint the other generators and ignore files.** `mobile/swagger_parser.yaml`: `schema_path` becomes `../backend/openapi.json`. `web/`: every reference to `../api/openapi.json` (package scripts, `openapi-typescript` invocation, docs in `web/CLAUDE.md` handled in Task 3) becomes `../backend/openapi.json`. `.gitignore`, `.gitattributes`, `.dockerignore` (root and `docker/`): replace `server/target`, `app/build`, `app/.dart_tool`, `api/` and similar entries with the new folder names; keep every ignore that existed.
- [ ] **Step 6: Regenerate and verify.** From `backend/` run `cargo fmt --check`, `cargo check --workspace --all-targets`, then `cargo xtask openapi` followed by `cargo xtask openapi --check` (needs `pnpm install` in `web/` through PowerShell and `flutter pub get` in `mobile/`). Expected: exit 0, and `git diff --stat` shows no change to `backend/openapi.json`, `web/src/api/schema.d.ts`, or `mobile/lib/api/` other than generated-header path text if any. Then `pnpm typecheck` in `web/` (PowerShell) and `flutter analyze` in `mobile/`. Use the separate `CARGO_TARGET_DIR` and DB URL from the `test-env-gotchas` memory.
- [ ] **Step 7: Commit** `refactor: repoint codegen and ignore files at backend/ and mobile/`.

---

### Task 2: Docker, launcher scripts, CI, and deploy files

**Files:**
- Modify: `docker/server.Dockerfile`, `docker/docker-compose.yml`, `docker/docker-compose.development.yml`, `docker/docker-compose.qa.yml`, `docker/docker-compose.production.yml`, `docker/ci/*`, `docker/tests/*`, `docker/postgres/*` and `docker/shared/*` if they name old folders, `stack.sh`, `stack.ps1`, `drop-db.sh`, `drop-db.ps1`, `cert.sh`, `cert.ps1`, `deploy/**`, `.github/workflows/ci.yml`, `.github/pull_request_template.md`, `mobile/tool/android-dev.ps1`, `mobile/tool/android-dev.sh`
- Test: the `docker/tests/*` scripts, `./stack.ps1 up` smoke, a local `docker build`

**Interfaces:**
- Consumes: Task 1's layout.
- Produces: a working image build, stack, and CI whose commands, caches, path filters and artifacts name `backend/`, `mobile/`, `web/`.

- [ ] **Step 1: Dockerfile and compose.** In `docker/server.Dockerfile` replace `COPY server/…`, `WORKDIR`, and `cargo` paths with `backend/`; the web stage copies `web/` and now reads the OpenAPI document from `backend/openapi.json` if it does. Compose `build.context`/`dockerfile`, bind-mount sources, and `env_file` paths: only entries that name `server/`, `app/` or `api/` change. Keep service names, container names and image names.
- [ ] **Step 2: Launcher and helper scripts.** In `stack.sh`, `stack.ps1`, `drop-db.*`, `cert.*`, and `docker/tests/*`, replace the folder names in paths and help text (for example the `cd server` before running `cargo`, the `server/config` reads, the `.cargo` lookups). The `stack` skill text under `.claude/` is handled in Task 3.
- [ ] **Step 3: CI.** In `.github/workflows/ci.yml` update every `working-directory:`, `paths:`/`paths-ignore:` filter, cache `key`/`path` (`server/target`, `server/Cargo.lock`, `app/pubspec.lock`, `web/pnpm-lock.yaml`), artifact path, and run-step `cd`. Read the whole file after editing and confirm each job still triggers on the folders it builds, and that the `contract` job compares `backend/openapi.json`. Keep job ids and names unchanged (branch protection may require them); if a job is named `app`, keep the id and set a display `name: mobile` only if no required check depends on it, otherwise leave it and note it in the PR body. Update `.github/pull_request_template.md` text. Run `actionlint` on the file if available (`actionlint -version`); otherwise note it was read by eye.
- [ ] **Step 4: Deploy files.** Update `deploy/**` (and any systemd/Terraform stubs) that name the old folders.
- [ ] **Step 5: Verify.** Run `docker build -f docker/server.Dockerfile -t postit-rename-check .` (use `--build-arg` flags the compose file passes; expected: success), `docker/tests/stack-doctor.sh` (expected: all doctor tests passed), then `./stack.ps1 up` and confirm every container reaches healthy, `./stack.ps1 down` afterward. Expected: no error mentioning `server/`, `app/` or `api/`.
- [ ] **Step 6: Commit** `refactor: point Docker, launchers, CI, and deploy files at backend/ and mobile/`.

---

### Task 3: Documentation, guidance, skills, and memory

**Files:**
- Modify: `CLAUDE.md`, `web/CLAUDE.md`, `mobile/CLAUDE.md` if present, `README.md`, `docs/*.md`, `docs/agents/*`, `CONTEXT.md`, `docs/adr/*` only where they name folders as live guidance (ADR decisions keep their text if the folder name is part of what was decided, adding a one-line "renamed to `backend/` on 2026-10-25" note instead), all of `docs/plans/**` including `archive/`, `docs/design/**`, every dated plan and spec under `docs/superpowers/`, `.claude/**` project skills (the `stack` skill), the user memory file `C:\Users\asm\.claude\projects\D--Work-rust-postit\memory\test-env-gotchas.md` and any other memory file naming `server/`, `app/` or `api/`
- Test: a repo-wide grep (Step 6)

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: guidance that matches the repo. `CLAUDE.md` gains a short "Layout" paragraph: `backend/` is the Rust workspace (API, worker, internal mail listener, xtask), `web/` the React client, `mobile/` the Flutter client, `docker/` the stack, `backend/openapi.json` the generated API contract.

- [ ] **Step 1: Root `CLAUDE.md`.** Replace every live path: crate map "under `server/`", `cargo dev` "from `server/`", `app/` commands "from `app/`", `api/openapi.json`, the vault paths section, and the Flutter command block (`mobile/`). Add the Layout paragraph. Keep the sentence that D0c is built; do not otherwise reword.
- [ ] **Step 2: Other `CLAUDE.md` and README.** `web/CLAUDE.md` (OpenAPI source path, `cd` hints), `README.md` (quick start for the three clients: `./stack.ps1 up`, `cd backend && cargo dev`, `cd web && pnpm dev`, `cd mobile && flutter run …`), and `docs/deployment.md`, `docs/configuration.md`, `docs/security.md`, `docs/agents/*`.
- [ ] **Step 3: Plans and designs.** Rewrite folder paths in `docs/plans/**` (the four live plans and `archive/`), `docs/design/**`, and every dated plan or spec under `docs/plans/` and `docs/superpowers/` (except this plan, which names the old folders on purpose). Rules: change only folder paths (`server/…`, `app/…`, `api/openapi.json`, `cd server`, `../api`), never prose ("web app", "the server", "API"), and never reword tasks, dates or decisions. Do it with an anchored script (the Step 6 pattern plus `cargo` workdir mentions), then read `git diff` for each file class and revert any hit that was prose. Where a historical plan describes the old layout as a fact (for example "the monorepo has `server/`"), rewrite the path and add nothing else; plan 02's P1 monorepo section gets one added sentence: "Renamed on 2026-10-25: `server/` → `backend/`, `app/` → `mobile/`, `api/openapi.json` → `backend/openapi.json`."
- [ ] **Step 4: Skills.** Update the `stack` skill and any other skill under `.claude/` (project scope) that names the folders.
- [ ] **Step 5: Memory.** Update `test-env-gotchas.md` (and any other memory file that names the folders) so recalled commands use `backend/`, `mobile/`.
- [ ] **Step 6: Leftover-path grep.** Run `git grep -nE '(^|[^A-Za-z0-9_./-])(server|app|api)/(openapi|crates|xtask|\.cargo|config|lib|pubspec|tool|android|ios|swagger|target|build)|cd (server|app|api)\b|\.\./(server|app|api)\b' -- . ':!web/node_modules' ':!docs/plans/2026-10-25-rename-backend-mobile.md'`. Expected: no output. Each remaining hit is either fixed or is a deliberate non-folder use (for example the word "server" in prose, `backend/crates/server`, `docker/server.Dockerfile`), which does not match this pattern; if one still matches legitimately, tighten the pattern in this step rather than leaving the hit.
- [ ] **Step 7: Commit** `docs: describe the backend/, mobile/, web/ layout`.

---

### Task 4: Full verification and finish

**Files:**
- Modify: nothing expected; fixes go back to the owning task's files
- Test: the full gates

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: a branch ready to merge to `stage`.

- [ ] **Step 1: Clean build.** Delete only this task's own scratch target directory, then from `backend/` run, with the `test-env-gotchas` environment: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo xtask openapi --check`. Expected: all exit 0.
- [ ] **Step 2: Web.** From `web/` through PowerShell: `pnpm install --frozen-lockfile`, typecheck, lint, format check, `pnpm test`, `pnpm build`. Expected: all pass.
- [ ] **Step 3: Mobile.** From `mobile/`: `flutter pub get`, `dart format --output=none --set-exit-if-changed lib/main.dart lib/data lib/state lib/ui test integration_test`, `flutter analyze`, `flutter test`. Expected: all pass. If an Android Gradle build is available, `flutter build apk --debug --flavor development --dart-define-from-file=config/development.json` succeeds (stale `build/` and `.dart_tool/` are removed first because they hold old absolute paths).
- [ ] **Step 4: Stack smoke.** `./stack.ps1 up`, `cargo xtask zitadel-bootstrap --check` exits 0, `cargo dev` from `backend/` starts and `GET https://postit.local:44310/health` (or the repo's health route) answers 200, then stop everything.
- [ ] **Step 5: Final leftover grep** (Task 3 Step 6) and `git status --short` empty; `git log --follow --oneline -3 backend/Cargo.toml` shows pre-rename history.
- [ ] **Step 6: Review and finish.** One whole-branch review on the most capable model focused on Review Focus 1–5, then `superpowers:finishing-a-development-branch`. Tell the user: close editors before pulling the branch, run `flutter pub get` in `mobile/` and `pnpm install` in `web/` once, and expect rust-analyzer to re-index. CI job renames (if any) may need a branch-protection update.
