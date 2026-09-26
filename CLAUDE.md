# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

`postit` is a self-hosted social publishing service for a small team: a Rust (edition 2024) REST API + background worker with a Flutter client (web, desktop, mobile). Users sign in through a generic OIDC provider (Zitadel in dev), connect their own TikTok, Facebook, Instagram, YouTube, and X accounts, draft or AI-generate posts, and publish them immediately or on a schedule. Owners can delegate scoped access to other users. It is **not** a CLI.

The repository is currently a **starter**: a single-package workspace (`members = ["."]`) whose `src/main.rs` is a hello-world. Dependencies in `Cargo.toml` are placeholders. Plan 02 phase P1 moves the workspace under `server/`.

The implementation plans live in `!ref/plans/`. They are one consistent set, not layered revisions; read them before starting any feature work:

- `01. vision and architecture.md` — decisions, engineering standards, crate map and dependency order, identity and access model (OIDC, pending approval, delegation), data model, non-goals, definition of done.
- `02. foundation.md` — phases P1…P10: monorepo (`server/`, `app/` Flutter client, `api/openapi.json`), config, `postit-http`, Postgres, OIDC identity, apalis jobs behind `postit-jobs` with a Hangfire-style admin Jobs console (P8), mail, API shell, Docker with bundled Zitadel, three environments (`development`, `qa`, `production`), dev on `https://postit.local` with ports 44300–44399 (API 44300, worker 44305, web 44310, Zitadel 44330).
- `03. publishing roadmap.md` — phases A1…G3: plugin contract, vault, social OAuth, media, delegation (B6), platforms, publishing engine, LLM drafting, Flutter publishing UI. REST surface with per-route access levels, key flows, dependency map.

`!ref/plans/archive/` holds the superseded layered plans (old local-users/password design). Do not implement from them.

## Commands

```sh
cargo build                     # dev build (dependencies compiled at opt-level 3)
cargo build --release           # release: lto=true, codegen-units=1
cargo build --profile dist      # release with thin LTO
cargo run -- <args>
cargo test                      # all tests
cargo test <name_substring>     # single test / filter
cargo test -p <crate> <name>    # single test in a specific crate (once split into crates)
```

