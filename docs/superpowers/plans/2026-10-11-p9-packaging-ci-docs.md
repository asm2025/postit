# P9: QA and production packaging, CI completion, and docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the one `postit-server` image deployable on any container host. The server refuses an incomplete qa/production configuration, the qa/production compose files reproduce an environment locally, the launcher gains `doctor`, `backup` and `restore`, CI gains the `web`, `contract` and `docker` jobs, and operators get a host-neutral deployment guide and an environment variable reference.

**Architecture:** The deliverable is the image plus its environment contract: roles `api` and `worker`, ports 8080/8081/8082, `/health` and `/ready`, `POSTIT__…` variables, and secrets as env vars or `_FILE`. Startup requirements are enforced in `postit-config` validation, so they hold on ECS, Container Apps or a VM alike. The qa and production compose files are the reference single-host deployment and the way to reproduce an environment on a laptop (as in VigilixKC, "Multi-environment usage"). Cloud-specific infrastructure (Terraform, managed secrets, load balancers) is out of scope until a hosting provider is chosen; it gets its own plan.

**Tech Stack:** Rust 2024 (`postit-config`, figment, serde), Docker Compose, bash + PowerShell 7 (launcher), GitHub Actions (`pnpm/action-setup`, `docker/build-push-action`, `tufin/oasdiff`, `wiremock/wiremock`), Markdown docs.

**Spec:** `!ref/plans/02. foundation.md`, phase P9 plus "QA and production hosting", "Testing and CI" and the `postit-config` section. The amendments below were agreed with the user on 2026-10-11 and replace P9's second exit criterion (Task 10 writes them into plan 02).

## Spec amendments decided while planning

1. **Hosting is undecided (AWS, Azure, or other).** P9 makes the image cloud-ready and host-neutral, with no cloud-specific infrastructure. Vercel cannot host the server, worker or Postgres. A later plan covers the chosen provider.
2. **Compose is local reproduction, not the deployment path.** `docker-compose.qa.yml` and `docker-compose.production.yml` stay complete and runnable (the reference single-VM deployment), but the docs present them as one option among container hosts.
3. **"A QA-config stack comes up behind a local proxy against the dev Zitadel" is replaced.** The new criterion is the CI `docker` job: it boots the built image as separate `api` and `worker` containers against Postgres and a wiremock OIDC issuer, and checks `/health`, `/ready` and `/config.json`. `stack up qa` remains the manual local reproduction. Running qa next to the dev Zitadel conflicts with "one host runs one environment" (same project, same `postit-postgres`).
4. **"Production refuses to start without …" lives in `postit-config` validation and the startup bootstrap check, not only in compose.** The production compose file also fails fast at interpolation (`${VAR:?…}`) for the deployment-specific values.
5. **New launcher commands, borrowed from VigilixKC:** `doctor` (checks vault files and keys with no Docker and no network), `backup` and `restore` (`pg_dump -Fc` / `pg_restore` inside `postit-postgres`, with refusal rails). They replace "backup notes" with a tool, and the docs still cover managed-database backups.
6. **Contract baseline:** the breaking-change check compares `api/openapi.json` with the newest `v*` tag. With no tag yet it passes with a notice.
7. **Docs location:** `docs/deployment.md` and `docs/configuration.md`, linked from the README. The README drops its stale "P9" placeholders.

## Global Constraints

- Quality gates after every Rust task, from `server/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`.
- Web gates (only if a web file changes), from `web/` in **PowerShell**: `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test`. pnpm only, never npm; the Git Bash pnpm shim is broken.
- Single-migration policy: P9 adds no migration.
- Compose conventions (README "Compose file layering"): project `name: postit`, every service `postit-`-prefixed with `container_name` equal to the service name, network `postit-net`, volume `postit-pgdata`, no secret value in any compose file. Operators never type the `-f` chain; `stack.sh`/`stack.ps1` build it. The CI smoke compose file is the one exception: it is its own project (`postit-smoke`) and holds only throwaway CI values.
- `stack.sh` and `stack.ps1` stay feature-identical. Every new command lands in both, with the help block at the top of each file updated.
- Vault: Claude edits extracted files only and never re-packs `!ref/vault.7z`. No step prints a vault value. Tests use throwaway fixture vaults.
- CI never needs real SMTP, certificates, an IdP, social credentials, or the vault.
- Docs are prose for humans: normal English, not caveman style. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Code style: match surrounding code (doc comments on public items, `# Errors` sections, `unwrap_or_else(|e| unreachable!(...))` in tests, no `unwrap()`/`expect()` outside tests).
- Pin every new CI action and container image to an exact version (`@v4`-style major tags for first-party GitHub actions, as `ci.yml` already does).

## Review Focus

- **Production restart after the first admin exists, with the bootstrap variable blank.** `POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL=""` (compose `${POSTIT_BOOTSTRAP_ADMIN_EMAIL:-}`) must load as "no rule" and start, not fail validation or match an empty email (Task 1, test `blank_bootstrap_email_is_no_rule`).
- **Rate limiting behind the host's reverse proxy.** Without `server.trusted_proxies`, every client shares the proxy's bucket and the audit IP is the Docker gateway (Task 2: fixed subnet plus gateway `/32` in qa and production, and the check in its Step 5).
- **`stack doctor` never prints a secret,** including a mismatch between `postgres.env` and `postit.env` passwords that contain `$` (Task 3, `doctor` test case 2).
- **A dump written into a tracked path, or restored over a running server.** `backup` refuses a path git does not ignore; `restore` refuses while `postit-api`, `postit-worker` or `postit-server` is running, refuses a file named for another environment, and needs `--force` on production (Task 4, Step 5 checks).
- **The contract job on a repository with no release tag.** It must pass with a notice, not fail or diff against nothing (Task 6, Step 3).

---

## File Structure

**Create**
- `server/crates/config/tests/common/mod.rs`: `shipped_config_dir()` and `production_vars()` shared by the integration tests.
- `server/crates/config/tests/production_requirements.rs`: the shipped `default.toml` + `production.toml` load only with every required value.
- `server/crates/config/tests/reference_doc.rs`: every settings key appears in `docs/configuration.md`.
- `docker/tests/stack-doctor.sh`: fixture-vault tests for `stack.sh doctor`.
- `docker/ci/docker-compose.smoke.yml`, `docker/ci/smoke.sh`, `docker/ci/oidc/mappings/discovery.json`, `docker/ci/oidc/mappings/jwks.json`, `docker/ci/oidc/__files/jwks.json`: the CI container smoke test.
- `.github/pull_request_template.md`: the migration review checklist.
- `docs/deployment.md`: the host-neutral operator guide.
- `docs/configuration.md`: the environment variable reference.

**Modify**
- `server/crates/config/src/settings.rs`: blank-as-unset bootstrap fields, and non-empty checks outside development.
- `docker/docker-compose.production.yml`, `docker/docker-compose.qa.yml`, `docker/.env.example`.
- `stack.sh`, `stack.ps1`: `doctor`, `backup`, `restore`, and the vault/backup dir overrides.
- `.gitignore`: `/!ref/backups`.
- `.github/workflows/ci.yml`: `web`, `contract`, `docker` jobs.
- `README.md`, `CLAUDE.md`, `!ref/plans/02. foundation.md`.

---

### Task 1: Startup requirements in `postit-config`

**Files:**
- Modify: `server/crates/config/src/settings.rs` (`BootstrapSettings` ~line 171, `Settings::validate` ~line 434)
- Create: `server/crates/config/tests/common/mod.rs`, `server/crates/config/tests/production_requirements.rs`

