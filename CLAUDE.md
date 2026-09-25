# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

`postly` is a self-hosted social publishing service for a small team: a Rust (edition 2024) REST API + background worker with a Flutter client (web, desktop, mobile). Each user connects their own TikTok, Facebook, Instagram, YouTube, and X accounts, drafts or AI-generates posts, and publishes them immediately or on a schedule. It is **not** a CLI.

The repository is currently a **starter**: a single-package workspace (`members = ["."]`) whose `src/main.rs` is a hello-world. Dependencies in `Cargo.toml` are placeholders. Plan 02 phase P1 moves the workspace under `server/`.

The implementation plans live in `!ref/plans/` (untracked reference material). Read them before starting any feature work:

- `01. social publishing platform roadmap.md` — revision 2. **Superseded** by plan 03; kept for history.
- `02. API server, identity, environments, and Flutter baseline.md` — the foundation, phases P1…P9: monorepo (`server/`, `app/` Flutter client, `api/openapi.json`), apalis jobs on PostgreSQL behind `postly-jobs`, Docker, local users only (no OIDC), three environments (`development`, `qa`, `production`), dev on `https://postly.local` with ports 44300–44399 (API 44300, worker 44305, web 44310).
- `03. social publishing roadmap, revision 3.md` — **current roadmap**, phases A1…G3 built on plan 02. Data model, REST surface, key flows, dependency map.

Where plans disagree, the higher-numbered plan wins.

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

Quality gates the roadmap requires (Phase A1 exit criteria) — keep them passing:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
```

## Target architecture (plan 03, roadmap revision 3)

Multi-crate workspace under `server/crates/`, pinned to Rust 1.98.1, package names prefixed `postly-`. Plan 02 crates: `config`, `data`, `identity`, `jobs` (only crate depending on apalis), `api` (axum + utoipa), `server` (bin `postly`, composition root, builds the `PluginRegistry` with each platform behind a Cargo feature). Plan 03 adds:

- `core` — domain and plugin contract: open `PlatformId` newtype, `AccountRef`, `Post`/`PostOverrides`/`PostVersion`, `MediaSource` (`Stored`/`PublicUrl`/`RemoteRef`, no local paths), `MediaStore`/`MediaHost`/`MediaProbe` traits, `PlatformPlugin` + `PlatformClient`, `PluginRegistry`, `DeliveryState` (incl. `OutcomeUnknown`, `Parked`), validation engine, `Clock`/`IdGenerator`.
- `http` — the only place that builds `reqwest` clients: retry classification, backoff+jitter, governor rate limiting, redaction, streaming bodies.
- `vault` — envelope encryption (XChaCha20-Poly1305, versioned keys) for social tokens, PKCE verifiers, and LLM keys stored in Postgres.
- `oauth` — social OAuth 2.0 + PKCE from plugin-supplied `OAuthProviderSpec`, server-side callback `/api/v1/platforms/{id}/oauth/callback`, `PgTokenStore`, refresh serialized across processes with Postgres advisory locks.
- `media` — filesystem storage, streamed upload intake, probing, HMAC-signed expiring public URLs for platforms, GC.
- `platform-kit` (+ `testkit` conformance suite), `platforms/{mock,x,youtube,meta,facebook,instagram,tiktok}`.
- `content` — `LlmProvider` (`openai-compatible` + native `anthropic`), per-user opencode-shaped provider rows.
- `publishing` — publish jobs, one delivery per account, state machine, reconcile, native scheduling, due sweep; job handlers registered via `postly-jobs`.
- `notify` — in-app notifications + critical-event email.

Cross-cutting rules:

- **Private per user.** Every domain table has `owner_id`; owner scoping lives only in `postly-data` repositories and is proven by isolation tests. Admins manage users and deployment settings, never user content or secrets.
- Platform developer apps are deployment-level config (`[platforms.<id>]`); `platforms` table uses the plugin ID as a text primary key, upserted at startup.
- Shared code consumes the plugin contract and core models only; platform API types never leak out of platform crates. Adding a platform = new crate + feature + one registration line.
- DRY: one owner per concern (plan 03 "Engineering standards"); workspace-level dependencies and `[workspace.lints]`.
- Secrets use `secrecy` types and are stored only vault-sealed; never log tokens, auth codes, API keys, or signed URLs; redaction is tested.
- Live publishing is on in every environment but every live publish is explicitly confirmed; a runtime kill switch and per-platform `enabled` flags stop it; dry-run works everywhere. CI never publishes live.
- Platform tests use wiremock plus the conformance suite; live smoke tests are opt-in (`POSTLY_LIVE_TESTS=1`).
- Large media is streamed/chunked, never fully buffered.
- Partial success is preserved per delivery; non-idempotent failures become `OutcomeUnknown` and are reconciled, never blindly retried.
- essentialMix-rs crates from crates.io with `"0"` specs: `emixdb` (pagination), `emixcrypto` (SHA-256, `default-features = false`). Not `emixai`/`emixnet`/`emixlog` — reasons in plan 03.