Quality gates required by every phase (plan 02 P1 exit criteria) — keep them passing:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
```

Docker lives under `docker/`: base `docker-compose.yml` plus exactly one
`docker-compose.<environment>.yml` (`development`, `qa`, `production`), project name pinned
to `postit`, every service/container `postit-`-prefixed (`postit-postgres`, `postit-zitadel`,
`postit-api`, …) on network `postit-net` with volume `postit-pgdata`, and the image built
from `docker/server.Dockerfile` (context: repo root). Never type the `-f` chain by hand —
use the launcher from the repo root:

```sh
./stack.sh up [environment] [--app]      # ./stack.ps1 up [environment] [-App] on Windows
./stack.sh config qa                     # resolved compose config, no side effects
./stack.sh down -v                       # dev: also wipes the DB and the cached Zitadel key
./stack.sh help                          # ps, logs, restart, build, psql, reset
```

qa/production compose files are not runnable until plan 02 P6 (the binary serves nothing
yet); their hostnames are `{api,app,auth}.qa.postit.com` and `{api,app,auth}.postit.com`.

**Vault.** Per-environment secrets are packed into committed `!ref/vault.7z`; extract with
`7z x vault.7z` from `!ref/` (password out of band) to get `!ref/vault/<environment>/`.
`!ref/vault/` itself is git-ignored except for `.gitkeep` — never commit the extracted
files in the clear. Layout: one folder per environment, **one `KEY=value` file per project**
(`postgres.env`, `postit.env`, and `zitadel.env` in development only), holding **secrets
only** — usernames, passwords, keys, client secrets. URLs and all other settings are config
and stay with the project (`server/config/<environment>.toml`, the compose files);
`database.url` must not carry credentials. Every environment, development included, uses the
default `postgres` superuser with a random password from the vault. Containers read the files
with `env_file:`; a native `cargo run` reads `postit.env` through `POSTIT_SECRETS_FILE`
(default set in `server/.cargo/config.toml`). The maintainer re-packs `vault.7z` after a
vault change; Claude edits the extracted files only and never re-packs the archive.

## Target architecture (plan 01)

Multi-crate workspace under `server/crates/`, on Rust stable with a `rust-version = "1.98"` floor, package names prefixed `postit-`. Plan 02 crates: `config`, `http`, `data`, `jobs` (only crate depending on apalis), `mail`, `identity` (OIDC verification, claims transformation, users, delegation resolution), `api` (axum + utoipa), `server` (bin `postit`, composition root, builds the `PluginRegistry` with each platform behind a Cargo feature), plus a `core` skeleton (ID newtypes, `Clock`, `IdGenerator`) created in P1. Plan 03 adds:

- `core` (extended in A1) — domain and plugin contract: open `PlatformId` newtype, `AccountRef`, `Post`/`PostOverrides`/`PostVersion`, `MediaSource` (`Stored`/`PublicUrl`/`RemoteRef`, no local paths), `MediaStore`/`MediaHost`/`MediaProbe` traits, `PlatformPlugin` + `PlatformClient`, `PluginRegistry`, `DeliveryState` (incl. `OutcomeUnknown`, `Parked`), validation engine, `Clock`/`IdGenerator`.
- `vault` — envelope encryption (XChaCha20-Poly1305, versioned keys) for social tokens, PKCE verifiers, and LLM keys stored in Postgres.
- `oauth` — social OAuth 2.0 + PKCE from plugin-supplied `OAuthProviderSpec`, server-side callback `/api/v1/platforms/{id}/oauth/callback`, `PgTokenStore`, refresh serialized across processes with Postgres advisory locks.
- `media` — filesystem storage, streamed upload intake, probing, HMAC-signed expiring public URLs for platforms, GC.
- `platform-kit` (+ `testkit` conformance suite), `platforms/{mock,x,youtube,meta,facebook,instagram,tiktok}`.
- `content` — `LlmProvider` (`openai-compatible` + native `anthropic`), per-user opencode-shaped provider rows.
- `publishing` — publish jobs, one delivery per account, state machine, reconcile, native scheduling, due sweep; job handlers registered via `postit-jobs`.
- `notify` — in-app notifications + critical-event email.

Cross-cutting rules:

- `http` (plan 02, extended in B1) is the only place that builds `reqwest` clients: retry classification, backoff+jitter, governor rate limiting, redaction, streaming bodies.
- **Auth is OIDC only.** No passwords or postit-issued tokens. API verifies bearer JWTs (iss, aud, exp, signature via cached JWKS); users keyed on `(issuer, sub)`, never email. Role (`admin`/`member`) and status (`pending`/`active`/`disabled`) live in Postgres; IdP role claims are ignored. First sign-in is `pending` until an admin approves; first admin comes from `auth.bootstrap`.
- **Private per user, with delegation.** Every owned table has `owner_id` plus `created_by`/`updated_by` (the actor). `OwnerScope { actor, owner, access }` is the only access check: repositories in `postit-data` filter by owner and delegation account limit; services call `scope.require(capability)`. Delegates act via `X-Postit-Act-As` at levels `view`/`edit`/`publish`; no re-delegation. Admins manage users and deployment settings, never user content or secrets.
- Platform developer apps are deployment-level config (`[platforms.<id>]`); `platforms` table uses the plugin ID as a text primary key, upserted at startup.
- Shared code consumes the plugin contract and core models only; platform API types never leak out of platform crates. Adding a platform = new crate + feature + one registration line.
- DRY: one owner per concern (plan 01 "Engineering standards"); workspace-level dependencies and `[workspace.lints]`.
- Secrets use `secrecy` types and are stored only vault-sealed; never log tokens, auth codes, API keys, or signed URLs; redaction is tested.
- Live publishing is on in every environment but every live publish is explicitly confirmed; a runtime kill switch and per-platform `enabled` flags stop it; dry-run works everywhere. CI never publishes live.
- Platform tests use wiremock plus the conformance suite; live smoke tests are opt-in (`POSTIT_LIVE_TESTS=1`).
- Large media is streamed/chunked, never fully buffered.
- Partial success is preserved per delivery; non-idempotent failures become `OutcomeUnknown` and are reconciled, never blindly retried.
- essentialMix-rs crates from crates.io with `"0"` specs: `emixdb` (pagination), `emixcrypto` (SHA-256, `default-features = false`). Not `emixai`/`emixnet`/`emixlog` — reasons in plan 01.