**Interfaces:**
- Consumes: `postit_config::load_with(env, dir, vars)`, `Environment`, `ConfigError` (its `Display` includes the validation message and figment's "missing field" text).
- Produces: `tests/common::shipped_config_dir() -> PathBuf` and `tests/common::production_vars() -> Vec<(String, String)>`, reused by Task 8. Validation behavior: outside development, a blank `database.password`, `mail.smtp.host` or `mail.from_address` is rejected. A blank `audit.pseudonym_key` is rejected in every environment. A blank `auth.bootstrap.admin_email` or `admin_subject` deserializes as `None`.

- [ ] **Step 1: Write the shared test helpers**

`server/crates/config/tests/common/mod.rs`:

```rust
//! Helpers shared by the integration tests that load the shipped `server/config/` files.

use std::path::PathBuf;

/// `server/config/`, the files baked into the image.
pub fn shipped_config_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config")
}

/// Every value a production deployment must supply on top of `production.toml`: the
/// vault's secrets plus the deployment-specific `POSTIT__…` settings that
/// `docker-compose.production.yml` sets. Removing any one of them must fail startup.
pub fn production_vars() -> Vec<(String, String)> {
    [
        ("POSTIT__DATABASE__USERNAME", "postgres"),
        ("POSTIT__DATABASE__PASSWORD", "db-password"),
        ("POSTIT__AUDIT__PSEUDONYM_KEY", "0123456789abcdef0123456789abcdef"),
        ("POSTIT__AUTH__OIDC__ISSUER", "https://auth.example.com"),
        ("POSTIT__AUTH__OIDC__AUDIENCES", "[\"postit\"]"),
        ("POSTIT__MAIL__FROM_ADDRESS", "postit <noreply@example.com>"),
        ("POSTIT__MAIL__SMTP__HOST", "smtp.example.com"),
        ("POSTIT__SERVER__PUBLIC_URL", "https://api.example.com"),
        ("POSTIT__SERVER__WEB__ROOT", "/srv/web"),
        ("POSTIT__APP__PUBLIC_URL", "https://app.example.com"),
        ("POSTIT__CORS__ALLOWED_ORIGINS", "[\"https://app.example.com\"]"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}
```

- [ ] **Step 2: Write the failing tests**

`server/crates/config/tests/production_requirements.rs`:

```rust
//! Plan 02 P9: production refuses to start without an OIDC issuer and audience, a
//! database password, SMTP settings, and `audit.pseudonym_key`. The bootstrap-admin
//! requirement needs the database and is covered by `postit-identity`'s
//! `bootstrap::check_startup` tests.

mod common;

use postit_config::{Environment, load_with};

use common::{production_vars, shipped_config_dir};

fn load(vars: Vec<(String, String)>) -> Result<postit_config::Settings, postit_config::ConfigError> {
    load_with(Environment::Production, &shipped_config_dir(), vars)
}

fn without(key: &str) -> Vec<(String, String)> {
    production_vars().into_iter().filter(|(k, _)| k != key).collect()
}

fn with(key: &str, value: &str) -> Vec<(String, String)> {
    let mut vars = without(key);
    vars.push((key.to_owned(), value.to_owned()));
    vars
}

fn assert_refused(vars: Vec<(String, String)>, mentions: &str) {
    let Err(err) = load(vars) else {
        unreachable!("production config unexpectedly loaded; expected an error about {mentions}");
    };
    assert!(err.to_string().contains(mentions), "{err}");
}

#[test]
fn complete_production_config_loads() {
    let settings = load(production_vars()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(settings.auth.oidc.issuer.as_str(), "https://auth.example.com/");
}

#[test]
fn missing_issuer_is_refused() {
    assert_refused(without("POSTIT__AUTH__OIDC__ISSUER"), "issuer");
}

#[test]
fn missing_audience_is_refused() {
    assert_refused(without("POSTIT__AUTH__OIDC__AUDIENCES"), "audiences");
}

#[test]
fn empty_audience_list_is_refused() {
    assert_refused(with("POSTIT__AUTH__OIDC__AUDIENCES", "[]"), "audiences");
}

#[test]
fn missing_database_password_is_refused() {
    assert_refused(without("POSTIT__DATABASE__PASSWORD"), "password");
}

#[test]
fn blank_database_password_is_refused() {
    assert_refused(with("POSTIT__DATABASE__PASSWORD", ""), "database.password");
}

#[test]
fn missing_smtp_host_is_refused() {
    assert_refused(without("POSTIT__MAIL__SMTP__HOST"), "host");
}

#[test]
fn blank_smtp_host_is_refused() {
    assert_refused(with("POSTIT__MAIL__SMTP__HOST", " "), "mail.smtp.host");
}

#[test]
fn missing_from_address_is_refused() {
    assert_refused(without("POSTIT__MAIL__FROM_ADDRESS"), "from_address");
}

#[test]
fn missing_pseudonym_key_is_refused() {
    assert_refused(without("POSTIT__AUDIT__PSEUDONYM_KEY"), "pseudonym_key");
}

#[test]
fn blank_pseudonym_key_is_refused() {
    assert_refused(with("POSTIT__AUDIT__PSEUDONYM_KEY", ""), "audit.pseudonym_key");
}

#[test]
fn blank_bootstrap_email_is_no_rule() {
    let settings = load(with("POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL", ""))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(settings.auth.bootstrap.admin_email, None);
}

#[test]
fn bootstrap_email_is_kept() {
    let settings = load(with("POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL", "admin@example.com"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        settings.auth.bootstrap.admin_email.as_deref(),
        Some("admin@example.com")
    );
}
```

- [ ] **Step 3: Run the tests and confirm which fail**

Run, from `server/`: `cargo test -p postit-config --test production_requirements`
Expected: `blank_database_password_is_refused`, `blank_smtp_host_is_refused`, `blank_pseudonym_key_is_refused` and `blank_bootstrap_email_is_no_rule` FAIL. The `missing_*` tests and `complete_production_config_loads` should already PASS. If `complete_production_config_loads` fails naming another missing key, that key is a real production requirement nobody supplies. Add it to `production_vars()` here and to the production compose file in Task 2, then re-run.

- [ ] **Step 4: Treat a blank bootstrap value as unset**

In `server/crates/config/src/settings.rs`, change `BootstrapSettings`:

```rust
/// First-admin bootstrap; at most one of the two fields may be set. A blank value counts
/// as unset, so a deployment can pass an empty variable (compose `${VAR:-}`) once its
/// first admin exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSettings {
    /// Email address whose first sign-in becomes the initial admin.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub admin_email: Option<String>,
    /// Subject (`sub`) whose first sign-in becomes the initial admin.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub admin_subject: Option<String>,
}

fn blank_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|s| !s.trim().is_empty()))
}
```

Add `use serde::Deserialize as _;` only if the `Deserialize` derive import does not already bring the trait into scope (it does when the file has `use serde::{Deserialize, Serialize};`).

- [ ] **Step 5: Add the non-empty checks to `validate`**

In `Settings::validate`, after the bootstrap mutual-exclusion check:

```rust
        if self.audit.pseudonym_key.expose().trim().is_empty() {
            return Err(ConfigError::Validation(
                "audit.pseudonym_key must not be empty".into(),
            ));
        }

        if env != Environment::Development {
            for (key, value) in [
                ("database.password", self.database.password.expose()),
                ("mail.smtp.host", self.mail.smtp.host.as_str()),
                ("mail.from_address", self.mail.from_address.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "{key} must not be empty outside development"
                    )));
                }
            }
        }
```

- [ ] **Step 6: Run the config tests**

Run: `cargo test -p postit-config`
Expected: PASS, including the existing `loader.rs` unit tests.

- [ ] **Step 7: Run the quality gates and commit**

Run the four gates from Global Constraints.

```bash
git add server/crates/config
git commit -m "feat(config): refuse blank production secrets; blank bootstrap is no rule"
```

---

### Task 2: Complete the qa and production compose files

**Files:**
- Modify: `docker/docker-compose.production.yml`, `docker/docker-compose.qa.yml`, `docker/.env.example`

**Interfaces:**
- Consumes: Task 1 (`blank_as_none`, so an empty `POSTIT_BOOTSTRAP_ADMIN_EMAIL` is safe).
- Produces: production needs these shell or `docker/.env` variables: `POSTIT_API_URL`, `POSTIT_APP_URL`, `POSTIT_OIDC_ISSUER`, `POSTIT_OIDC_AUDIENCE`, `POSTIT_SMTP_HOST`, `POSTIT_MAIL_FROM`. `POSTIT_BOOTSTRAP_ADMIN_EMAIL` and `POSTIT_TRUSTED_PROXIES` are optional. Task 3's `doctor` checks the same list.

- [ ] **Step 1: Show the current gap**

Run, from the repo root: `./stack.sh config production > /dev/null && echo RESOLVED`
Expected today: `RESOLVED`. Production resolves with postit.com defaults and no `mail.from_address` (production.toml has none), so the server would only fail at runtime. After this task the command must fail and name the first missing variable.

- [ ] **Step 2: Replace the production environment block**

In `docker/docker-compose.production.yml`, replace the `x-postit-environment` anchor and the comment above it:

```yaml
# Deployment-specific values have no default: compose refuses to resolve the file until
# each is set in the shell or in docker/.env (copy docker/.env.example), and the server
# refuses to start if one reaches it blank. The bootstrap admin is the exception, because
# it may be blank once an active admin exists; the server refuses a production start with
# no admin and no rule.
x-postit-environment: &postit-environment
    POSTIT_ENV: production
    # Plain HTTP inside the container; the reverse proxy owns TLS.
    POSTIT__SERVER__TLS__ENABLED: "false"
    POSTIT__SERVER__API_PORT: "8080"
    POSTIT__SERVER__WORKER_PORT: "8081"
    POSTIT__SERVER__PUBLIC_URL: "${POSTIT_API_URL:?set POSTIT_API_URL in docker/.env, e.g. https://api.postit.com}"
    POSTIT__APP__PUBLIC_URL: "${POSTIT_APP_URL:?set POSTIT_APP_URL in docker/.env, e.g. https://app.postit.com}"
    POSTIT__CORS__ALLOWED_ORIGINS: '["${POSTIT_APP_URL:?set POSTIT_APP_URL in docker/.env}"]'
    POSTIT__AUTH__OIDC__ISSUER: "${POSTIT_OIDC_ISSUER:?set POSTIT_OIDC_ISSUER in docker/.env, e.g. https://auth.postit.com}"
    POSTIT__AUTH__OIDC__AUDIENCES: '["${POSTIT_OIDC_AUDIENCE:?set POSTIT_OIDC_AUDIENCE in docker/.env (the IdP project or API audience)}"]'
    POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL: "${POSTIT_BOOTSTRAP_ADMIN_EMAIL:-}"
    POSTIT__MAIL__SMTP__HOST: "${POSTIT_SMTP_HOST:?set POSTIT_SMTP_HOST in docker/.env}"
    POSTIT__MAIL__FROM_ADDRESS: "${POSTIT_MAIL_FROM:?set POSTIT_MAIL_FROM in docker/.env, e.g. postit <noreply@postit.com>}"
    # The host's reverse proxy reaches the published loopback ports through Docker's
    # gateway on postit-net, so that is the one hop allowed to set X-Forwarded-For.
    # Without it every client shares one rate-limit bucket and one audit IP.
    POSTIT__SERVER__TRUSTED_PROXIES: "[${POSTIT_TRUSTED_PROXIES:-172.30.0.1/32}]"
```

At the end of the file add:

```yaml
networks:
    postit-net:
        ipam:
            config:
                # Fixed so the gateway in POSTIT__SERVER__TRUSTED_PROXIES is known. An
                # existing postit-net created without it must be recreated: stack down, up.
                - subnet: 172.30.0.0/24
```

Update the header comment's last paragraph to: "Every hostname and the mail sender are required variables (`docker/.env`, see `docker/.env.example`); there are no postit.com defaults to fall back to."

- [ ] **Step 3: Add the proxy trust to qa**

In `docker/docker-compose.qa.yml`, add to the `x-postit-environment` anchor:

```yaml
    # The host's reverse proxy reaches the published loopback ports through Docker's
    # gateway on postit-net (see docker-compose.production.yml).
    POSTIT__SERVER__TRUSTED_PROXIES: "[${POSTIT_TRUSTED_PROXIES:-172.30.0.1/32}]"
```

and the same `networks:` block as production at the end of the file. The qa hostnames stay in `server/config/qa.toml`.

- [ ] **Step 4: Update `docker/.env.example`**

Replace the production section with:

```sh
# production: REQUIRED. docker-compose.production.yml refuses to resolve without these.
# POSTIT_API_URL=https://api.postit.com
# POSTIT_APP_URL=https://app.postit.com
# POSTIT_OIDC_ISSUER=https://auth.postit.com
# POSTIT_OIDC_AUDIENCE=postit
# POSTIT_SMTP_HOST=smtp.postit.com
# POSTIT_MAIL_FROM=postit <noreply@postit.com>

# production: optional. The first-boot admin; may be left unset once an admin exists.
# POSTIT_BOOTSTRAP_ADMIN_EMAIL=admin@postit.com

# qa / production: the one proxy hop allowed to set X-Forwarded-For. Defaults to
# postit-net's gateway (172.30.0.1/32), which is how a reverse proxy on this host arrives.
# POSTIT_TRUSTED_PROXIES=172.30.0.1/32
```

- [ ] **Step 5: Verify both files resolve, and production refuses**

Run, from the repo root, in Git Bash:

```bash
./stack.sh config qa > /dev/null && echo "qa resolves"
env -u POSTIT_OIDC_ISSUER ./stack.sh config production 2>&1 | grep -E "POSTIT_[A-Z_]+" | head -1
POSTIT_API_URL=https://api.example.com POSTIT_APP_URL=https://app.example.com \
POSTIT_OIDC_ISSUER=https://auth.example.com POSTIT_OIDC_AUDIENCE=postit \
POSTIT_SMTP_HOST=smtp.example.com "POSTIT_MAIL_FROM=postit <noreply@example.com>" \
  ./stack.sh config production | grep -E "TRUSTED_PROXIES|FROM_ADDRESS|ADMIN_EMAIL"
```

Expected: `qa resolves`. The second command prints an error naming a required variable (`POSTIT_API_URL` is the first one compose meets). The third prints `POSTIT__SERVER__TRUSTED_PROXIES: '[172.30.0.1/32]'`, the `FROM_ADDRESS`, and an empty `ADMIN_EMAIL`. This needs a `docker/.env` without those variables; if one exists, move it aside for the check.

Then confirm the gateway assumption on a running qa stack (needs the qa vault extracted): `./stack.sh up qa`, then `docker network inspect postit_postit-net --format '{{(index .IPAM.Config 0).Gateway}}'`.
Expected: `172.30.0.1`. Then `./stack.sh down qa`. If the vault is not available, note in the task report that this check was skipped.

- [ ] **Step 6: Commit**

```bash
git add docker/docker-compose.production.yml docker/docker-compose.qa.yml docker/.env.example
git commit -m "feat(docker): production requires its deployment values; trust the proxy hop"
```

---

### Task 3: `stack doctor`

**Files:**
- Modify: `stack.sh`, `stack.ps1`
- Create: `docker/tests/stack-doctor.sh`

**Interfaces:**
- Consumes: `vault_files()` / `$vaultFiles`, `$environment`, `die` / `Stop-Stack`.
- Produces: the command `doctor [environment]`, exit 0 when complete and 1 with one line per problem, never printing a value. Two overrides for tests and odd layouts: `POSTIT_VAULT_DIR` (default `!ref/vault`) and `POSTIT_DOCKER_ENV_FILE` (default `docker/.env`).

What `doctor` checks, for the chosen environment:
1. Every vault file of the environment exists.
2. Each required key is present and non-empty: `postgres.env`: `POSTGRES_USER`, `POSTGRES_PASSWORD`. `postit.env`: `POSTIT__DATABASE__USERNAME`, `POSTIT__DATABASE__PASSWORD`, `POSTIT__AUDIT__PSEUDONYM_KEY`. Development `zitadel.env`: `ZITADEL_MASTERKEY`.
3. `POSTIT__AUDIT__PSEUDONYM_KEY` is at least 32 characters.
4. `POSTGRES_USER` equals `POSTIT__DATABASE__USERNAME`, and `POSTGRES_PASSWORD` equals `POSTIT__DATABASE__PASSWORD` (README: "They have to agree, and nothing checks it").
5. `POSTIT__MAIL__SMTP__USERNAME` and `POSTIT__MAIL__SMTP__PASSWORD` are both set or both unset.
6. Production only: `POSTIT_API_URL`, `POSTIT_APP_URL`, `POSTIT_OIDC_ISSUER`, `POSTIT_OIDC_AUDIENCE`, `POSTIT_SMTP_HOST`, `POSTIT_MAIL_FROM` are set in the shell or in `docker/.env`.

- [ ] **Step 1: Write the failing test script**

`docker/tests/stack-doctor.sh`:

```bash
#!/usr/bin/env bash
# Tests `stack.sh doctor` against throwaway vault folders. Needs bash only: no Docker, no
# network, no real vault. Run from anywhere: docker/tests/stack-doctor.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0

# A complete vault for $1. The postgres password contains `$`, single-quoted as the README
# prescribes, so a mismatch report that leaked it would be caught below.
write_vault() {
    local env="$1" db_password="${2:-pg\$secret-value}" key="${3:-0123456789abcdef0123456789abcdef}"
    mkdir -p "$tmp/vault/$env"
    printf "POSTGRES_USER=postgres\nPOSTGRES_PASSWORD='pg\$secret-value'\n" >"$tmp/vault/$env/postgres.env"
    printf "POSTIT__DATABASE__USERNAME=postgres\nPOSTIT__DATABASE__PASSWORD='%s'\nPOSTIT__AUDIT__PSEUDONYM_KEY='%s'\n" \
        "$db_password" "$key" >"$tmp/vault/$env/postit.env"
}

write_docker_env() {
    cat >"$tmp/docker.env" <<'EOF'
POSTIT_API_URL=https://api.example.com
POSTIT_APP_URL=https://app.example.com
POSTIT_OIDC_ISSUER=https://auth.example.com
POSTIT_OIDC_AUDIENCE=postit
POSTIT_SMTP_HOST=smtp.example.com
POSTIT_MAIL_FROM='postit <noreply@example.com>'
EOF
}

# expect <name> <exit code> <text the output must contain> -- <doctor args...>
expect() {
    local name="$1" want_code="$2" want_text="$3"
    shift 4
    local out code=0
    out="$(env -u POSTIT_API_URL -u POSTIT_APP_URL -u POSTIT_OIDC_ISSUER -u POSTIT_OIDC_AUDIENCE \
        -u POSTIT_SMTP_HOST -u POSTIT_MAIL_FROM \
        POSTIT_VAULT_DIR="$tmp/vault" POSTIT_DOCKER_ENV_FILE="$tmp/docker.env" \
        "$root/stack.sh" doctor "$@" 2>&1)" || code=$?
    if [ "$code" != "$want_code" ] || [[ "$out" != *"$want_text"* ]] || [[ "$out" == *'pg$secret'* ]]; then
        echo "FAIL $name: exit $code (want $want_code), output:" >&2
        echo "$out" | sed 's/^/    /' >&2
        failures=$((failures + 1))
    else
        echo "ok   $name"
    fi
}

write_vault qa
expect "complete qa vault" 0 "qa looks complete" -- qa

rm -rf "$tmp/vault"; write_vault qa 'other$password'
expect "password mismatch is named, not printed" 1 "POSTIT__DATABASE__PASSWORD" -- qa

rm -rf "$tmp/vault"; write_vault qa; rm "$tmp/vault/qa/postit.env"
expect "missing vault file" 1 "!ref/vault/qa/postit.env" -- qa

rm -rf "$tmp/vault"; write_vault qa 'pg$secret-value' short
expect "short pseudonym key" 1 "POSTIT__AUDIT__PSEUDONYM_KEY" -- qa

rm -rf "$tmp/vault"; write_vault production; rm -f "$tmp/docker.env"
expect "production without deployment values" 1 "POSTIT_OIDC_ISSUER" -- production

write_docker_env
expect "complete production" 0 "production looks complete" -- production

rm -rf "$tmp/vault"; write_vault qa
printf "POSTIT__MAIL__SMTP__USERNAME=relay\n" >>"$tmp/vault/qa/postit.env"
expect "half an SMTP credential" 1 "POSTIT__MAIL__SMTP__PASSWORD" -- qa

[ "$failures" = 0 ] || { echo "$failures doctor test(s) failed" >&2; exit 1; }
echo "all doctor tests passed"
```

- [ ] **Step 2: Run it and watch it fail**

Run, in Git Bash from the repo root: `bash docker/tests/stack-doctor.sh`
Expected: every case FAILs with `stack: unknown command: doctor`.

- [ ] **Step 3: Implement `doctor` in `stack.sh`**

Replace the `vault_dir=` line with:

```bash
# Overridable for tests (docker/tests/stack-doctor.sh) and unusual layouts.
vault_dir="${POSTIT_VAULT_DIR:-$script_dir/!ref/vault}"
docker_env_file="${POSTIT_DOCKER_ENV_FILE:-$docker_dir/.env}"
```

Add to the help block's command list, after `config`:

```text
#   doctor      Check the environment's vault files and required values.  No Docker, no
#               network, and no value is ever printed
```

Add these functions after `preflight()`:

```bash
# KEY's value in a dotenv file, read as data (a vault file is never sourced). Surrounding
# single or double quotes are stripped, matching postit-config and Compose.
env_value() {
    local file="$1" key="$2" line value
    [ -f "$file" ] || return 0
    line="$(grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 || true)"
    [ -n "$line" ] || return 0
    value="${line#*=}"
    case "$value" in
        \'*\') value="${value:1:${#value}-2}" ;;
        \"*\") value="${value:1:${#value}-2}" ;;
    esac
    printf '%s' "$value"
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
production_compose_vars="POSTIT_API_URL POSTIT_APP_URL POSTIT_OIDC_ISSUER POSTIT_OIDC_AUDIENCE POSTIT_SMTP_HOST POSTIT_MAIL_FROM"

doctor() {
    local problems=() f key dir="$vault_dir/$environment"
    for f in $(vault_files); do
        [ -f "$dir/$f" ] || problems+=("missing !ref/vault/$environment/$f")
    done
    if [ ${#problems[@]} -eq 0 ]; then
        local pg="$dir/postgres.env" app="$dir/postit.env"
        for key in POSTGRES_USER POSTGRES_PASSWORD; do
            [ -n "$(env_value "$pg" "$key")" ] || problems+=("postgres.env: $key is missing or empty")
        done
        for key in POSTIT__DATABASE__USERNAME POSTIT__DATABASE__PASSWORD POSTIT__AUDIT__PSEUDONYM_KEY; do
            [ -n "$(env_value "$app" "$key")" ] || problems+=("postit.env: $key is missing or empty")
        done
        if [ "$environment" = "development" ] && [ -z "$(env_value "$dir/zitadel.env" ZITADEL_MASTERKEY)" ]; then
            problems+=("zitadel.env: ZITADEL_MASTERKEY is missing or empty")
        fi
        local pkey
        pkey="$(env_value "$app" POSTIT__AUDIT__PSEUDONYM_KEY)"
        if [ -n "$pkey" ] && [ ${#pkey} -lt 32 ]; then
            problems+=("postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters")
        fi
        [ "$(env_value "$pg" POSTGRES_USER)" = "$(env_value "$app" POSTIT__DATABASE__USERNAME)" ] ||
            problems+=("postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env")
        [ "$(env_value "$pg" POSTGRES_PASSWORD)" = "$(env_value "$app" POSTIT__DATABASE__PASSWORD)" ] ||
            problems+=("postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env")
        local smtp_user smtp_pass
        smtp_user="$(env_value "$app" POSTIT__MAIL__SMTP__USERNAME)"
        smtp_pass="$(env_value "$app" POSTIT__MAIL__SMTP__PASSWORD)"
        if [ -n "$smtp_user" ] && [ -z "$smtp_pass" ]; then
            problems+=("postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not")
        elif [ -z "$smtp_user" ] && [ -n "$smtp_pass" ]; then
            problems+=("postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not")
        fi
    fi
    if [ "$environment" = "production" ]; then
        for key in $production_compose_vars; do
            [ -n "${!key:-}" ] || [ -n "$(env_value "$docker_env_file" "$key")" ] ||
                problems+=("$key is not set in the shell or docker/.env (see docker/.env.example)")
        done
    fi
    if [ ${#problems[@]} -gt 0 ]; then
        echo "stack: $environment is not ready:" >&2
        printf '  %s\n' "${problems[@]}" >&2
        exit 1
    fi
    echo "stack: $environment looks complete"
}
```

Add to the `case "$command"` dispatch, before `help|-h|--help)`:

```bash
    doctor)
        doctor
        ;;
```

- [ ] **Step 4: Run the tests**

Run: `bash docker/tests/stack-doctor.sh`
Expected: `all doctor tests passed`.

- [ ] **Step 5: Mirror `doctor` in `stack.ps1`**

Replace `$vaultDir = ...` with:

```powershell
# Overridable for tests (docker/tests/stack-doctor.sh) and unusual layouts.
$vaultDir = if ($env:POSTIT_VAULT_DIR) { $env:POSTIT_VAULT_DIR } else { Join-Path $PSScriptRoot "!ref/vault" }
$dockerEnvFile = if ($env:POSTIT_DOCKER_ENV_FILE) { $env:POSTIT_DOCKER_ENV_FILE } else { Join-Path $dockerDir ".env" }
```

Add the same help line as Step 3. Add after `Test-Preflight`:

```powershell
# KEY's value in a dotenv file, read as data (a vault file is never dot-sourced).
# Surrounding single or double quotes are stripped, matching postit-config and Compose.
function Get-EnvValue([string]$File, [string]$Key) {
    if (-not (Test-Path $File)) { return "" }
    $line = Get-Content $File | Where-Object { $_ -match "^\s*$([regex]::Escape($Key))=" } | Select-Object -Last 1
    if ($null -eq $line) { return "" }
    $value = $line.Substring($line.IndexOf('=') + 1)
    if ($value.Length -ge 2 -and (($value[0] -eq "'" -and $value[-1] -eq "'") -or ($value[0] -eq '"' -and $value[-1] -eq '"'))) {
        $value = $value.Substring(1, $value.Length - 2)
    }
    return $value
}

# The variables docker-compose.production.yml requires (${VAR:?...}).
$productionComposeVars = @("POSTIT_API_URL", "POSTIT_APP_URL", "POSTIT_OIDC_ISSUER", "POSTIT_OIDC_AUDIENCE", "POSTIT_SMTP_HOST", "POSTIT_MAIL_FROM")

function Invoke-Doctor {
    $problems = [System.Collections.Generic.List[string]]::new()
    $dir = Join-Path $vaultDir $environment
    foreach ($f in $vaultFiles) {
        if (-not (Test-Path (Join-Path $dir $f))) { $problems.Add("missing !ref/vault/$environment/$f") }
    }
    if ($problems.Count -eq 0) {
        $pg = Join-Path $dir "postgres.env"
        $app = Join-Path $dir "postit.env"
        foreach ($key in @("POSTGRES_USER", "POSTGRES_PASSWORD")) {
            if (-not (Get-EnvValue $pg $key)) { $problems.Add("postgres.env: $key is missing or empty") }
        }
        foreach ($key in @("POSTIT__DATABASE__USERNAME", "POSTIT__DATABASE__PASSWORD", "POSTIT__AUDIT__PSEUDONYM_KEY")) {
            if (-not (Get-EnvValue $app $key)) { $problems.Add("postit.env: $key is missing or empty") }
        }
        if ($environment -eq "development" -and -not (Get-EnvValue (Join-Path $dir "zitadel.env") "ZITADEL_MASTERKEY")) {
            $problems.Add("zitadel.env: ZITADEL_MASTERKEY is missing or empty")
        }
        $pkey = Get-EnvValue $app "POSTIT__AUDIT__PSEUDONYM_KEY"
        if ($pkey -and $pkey.Length -lt 32) {
            $problems.Add("postit.env: POSTIT__AUDIT__PSEUDONYM_KEY is shorter than 32 characters")
        }
        if ((Get-EnvValue $pg "POSTGRES_USER") -cne (Get-EnvValue $app "POSTIT__DATABASE__USERNAME")) {
            $problems.Add("postit.env: POSTIT__DATABASE__USERNAME differs from POSTGRES_USER in postgres.env")
        }
        if ((Get-EnvValue $pg "POSTGRES_PASSWORD") -cne (Get-EnvValue $app "POSTIT__DATABASE__PASSWORD")) {
            $problems.Add("postit.env: POSTIT__DATABASE__PASSWORD differs from POSTGRES_PASSWORD in postgres.env")
        }
        $smtpUser = Get-EnvValue $app "POSTIT__MAIL__SMTP__USERNAME"
        $smtpPass = Get-EnvValue $app "POSTIT__MAIL__SMTP__PASSWORD"
        if ($smtpUser -and -not $smtpPass) {
            $problems.Add("postit.env: POSTIT__MAIL__SMTP__USERNAME is set but POSTIT__MAIL__SMTP__PASSWORD is not")
        } elseif ($smtpPass -and -not $smtpUser) {
            $problems.Add("postit.env: POSTIT__MAIL__SMTP__PASSWORD is set but POSTIT__MAIL__SMTP__USERNAME is not")
        }
    }
    if ($environment -eq "production") {
        foreach ($key in $productionComposeVars) {
            if (-not [Environment]::GetEnvironmentVariable($key) -and -not (Get-EnvValue $dockerEnvFile $key)) {
                $problems.Add("$key is not set in the shell or docker/.env (see docker/.env.example)")
            }
        }
    }
    if ($problems.Count -gt 0) {
        [Console]::Error.WriteLine("stack: $environment is not ready:")
        $problems | ForEach-Object { [Console]::Error.WriteLine("  $_") }
        exit 1
    }
    Write-Host "stack: $environment looks complete"
}
```

Add `"doctor" { Invoke-Doctor }` to the `switch`, before the help case.

- [ ] **Step 6: Check the PowerShell half by hand against the same fixtures**

In PowerShell from the repo root, build a complete qa fixture and a mismatching one:

```powershell
$t = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid()); New-Item -ItemType Directory -Force "$t/qa" | Out-Null
Set-Content "$t/qa/postgres.env" "POSTGRES_USER=postgres`nPOSTGRES_PASSWORD='pg`$secret-value'"
Set-Content "$t/qa/postit.env" "POSTIT__DATABASE__USERNAME=postgres`nPOSTIT__DATABASE__PASSWORD='pg`$secret-value'`nPOSTIT__AUDIT__PSEUDONYM_KEY='0123456789abcdef0123456789abcdef'"
$env:POSTIT_VAULT_DIR = $t; ./stack.ps1 doctor qa; "exit $LASTEXITCODE"
Set-Content "$t/qa/postit.env" "POSTIT__DATABASE__USERNAME=postgres`nPOSTIT__DATABASE__PASSWORD='other'`nPOSTIT__AUDIT__PSEUDONYM_KEY='0123456789abcdef0123456789abcdef'"
./stack.ps1 doctor qa; "exit $LASTEXITCODE"
Remove-Item Env:POSTIT_VAULT_DIR; Remove-Item -Recurse -Force $t
```

Expected: `stack: qa looks complete` / `exit 0`, then the `POSTIT__DATABASE__PASSWORD differs` line / `exit 1`, with no `pg$secret` in either output. Then run `./stack.ps1 doctor` against the real development vault (if extracted) and expect `development looks complete`.

- [ ] **Step 7: Commit**

```bash
git add stack.sh stack.ps1 docker/tests/stack-doctor.sh
git commit -m "feat(stack): doctor checks vault files and required values offline"
```

---

### Task 4: `stack backup` and `stack restore`

**Files:**
- Modify: `stack.sh`, `stack.ps1`, `.gitignore`

**Interfaces:**
- Consumes: `guard_destroy` / `Test-DestroyGuard` (production needs `--force` / `-Force`), `$extra` (the dump file path for `restore`).
- Produces: `backup [environment]` writes `<backup dir>/postit-<environment>-<UTC yyyymmddThhmmssZ>.dump` (custom format). `restore [environment] <file>` runs `pg_restore --clean --if-exists --no-owner`. The backup dir defaults to `!ref/backups` and is overridable with `POSTIT_BACKUP_DIR`.

Rails, each one a way to lose data otherwise:
- `backup` refuses a destination `git check-ignore` does not claim. A dump holds every user's data unencrypted.
- The dump moves with `docker cp`, never a shell redirect, so PowerShell's text encoding cannot corrupt it.
- `restore` refuses while `postit-api`, `postit-worker` or `postit-server` runs. A live server would keep writing to a half-restored schema.
- `restore` refuses a file whose name is not `postit-<environment>-*.dump`. One host runs one environment, and a file name is how production data reaches a qa host.
- `restore` on production needs `--force`. It does not start anything afterwards; `stack up` is the operator's next step.

- [ ] **Step 1: Ignore the default backup directory**

Append to `.gitignore`, after the vault entry:

```gitignore
# Database dumps from `stack backup` (unencrypted user data; delete when done)
/!ref/backups
```

- [ ] **Step 2: Implement in `stack.sh`**

Help block additions, after `psql`:

```text
#   backup      Dump the postit database to !ref/backups/postit-<environment>-<UTC>.dump
#               (POSTIT_BACKUP_DIR overrides; it must be git-ignored)
#   restore     Restore a dump: ./stack.sh restore [environment] <file>.  Refuses while the
#               server runs; production needs --force.  Does not start anything afterwards
```

After `docker_env_file=`:

```bash
backup_dir="${POSTIT_BACKUP_DIR:-$script_dir/!ref/backups}"
```

Functions, after `doctor()`:

```bash
backup() {
    local stamp file
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    mkdir -p "$backup_dir"
    file="$backup_dir/postit-$environment-$stamp.dump"
    git -C "$script_dir" check-ignore -q "$file" ||
        die "refusing to write a dump to $file: git does not ignore that path, and a dump is unencrypted user data"
    # Written inside the container and copied out: binary-safe on every host shell.
    docker exec postit-postgres sh -c 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump'
    docker cp postit-postgres:/tmp/postit.dump "$file"
    docker exec postit-postgres rm -f /tmp/postit.dump
    echo "stack: wrote $file"
    echo "stack: it holds every user's data unencrypted; delete it when you are done with it"
}

restore() {
    local file="${extra[0]:-}" running
    [ -n "$file" ] || die "usage: ./stack.sh restore [environment] <dump file>"
    [ -f "$file" ] || die "no such file: $file"
    case "$(basename "$file")" in
        "postit-$environment-"*.dump) ;;
        *) die "refusing: $(basename "$file") is not a postit-$environment-*.dump file" ;;
    esac
    guard_destroy
    running="$(docker ps --format '{{.Names}}' | grep -E '^postit-(api|worker|server)$' || true)"
    [ -z "$running" ] ||
        die "stop the server first (docker stop $(echo $running)): it would keep writing to a half-restored database"
    docker cp "$file" postit-postgres:/tmp/postit.dump
    if ! docker exec postit-postgres sh -c \
        'pg_restore --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump'; then
        docker exec postit-postgres rm -f /tmp/postit.dump
        die "pg_restore failed; the database may be partly restored. Do not start the server on it"
    fi
    docker exec postit-postgres rm -f /tmp/postit.dump
    echo "stack: restored $file; start the server with ./stack.sh up $environment"
}
```

`guard_destroy`'s message says "destroy the production database", which is what a `--clean` restore does.

Dispatch, before `help`:

```bash
    backup)
        backup
        ;;
    restore)
        restore
        ;;
```

- [ ] **Step 3: Implement in `stack.ps1`**

Same help lines. After `$dockerEnvFile`:

```powershell
$backupDir = if ($env:POSTIT_BACKUP_DIR) { $env:POSTIT_BACKUP_DIR } else { Join-Path $PSScriptRoot "!ref/backups" }
```

Functions, after `Invoke-Doctor`:

```powershell
function Invoke-Backup {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    New-Item -ItemType Directory -Force $backupDir | Out-Null
    $file = Join-Path $backupDir "postit-$environment-$stamp.dump"
    & git -C $PSScriptRoot check-ignore -q $file
    if ($LASTEXITCODE -ne 0) {
        Stop-Stack "refusing to write a dump to ${file}: git does not ignore that path, and a dump is unencrypted user data"
    }
    # Written inside the container and copied out with docker cp: PowerShell's own
    # redirection would re-encode the binary dump as text.
    Invoke-Docker @("exec", "postit-postgres", "sh", "-c", 'pg_dump -Fc -U "$POSTGRES_USER" -d "$POSTGRES_DB" -f /tmp/postit.dump')
    Invoke-Docker @("cp", "postit-postgres:/tmp/postit.dump", $file)
    Invoke-Docker @("exec", "postit-postgres", "rm", "-f", "/tmp/postit.dump")
    Write-Host "stack: wrote $file"
    Write-Host "stack: it holds every user's data unencrypted; delete it when you are done with it"
}

function Invoke-Restore {
    $file = if ($extra.Count -gt 0) { $extra[0] } else { "" }
    if (-not $file) { Stop-Stack "usage: ./stack.ps1 restore [environment] <dump file>" }
    if (-not (Test-Path $file -PathType Leaf)) { Stop-Stack "no such file: $file" }
    $name = Split-Path $file -Leaf
    if ($name -notlike "postit-$environment-*.dump") {
        Stop-Stack "refusing: $name is not a postit-$environment-*.dump file"
    }
    Test-DestroyGuard
    $running = @(& docker ps --format '{{.Names}}' | Where-Object { $_ -match '^postit-(api|worker|server)$' })
    if ($running.Count -gt 0) {
        Stop-Stack "stop the server first (docker stop $($running -join ' ')): it would keep writing to a half-restored database"
    }
    Invoke-Docker @("cp", $file, "postit-postgres:/tmp/postit.dump")
    & docker exec postit-postgres sh -c 'pg_restore --clean --if-exists --no-owner -U "$POSTGRES_USER" -d "$POSTGRES_DB" /tmp/postit.dump'
    $status = $LASTEXITCODE
    & docker exec postit-postgres rm -f /tmp/postit.dump | Out-Null
    if ($status -ne 0) {
        Stop-Stack "pg_restore failed; the database may be partly restored. Do not start the server on it"
    }
    Write-Host "stack: restored $file; start the server with ./stack.ps1 up $environment"
}
```

Dispatch: `"backup" { Invoke-Backup }` and `"restore" { Invoke-Restore }`.

- [ ] **Step 4: Round trip in development (bash)**

Needs the dev stack (`./stack.sh up`) with at least one user signed in once, so `users` has rows. From the repo root:

```bash
./stack.sh backup
docker exec postit-postgres sh -c 'psql -U "$POSTGRES_USER" -d postit -Atc "select count(*) from users"'
docker exec postit-postgres sh -c 'psql -U "$POSTGRES_USER" -d postit -c "delete from audit_events"'
./stack.sh restore development '!ref/backups/<the file backup printed>'
docker exec postit-postgres sh -c 'psql -U "$POSTGRES_USER" -d postit -Atc "select count(*) from users; select count(*) > 0 from audit_events"'
```

Expected: `backup` prints the file under `!ref/backups/`. `git status --short` does not list it. After `restore`, the user count matches the first count and the audit check prints `t`. Stop any `cargo run` first, or `restore` correctly refuses only if it runs as a container; a natively run server is invisible to `docker ps`, so stop it by hand. Mention this in the docs (Task 9).

- [ ] **Step 5: Check the rails (bash)**

```bash
POSTIT_BACKUP_DIR="$PWD/docs" ./stack.sh backup; echo "exit $?"
./stack.sh restore qa '!ref/backups/<the development file>'; echo "exit $?"
cp '!ref/backups/<the development file>' '!ref/backups/postit-production-x.dump'
./stack.sh restore production '!ref/backups/postit-production-x.dump'; echo "exit $?"
rm '!ref/backups/postit-production-x.dump'
./stack.sh up --app && ./stack.sh restore development '!ref/backups/<the development file>'; echo "exit $?"
```

Expected, in order: refusal "git does not ignore that path" / `exit 1`; refusal "is not a postit-qa-*.dump file" / `exit 1`; refusal "without --force" / `exit 1`; refusal "stop the server first (docker stop postit-server)" / `exit 1`. Then `./stack.sh down` (no `-v`).

- [ ] **Step 6: Same round trip and one rail in PowerShell**

```powershell
./stack.ps1 backup
./stack.ps1 restore development '!ref/backups/<the new file>'
./stack.ps1 restore qa '!ref/backups/<the new file>'; "exit $LASTEXITCODE"
```

Expected: the backup and restore succeed and the file is byte-identical to a bash backup in format (`pg_restore -l` inside the container lists its contents). The third command refuses with `exit 1`. Delete the dumps afterwards.

- [ ] **Step 7: Commit**

```bash
git add stack.sh stack.ps1 .gitignore
git commit -m "feat(stack): backup and restore the postit database with refusal rails"
```

---

### Task 5: CI `web` job

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `web/package.json` scripts `typecheck`, `lint`, `format:check`, `test`, `build`. `packageManager: pnpm@11.10.0`, `web/.nvmrc` (24).
- Produces: job `web`.

- [ ] **Step 1: Add the job**

Append under `jobs:`:

```yaml
  web:
    name: web
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      # Reads the exact pnpm version from package.json's packageManager field.
      - uses: pnpm/action-setup@v4
        with:
          package_json_file: web/package.json
      - uses: actions/setup-node@v4
        with:
          node-version-file: web/.nvmrc
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - name: install
        run: pnpm install --frozen-lockfile
      - name: typecheck
        run: pnpm typecheck
      - name: lint
        run: pnpm lint
      - name: format
        run: pnpm format:check
      - name: test
        run: pnpm test
      - name: build
        run: pnpm build
```

- [ ] **Step 2: Lint the workflow file locally**

Run, from the repo root: `docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:1.7.7 -color`
Expected: no output, exit 0. If that image tag does not exist, use the newest tag listed at https://hub.docker.com/r/rhysd/actionlint/tags and use the same one in Tasks 6 and 7.

- [ ] **Step 3: Run the same commands locally**

In PowerShell from `web/`: `pnpm install --frozen-lockfile; pnpm typecheck; pnpm lint; pnpm format:check; pnpm test; pnpm build`
Expected: all succeed. A failure here is a real web defect to fix in this task, not something to skip in CI.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add the web job"
```

---

### Task 6: CI `contract` job

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `cargo xtask openapi --check` (compares `api/openapi.json` and `web/src/api/schema.d.ts` with what the code generates; needs Node and `web/node_modules`). Tags named `v*`.
- Produces: job `contract`. With no `v*` tag it passes with a `::notice::`. With a tag, it fails on any oasdiff `ERR`-level breaking change between that tag's `api/openapi.json` and the working tree's.

- [ ] **Step 1: Pin oasdiff**

Look up the newest release at https://github.com/oasdiff/oasdiff/releases and its matching image tag at https://hub.docker.com/r/tufin/oasdiff/tags (same version, with the `v`, e.g. `v1.11.7`). Use that exact value for `OASDIFF_VERSION` below. Do not use `latest`.

- [ ] **Step 2: Add the job**

```yaml
  contract:
    name: contract
    runs-on: ubuntu-latest
    env:
      SQLX_OFFLINE: "true"
      OASDIFF_VERSION: v1.11.7 # the exact tag chosen in Step 1
    steps:
      - uses: actions/checkout@v4
        with:
          # Tags, so the breaking-change check can find the previous release.
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: server
      - uses: pnpm/action-setup@v4
        with:
          package_json_file: web/package.json
      - uses: actions/setup-node@v4
        with:
          node-version-file: web/.nvmrc
          cache: pnpm
          cache-dependency-path: web/pnpm-lock.yaml
      - name: install web dependencies
        # cargo xtask openapi runs web/'s pinned openapi-typescript.
        working-directory: web
        run: pnpm install --frozen-lockfile
      - name: generated contract is current
        working-directory: server
        run: cargo xtask openapi --check
      - name: no breaking change against the previous release
        run: |
          base="$(git tag --list 'v*' --sort=-v:refname | head -n 1)"
          if [ -z "$base" ]; then
            echo "::notice::no v* release tag yet; breaking-change check skipped"
            exit 0
          fi
          if ! git show "$base:api/openapi.json" > "$RUNNER_TEMP/base.json" 2>/dev/null; then
            echo "::notice::$base has no api/openapi.json; breaking-change check skipped"
            exit 0
          fi
          echo "comparing api/openapi.json with $base"
          docker run --rm \
            -v "$RUNNER_TEMP:/base:ro" -v "$GITHUB_WORKSPACE/api:/head:ro" \
            "tufin/oasdiff:$OASDIFF_VERSION" \
            breaking /base/base.json /head/openapi.json --fail-on ERR
```

- [ ] **Step 3: Exercise the no-tag path and the breaking path locally**

From the repo root in Git Bash:

```bash
git tag --list 'v*'   # expect nothing
cp api/openapi.json "$TMP/base.json"
docker run --rm -v "$TMP:/base:ro" -v "$PWD/api:/head:ro" tufin/oasdiff:<version> breaking /base/base.json /head/openapi.json --fail-on ERR; echo "exit $?"
```

Expected: no tags, and comparing the spec with itself prints nothing and `exit 0`. Then remove one path (for example `/api/v1/me`) from a copy, `$TMP/head/openapi.json`, compare the original as base with the copy as head, and expect a `removed the path` error and `exit 1`. This proves `--fail-on ERR` catches a removed operation. Delete the temp files.

- [ ] **Step 4: Lint and commit**

Run the actionlint command from Task 5, Step 2. Expected: clean.

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add the contract job with an oasdiff check against the last release"
```

---

### Task 7: CI `docker` job and container smoke test

**Files:**
- Create: `docker/ci/docker-compose.smoke.yml`, `docker/ci/smoke.sh`, `docker/ci/oidc/mappings/discovery.json`, `docker/ci/oidc/mappings/jwks.json`, `docker/ci/oidc/__files/jwks.json`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `docker/server.Dockerfile` (image with `/srv/web`, `HEALTHCHECK`, ports 8080/8081/8082). The server accepts an `http` issuer only in development (plan 02 "Testing and CI"). `docker/tests/stack-doctor.sh` from Task 3.
- Produces: job `docker`. It builds `postit-server:ci` (no push), runs `postit-api` (role `api`, web on 8082) and `postit-worker` (role `worker`) as separate containers, and requires 200 from API `/health` and `/ready`, worker `/health` and `/ready`, and web `/config.json`.

- [ ] **Step 1: Commit the OIDC fixtures**

`docker/ci/oidc/__files/jwks.json` is a public key only. Its private half was discarded when it was generated, so nothing can sign with it. The smoke test needs a JWKS to load, not a valid token:

```json
{
  "keys": [
    {
      "kty": "RSA",
      "use": "sig",
      "alg": "RS256",
      "kid": "ci-smoke",
      "n": "h_GujP6zcYBbOoFwyCZyv5hvBiqflLWnd1oDBvuKryLnVRghIzAGz-J-TDZsN38wyFlMoJZzaZOwLZtQaX_Wzn4UERqr6hG0oYUVspertNIky3mH01ckQ3nKGj8eC8JDVol9lf-LftFN45RwLfSOZ28btAOGvxyQn05SoYkR0QdUOZagb1x9OjHw3mW1MgArhJ4iGRdVFw_AVr8q3tsYpP1yafwzJW8j6bfoi-Gptoqrd_wSTxQrwP1fZHu3dmvdTmKE0gdawOza56YKcVkwhePK8POug6tojFXs1VhVpF7oPWOgPC7_EudX723J1DPpVhYC14BXXCaGa0ba8u0mfw",
      "e": "AQAB"
    }
  ]
}
```

`docker/ci/oidc/mappings/jwks.json`:

```json
{
  "request": { "method": "GET", "url": "/jwks" },
  "response": {
    "status": 200,
    "headers": { "Content-Type": "application/json" },
    "bodyFileName": "jwks.json"
  }
}
```

`docker/ci/oidc/mappings/discovery.json`:

```json
{
  "request": { "method": "GET", "url": "/.well-known/openid-configuration" },
  "response": {
    "status": 200,
    "headers": { "Content-Type": "application/json" },
    "jsonBody": {
      "issuer": "http://postit-oidc:8080",
      "jwks_uri": "http://postit-oidc:8080/jwks",
      "authorization_endpoint": "http://postit-oidc:8080/authorize",
      "token_endpoint": "http://postit-oidc:8080/token",
      "response_types_supported": ["code"],
      "subject_types_supported": ["public"],
      "id_token_signing_alg_values_supported": ["RS256"]
    }
  }
}
```

- [ ] **Step 2: Write the smoke compose file**

`docker/ci/docker-compose.smoke.yml`:

```yaml
# CI container smoke test (plan 02 "Testing and CI", docker job): the built image as
# separate api and worker containers, against Postgres and a wiremock OIDC issuer that
# serves discovery and a committed public-key JWKS. Its own project, so it can never touch
# a real stack's containers or volume. Every value here is a throwaway CI value.
#
#   docker compose -f docker/ci/docker-compose.smoke.yml up -d
#   docker/ci/smoke.sh
#   docker compose -f docker/ci/docker-compose.smoke.yml down -v
#
# POSTIT_ENV=development because only development accepts an http issuer. The image
# carries no development.toml, so every value development.toml would add is set here.

name: postit-smoke

x-smoke-server: &smoke-server
    image: "${POSTIT_IMAGE:-postit-server:ci}"
    depends_on:
        postit-postgres:
            condition: service_healthy
        postit-oidc:
            condition: service_started

x-smoke-environment: &smoke-environment
    POSTIT_ENV: development
    POSTIT__SERVER__TLS__ENABLED: "false"
    POSTIT__SERVER__API_PORT: "8080"
    POSTIT__SERVER__WORKER_PORT: "8081"
    POSTIT__SERVER__PUBLIC_URL: "http://127.0.0.1:8080"
    POSTIT__DATABASE__URL: "postgres://postit-postgres:5432/postit"
    POSTIT__DATABASE__USERNAME: postgres
    POSTIT__DATABASE__PASSWORD: ci-only-password
    POSTIT__AUDIT__PSEUDONYM_KEY: ci-only-pseudonym-key-0123456789abcdef
    POSTIT__AUTH__OIDC__ISSUER: "http://postit-oidc:8080"
    POSTIT__AUTH__OIDC__AUDIENCES: '["postit"]'
    POSTIT__AUTH__BOOTSTRAP__ADMIN_EMAIL: admin@postit.test
    POSTIT__MAIL__FROM_ADDRESS: "postit <noreply@postit.test>"
    POSTIT__MAIL__SMTP__HOST: localhost
    POSTIT__MAIL__SMTP__PORT: "25"
    POSTIT__MAIL__SMTP__TLS: none
    POSTIT__APP__PUBLIC_URL: "http://127.0.0.1:8082"
    POSTIT__CORS__ALLOWED_ORIGINS: '["http://127.0.0.1:8082"]'

services:
    postit-postgres:
        image: postgres:18
        environment:
            POSTGRES_PASSWORD: ci-only-password
            POSTGRES_DB: postit
        healthcheck:
            test: ["CMD-SHELL", "pg_isready -U postgres -d postit"]
            interval: 2s
            timeout: 5s
            retries: 30

    postit-oidc:
        image: wiremock/wiremock:3.13.1
        volumes:
            - "./oidc:/home/wiremock:ro"

    postit-api:
        <<: *smoke-server
        environment:
            <<: *smoke-environment
            POSTIT_ROLE: api
            POSTIT__SERVER__WEB__ENABLED: "true"
            POSTIT__SERVER__WEB__BIND: "0.0.0.0:8082"
        ports:
            - "127.0.0.1:8080:8080"
            - "127.0.0.1:8082:8082"

    postit-worker:
        <<: *smoke-server
        environment:
            <<: *smoke-environment
            POSTIT_ROLE: worker
        ports:
            - "127.0.0.1:8081:8081"
```

If `wiremock/wiremock:3.13.1` does not exist, use the newest `3.x` tag at https://hub.docker.com/r/wiremock/wiremock/tags.

- [ ] **Step 3: Write the probe script**

`docker/ci/smoke.sh`:

```bash
#!/usr/bin/env bash
# Waits for every probe of the smoke stack to answer 200, or fails with the containers'
# logs. Run after `docker compose -f docker/ci/docker-compose.smoke.yml up -d`.
set -euo pipefail

compose=(docker compose -f "$(dirname "${BASH_SOURCE[0]}")/docker-compose.smoke.yml")
urls=(
    http://127.0.0.1:8080/health
    http://127.0.0.1:8080/ready
    http://127.0.0.1:8081/health
    http://127.0.0.1:8081/ready
    http://127.0.0.1:8082/config.json
)

for url in "${urls[@]}"; do
    for _ in $(seq 1 90); do
        if curl -fsS -o /dev/null "$url"; then
            echo "ok   $url"
            continue 2
        fi
        sleep 2
    done
    echo "smoke: $url never answered 200" >&2
    "${compose[@]}" ps >&2
    "${compose[@]}" logs --no-color >&2
    exit 1
done

# /config.json must describe this deployment, not a dev default baked into the build.
curl -fsS http://127.0.0.1:8082/config.json | grep -q '"ENV"' ||
    { echo "smoke: /config.json has no ENV" >&2; exit 1; }
echo "smoke: all probes passed"
```

`/ready` answers 200 on the API only once the JWKS is loaded from `postit-oidc`, and on the worker only once the job storage is migrated. So these probes prove the image reached the IdP and the database, not just that the process started.

- [ ] **Step 4: Run it locally**

From the repo root in Git Bash:

```bash
docker build -f docker/server.Dockerfile -t postit-server:ci .
docker compose -f docker/ci/docker-compose.smoke.yml up -d
bash docker/ci/smoke.sh
docker compose -f docker/ci/docker-compose.smoke.yml down -v
```

Expected: five `ok` lines and `smoke: all probes passed`. If the API never becomes ready, read `postit-api`'s log first. A config error naming a missing key means the smoke environment block lacks a value that `development.toml` normally supplies: add it to `x-smoke-environment`. A discovery issuer mismatch means the issuer comparison does not normalize the trailing slash: make the discovery `issuer` value match exactly what the log says the server expects. Do not loosen the server's issuer check.

- [ ] **Step 5: Add the CI job**

```yaml
  docker:
    name: docker
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: stack doctor tests
        # Launcher logic, fixture vaults only; no Docker needed.
        run: bash docker/tests/stack-doctor.sh
      - uses: docker/setup-buildx-action@v3
      - name: build image
        uses: docker/build-push-action@v6
        with:
          context: .
          file: docker/server.Dockerfile
          tags: postit-server:ci
          load: true
          push: false
          cache-from: type=gha
          cache-to: type=gha,mode=max
      - name: start smoke stack
        run: docker compose -f docker/ci/docker-compose.smoke.yml up -d
      - name: probe health, readiness and web config
        run: bash docker/ci/smoke.sh
      - name: stop smoke stack
        if: always()
        run: docker compose -f docker/ci/docker-compose.smoke.yml down -v
```

- [ ] **Step 6: Lint and commit**

Run actionlint (Task 5, Step 2). Expected: clean.

```bash
git add docker/ci .github/workflows/ci.yml
git commit -m "ci: add the docker job with an api/worker container smoke test"
```

---

### Task 8: Environment variable reference

**Files:**
- Create: `docs/configuration.md`, `server/crates/config/tests/reference_doc.rs`

**Interfaces:**
- Consumes: `tests/common::{shipped_config_dir, production_vars}` (Task 1), `Settings::redacted_dump()`.
- Produces: `docs/configuration.md` documents every settings key as its `POSTIT__…` variable. The test fails, listing the missing names, whenever a settings field is added without a doc entry.

- [ ] **Step 1: Write the failing test**

`server/crates/config/tests/reference_doc.rs`:

```rust
//! Every setting is documented in `docs/configuration.md` under its environment
//! variable name, so the reference cannot fall behind the settings tree.

mod common;

use postit_config::{Environment, load_with};

use common::{production_vars, shipped_config_dir};

fn env_names(prefix: &str, value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                env_names(&format!("{prefix}__{}", key.to_uppercase()), child, out);
            }
        }
        _ => out.push(prefix.to_owned()),
    }
}

#[test]
fn every_setting_is_in_the_reference() {
    let settings = load_with(Environment::Production, &shipped_config_dir(), production_vars())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let dump = settings.redacted_dump().unwrap_or_else(|e| unreachable!("{e}"));
    let mut names = Vec::new();
    env_names("POSTIT", &dump, &mut names);

    let doc_path = shipped_config_dir().join("../../docs/configuration.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| unreachable!("reading {}: {e}", doc_path.display()));
    let missing: Vec<&String> = names
        .iter()
        .filter(|name| !doc.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/configuration.md is missing {} setting(s):\n{}",
        missing.len(),
        missing.iter().map(|n| format!("  {n}")).collect::<Vec<_>>().join("\n")
    );
}
```

Add `serde_json.workspace = true` to `[dev-dependencies]` in `server/crates/config/Cargo.toml` if the integration test cannot see the normal dependency (integration tests can use regular dependencies, so this is normally unnecessary).

- [ ] **Step 2: Run it to get the full list**

Run: `cargo test -p postit-config --test reference_doc`
Expected: FAIL with "reading …/docs/configuration.md". Create an empty `docs/configuration.md` and re-run. It now fails with the complete list of variable names, which is the table's row set.

- [ ] **Step 3: Write `docs/configuration.md`**

Structure:

1. **How configuration is loaded.** The layering from plan 02, in order: `config/default.toml`, `config/{env}.toml`, `config/local.toml` (development only), the `POSTIT_SECRETS_FILE` dotenv file, `POSTIT__SECTION__KEY` variables, `…_FILE` variants. `POSTIT_ENV` (`development`/`qa`/`production`; a release build without it refuses to start) and `POSTIT_ROLE` (`all`/`api`/`worker`). How lists are written (`'["a","b"]'`) and durations (`"30s"`, `"5min"`, `"30days"`, humantime syntax).
2. **Secrets.** The five secret keys (`POSTIT__DATABASE__USERNAME`, `POSTIT__DATABASE__PASSWORD`, `POSTIT__AUDIT__PSEUDONYM_KEY`, `POSTIT__MAIL__SMTP__USERNAME`, `POSTIT__MAIL__SMTP__PASSWORD`). Each can be a plain variable or `…_FILE` pointing at a mounted file (Docker/Kubernetes secrets, ECS and Container Apps secret mounts). `database.url` must not carry credentials. Quote values containing `$` with single quotes.
3. **Required outside development.** A table of what qa and production refuse to start without: issuer (https), audiences (non-empty), database password, SMTP host, mail sender, pseudonym key (every environment), plus the production bootstrap rule when no active admin exists. Cross-reference the production compose `${VAR:?…}` names.
4. **Reference.** One table per top-level section (`server`, `database`, `auth`, `audit`, `retention`, `ops`, `rate_limit`, `mail`, `jobs`, `http`, `app`, `cors`), columns *Variable*, *Default* (from `default.toml`, or "required"), *Secret* (yes/no), *Meaning* (from the field's doc comment in `server/crates/config/src/settings.rs`). Write each variable name in backticks exactly as the test printed it. Map keys such as `POSTIT__JOBS__CONCURRENCY__MAIL` get their own rows.

- [ ] **Step 4: Run the test**

Run: `cargo test -p postit-config --test reference_doc`
Expected: PASS.

- [ ] **Step 5: Gates and commit**

Run the four Rust gates.

```bash
git add docs/configuration.md server/crates/config
git commit -m "docs: environment variable reference, checked against the settings tree"
```

---

### Task 9: Deployment guide, migration checklist, README

**Files:**
- Create: `docs/deployment.md`, `.github/pull_request_template.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: everything above. Facts from plan 02 and plan 01 (identity model, data model), `docker/shared/nginx/app.conf` (the CSP the dev web server already sends), `server/config/*.toml`.
- Produces: the operator documentation P9 lists, host-neutral.

- [ ] **Step 1: Write the migration review checklist**

`.github/pull_request_template.md`:

```markdown
## Summary

<!-- What changes and why. -->

## Checks

- [ ] Quality gates pass (`cargo fmt --check`, clippy `-D warnings`, `cargo test --workspace`; web: typecheck, lint, format, test).
- [ ] `cargo xtask openapi` re-run if any route or schema changed; `/api/v1` changes are additive only.

## Migration review (only if `server/crates/data/migrations/` changed)

During an upgrade the new `api` container migrates while the old `worker` still runs, so a migration must work with the previous release's code (docs/deployment.md, "Upgrades").

- [ ] Only adds: new tables, new nullable or defaulted columns, new indexes (`CONCURRENTLY` where the table can be large).
- [ ] Drops nothing and renames nothing that the previous release reads. A removal waits for a later release (expand, then contract).
- [ ] No new `NOT NULL` column without a default on an existing table.
- [ ] Needs no superuser right beyond the database owner role (trusted extensions only, such as `citext`).
- [ ] The `.sqlx` caches are regenerated.
```

- [ ] **Step 2: Write `docs/deployment.md`**

Sections and the facts each must state:

1. **What you deploy.** One image, `postit-server:<version>` and `:<git-sha>`, built from `docker/server.Dockerfile` with the repository root as context. The same image goes dev → qa → production; only configuration changes. Roles: `api` (HTTP API on 8080, web app on 8082 when `server.web.enabled`) and `worker` (jobs, `/health` and `/ready` on 8081), as two services or tasks. `all` is for development. Plain HTTP in the container; something in front terminates TLS. `postit healthcheck` is the container health check. `/ready` semantics per role (API: database plus JWKS loaded; worker: database plus job storage migrated) and which to use for a load balancer's target health (API `/ready` on 8080). Graceful shutdown: `server.shutdown_timeout` 30 s, so give the orchestrator a stop timeout above it (compose uses 40 s; ECS `stopTimeout`, Container Apps termination grace period).
2. **Where it can run.** Any container host: a VM with the qa/production compose files (the reference deployment), AWS ECS/Fargate or App Runner, Azure Container Apps, Kubernetes. Each needs: two services from one image, a managed or self-run Postgres 18, secrets as env vars or files (`_FILE`), and TLS in front. Vercel or other static hosts can at most serve the web build, which the image already serves. Cloud-specific infrastructure is planned separately once a provider is chosen.
3. **First boot (production).** Required values (link `docs/configuration.md`, "Required outside development"). The bootstrap admin: set `auth.bootstrap.admin_email` (verified email at the IdP) or `admin_subject`; production refuses to start with no active admin and no rule; the variable may be blank once an admin exists. Migrations run automatically under an advisory lock on every start, so concurrent `api` and `worker` starts are safe. Expected log lines (the redacted config dump at startup).
4. **Identity provider.** Generic requirements: OIDC discovery at the issuer, JWT (not opaque) access tokens whose `aud` includes `auth.oidc.audiences`, a public client with authorization code + PKCE and refresh tokens (`offline_access`), redirect URIs (web `https://<app host>/auth/callback`; Swagger `https://<api host>/docs/oauth2-redirect.html` where `/docs` is on; mobile `postit-qa://auth/callback` and `postit://auth/callback` once the P11 app exists), post-logout redirect URIs (the web origin, the mobile schemes), the web origin allowed to frame the IdP for silent renew (same-site IdP and app recommended), `email` and `email_verified` claims for `admin_email` bootstrap. **Zitadel production setup**, step by step: an organization; a project `postit` whose ID is the audience; a user-agent application with PKCE, *JWT* access token type, refresh tokens, redirect and post-logout URIs per environment and platform (a table: qa and production × web, Swagger, mobile); development mode off outside development (mobile custom schemes then need Zitadel's native app type, noted for P11); the admin user with a verified email. Note that `cargo xtask zitadel-bootstrap` automates the development equivalent and is a reference for the API calls. IdP outage behavior: issued tokens keep working until expiry because the JWKS is cached.
5. **TLS, reverse proxy and security headers.** The Caddy example for one host serving `api.` and `app.`, with the web app's headers on the app host:

   ```caddyfile
   api.postit.com {
       reverse_proxy 127.0.0.1:8080
   }

   app.postit.com {
       header {
           Content-Security-Policy "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' https://api.postit.com https://auth.postit.com; frame-src 'self' https://auth.postit.com; frame-ancestors 'self'; base-uri 'self'; object-src 'none'"
           Strict-Transport-Security "max-age=31536000"
           X-Content-Type-Options "nosniff"
           Referrer-Policy "strict-origin-when-cross-origin"
       }
       reverse_proxy 127.0.0.1:8082
   }
   ```

   The CSP is the one `docker/shared/nginx/app.conf` sends in development, with the hostnames swapped. Explain each origin: `connect-src` the API and the IdP, `frame-src` the IdP for the silent sign-in iframe, `frame-ancestors 'self'` because the app frames its own `/auth/callback`. On a cloud load balancer (ALB, Front Door, Application Gateway) the same headers are set on the listener/rules or the CDN. **Trusted proxies:** `server.trusted_proxies` must list exactly the hop that sets `X-Forwarded-For`. On the compose deployment that is postit-net's gateway `172.30.0.1/32` (the default); behind a cloud load balancer it is the load balancer's subnet CIDRs. Wrong values mean either one shared rate-limit bucket (too narrow) or spoofable client IPs (too wide).
6. **Web hosting options.** (a) A separate app host proxied to port 8082 (`server.web.bind`), the default in the compose files, with CORS set to the app origin. (b) The web app at `/` on the API origin (`server.web.bind` unset), which needs no CORS. `/config.json` supplies `API_BASE_URL` (from `server.web.api_base_url`) and `ENV` at runtime, so one build serves every environment. Caching: `/assets` immutable, `index.html` and `config.json` `no-cache`.
7. **Database.** Postgres 18. postit connects as the owner of its database and migrates with that role. The role needs `CREATE` on the database (for the `apalis` schema) and the trusted extension `citext`. Managed services: on Amazon RDS and Azure Database for PostgreSQL Flexible Server, `citext` is available; on Azure it must first be allow-listed in the server parameter `azure.extensions`. If the platform forbids the owner from creating it, create it once as the admin role: `CREATE EXTENSION IF NOT EXISTS citext;` in the postit database, after which the migration's `IF NOT EXISTS` passes. `database.url` has no credentials; the username and password are secrets. Pool size `database.max_connections` (default 10) per process, so size the server's connection limit for (api tasks + worker tasks) × 10 plus headroom.
8. **Backups.** Compose deployments: `stack backup [environment]` and `stack restore [environment] <file>`, with every rail from Task 4 and the warning that a natively run server is invisible to the running-server check. A dump is unencrypted user data; keep it out of git and delete it when done. Managed Postgres: rely on the provider's automated snapshots and point-in-time recovery, and still take a logical dump before an upgrade. What a dump does not contain: the vault (secrets), the IdP's data (Zitadel has its own database), and media files once plan 03 adds them. Restore order: stop api and worker, restore, start. Test a restore regularly.
9. **Upgrades.** Image promotion: build once, run in qa, promote the same tag (`POSTIT_IMAGE` on compose; the task definition or revision image elsewhere); never rebuild for production. Order: back up, roll the `api` service first (it migrates), then the `worker`. The old worker keeps running against the new schema during the roll, which is why migrations follow expand-then-contract: a release only adds, and removals happen a release later once no running version reads the old shape. The PR template's migration checklist enforces it. API compatibility: `/api/v1` changes are additive, enforced by the CI `contract` job against the last `v*` tag. Rollback: roll back the image, which is safe because the previous release works with the expanded schema; restore a backup only if data went wrong.
10. **Reproducing qa or production locally.** `stack doctor <env>`, then `stack up qa` / `stack up production` with `docker/.env` set. One host runs one environment; switch with `stack reset <env>`. The CI `docker` job is the automated equivalent (api and worker containers against a stub IdP).
11. **Building from source.** Server: `cargo build --profile dist -p postit-server` from `server/` (`SQLX_OFFLINE=true` is the default in `server/.cargo/config.toml`). Image: `docker build -f docker/server.Dockerfile -t postit-server:<version> -t postit-server:$(git rev-parse --short HEAD) .`. Web app alone: Node 24, `pnpm install --frozen-lockfile && pnpm build` in `web/`, output `web/dist`, served by any static host with the SPA fallback and the `/config.json` contract. Mobile apps: arrive with plan 02 P11 (Flutter, `--dart-define-from-file=config/<env>.json`); this section is extended then.

- [ ] **Step 3: Update `README.md`**

- Multi-environment table: drop "— from P6" in the QA and Production rows. Add one sentence after it: "Compose runs development and reproduces qa or production on one host; it is one deployment option among container hosts. See [docs/deployment.md](docs/deployment.md)."
- Hostnames, production bullet: replace the `${VAR:-default}` text with "Each value is a required variable in `docker/.env` (copy `docker/.env.example`); compose refuses to resolve the file without them, and `stack doctor production` lists what is missing."
- The server image: delete the "Still to come from plan 02: …" paragraph (all of it shipped).
- Reverse proxy: keep the short Caddy example and link docs/deployment.md for security headers, trusted proxies and cloud load balancers.
- PostgreSQL named volume: replace "Nothing here backs it up yet (plan 02 P9)." with "Back it up with `stack backup` (see [Backups](docs/deployment.md#backups))."
- Ports table: the 44310/44315 development cells now say `cargo run`/`pnpm dev`, or `postit-nginx-app` (no longer a placeholder).
- Continuous integration: list all five jobs (`server (linux)`, `server (windows)`, `web`, `contract`, `docker`) with one line each, plus the `app` job arriving in P11.
- Common commands: add `doctor`, `backup`, `restore` rows.
- Add a "Documentation" line near the top linking `docs/deployment.md` and `docs/configuration.md`.

- [ ] **Step 4: Check every relative link**

Run, from the repo root: `grep -oE '\]\((docs/[^)#]+)' README.md docs/*.md | sort -u`
For each path printed, confirm the file exists. Expected: all exist.

- [ ] **Step 5: Commit**

```bash
git add docs/deployment.md .github/pull_request_template.md README.md
git commit -m "docs: host-neutral deployment guide, migration checklist, README for P9"
```

---

### Task 10: Amend plan 02 and CLAUDE.md, then verify the phase

**Files:**
- Modify: `!ref/plans/02. foundation.md` (P9 section, "QA and production hosting", "Testing and CI"), `CLAUDE.md` ("Project state")

**Interfaces:**
- Consumes: the amendments at the top of this plan.
- Produces: plan 02 matches what was built.

- [ ] **Step 1: Amend plan 02**

- "QA and production hosting", first bullet: append "Hosting is not chosen yet (AWS, Azure, or other). The image and its environment contract are host-neutral; the compose files are the reference single-host deployment and the way to reproduce an environment locally. A later plan covers cloud infrastructure for the chosen provider."
- P9 bullets: add "Add `stack doctor`, `stack backup` and `stack restore` (`stack.sh`/`stack.ps1`)." Change the docs bullet to name `docs/deployment.md` and `docs/configuration.md`, and "backup notes" to "backup and restore (the `stack` commands and managed-database snapshots)".
- P9 exit, second bullet: replace with "The CI `docker` job boots the image as separate `api` and `worker` containers against Postgres and a stub OIDC issuer, and `/health`, `/ready` and `/config.json` answer 200. `stack up qa` reproduces qa locally."
- P9 exit, first bullet: append "(enforced by `postit-config` validation and the startup bootstrap check, so it holds on any host; the production compose file also refuses to resolve without its deployment values)".
- "Testing and CI", `contract` job: append "The previous release is the newest `v*` tag; with none, the check is skipped with a notice."

- [ ] **Step 2: Update CLAUDE.md "Project state"**

Change "P9 onward (Flutter mobile app in P11, and later) is not done; qa/production compose completion and the web CI job are plan 02 P9." to "plan 02 phases through P9 (… existing list …; P9: production startup requirements in `postit-config`, `stack doctor`/`backup`/`restore`, the `web`, `contract` and `docker` CI jobs, `docs/deployment.md` and `docs/configuration.md`). P10 onward (hardening, then the Flutter app in P11) is not done. Hosting is undecided; the image is host-neutral." Add `./stack.sh doctor [environment]`, `./stack.sh backup [environment]` and `./stack.sh restore [environment] <file>` to the Docker commands block.

- [ ] **Step 3: Full verification**

From `server/`: the four Rust gates. From `web/` (PowerShell): `pnpm typecheck; pnpm lint; pnpm format:check; pnpm test`. From the repo root: `bash docker/tests/stack-doctor.sh`, then the Task 7 Step 4 smoke run, then actionlint.
Expected: all pass. Record the outputs in the task report.

- [ ] **Step 4: Commit**

```bash
git add "!ref/plans/02. foundation.md" CLAUDE.md
git commit -m "docs(plans): record P9 as built; hosting stays host-neutral"
```

- [ ] **Step 5: CI is green (needs the user)**

P9's last exit criterion is "All CI jobs are green", which needs a push. Do not push without the user's go-ahead. Report the branch and ask. After the push, check `gh run list --branch <branch> --limit 1` and `gh run view <id>`. Fix any job that fails only on the runner (paths, missing tools) in a follow-up commit.
