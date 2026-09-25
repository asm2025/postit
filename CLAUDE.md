# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

`postly` is a Rust (edition 2024) CLI for authenticating social accounts, drafting/generating posts, and publishing them immediately or on a schedule to TikTok, Facebook, Instagram, YouTube, and X.

The repository is currently a **starter**: a single-package workspace (`members = ["."]`) whose `src/main.rs` is a hello-world. Dependencies in `Cargo.toml` (tokio, reqwest, serde, chrono, dotenv, …) are placeholders for the planned work. Workspace metadata (version, edition, license, authors, repository) lives in `[workspace.package]` and is inherited by packages via `*.workspace = true`.

The implementation plans live in `!ref/plans/` (untracked reference material). Read them before starting any feature work:

- `01. social publishing platform roadmap.md` — revision 2; phases A1…G3 with an explicit dependency map. Partly superseded by plan 02 (see its "Impact on the roadmap" section); plan 03 is the roadmap rewrite.
- `02. API server, identity, environments, and Flutter baseline.md` — **current direction.** postly is not a CLI: it is a Rust REST API + background worker (apalis on PostgreSQL, Dockerized) in a monorepo (`server/`, `app/` Flutter client, `api/openapi.json`), with local users only (no OIDC), three environments (`development`, `qa`, `production`), and a dev setup on `https://postly.local` with ports 44300–44399 (API 44300, worker 44305, web 44310). Phases P1…P9.

Where plan 02 and the roadmap/sections below disagree, plan 02 wins.

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

## Target architecture (per roadmap, revision 2)

The plan converts the repo into a multi-crate workspace pinned to Rust 1.98.1 (`rust-toolchain.toml`), with the binary moved to `crates/app`. Package names use the `postly-` prefix.

- `crates/core` — platform-agnostic domain and plugin contract: open `PlatformId` newtype (no closed enum), `AccountRef` (multiple accounts per platform), `Post`/`PostOverrides`/`PostVersion`, `MediaAsset`/`MediaSource` (streamed), `MediaHost`/`MediaProbe` traits, `PublishReceipt`, `DeliveryState` (incl. `OutcomeUnknown`), `PlatformPlugin` + `PlatformClient` traits, `PluginRegistry`, the capability-driven validation engine, structured errors, `Clock`/`IdGenerator` seams.
- `crates/config` — `postly.toml` + env overlay; opaque `[platforms.<id>]` sections parsed by each plugin; `{env:}`/`{file:}`/`{keyring:}` secret references.
- `crates/http` — the only place that builds `reqwest` clients: rustls, pooling, timeouts, tracing, redaction, retry classification with backoff+jitter, governor rate limiting, streaming bodies.
- `crates/auth` — generic OAuth 2.0 + PKCE driven by plugin-supplied `OAuthProviderSpec` (no provider config here), loopback/manual callback, per-account serialized refresh, `TokenStore` (keyring, in-memory, opt-in encrypted file).
- `crates/data` — SQLite via SQLx (offline `.sqlx` cache): drafts, jobs, per-account deliveries, attempts, receipts, idempotency keys. Platform IDs stored as text. Token *references* only.
- `crates/platform-kit` — shared plugin machinery (chunked/resumable upload driver, `poll_until`, error classification, `TextMeasure`) and the `testkit` conformance suite every plugin must pass.
- `crates/platforms/mock` — reference plugin used by tests and as the template for new platforms.
- `crates/platforms/{x,youtube,facebook,instagram,tiktok}` — compile-time plugins implementing `PlatformPlugin`; `crates/platforms/meta` is a shared Graph API/Meta OAuth library used by facebook and instagram.
- `crates/scheduler` — durable queue: leases, retry budgets, dead-letter, reconcile of unknown outcomes, `postly worker [--once]`, opt-in native platform scheduling.
- `crates/content` — `LlmProvider` with two adapters: `openai-compatible` (OpenAI, Abacus RouteLLM, Gemini's OpenAI endpoint, OpenRouter, Ollama, …) and native `anthropic`; provider config mirrors opencode's `provider` block. Propagates `disclose_ai_generated`.
- `crates/app` — CLI and composition root; builds the `PluginRegistry` in one place with each platform behind a Cargo feature.

Cross-cutting rules from the roadmap:

- Shared code consumes the plugin contract and core models only; platform API types never leak out of platform crates. Adding a platform = new crate + feature + one registration line.
- DRY: one owner per concern (see roadmap "Engineering standards"); workspace-level dependencies and `[workspace.lints]`.
- Secrets use `secrecy` types; never log tokens, auth codes, API keys, or sensitive URLs; redaction is tested.
- Real publishing is behind an explicit live path; dry-run works everywhere. Live publishing is disabled by default in dev and CI.
- Platform tests use mocked HTTP (wiremock) plus the shared conformance suite; live smoke tests are opt-in (`POSTLY_LIVE_TESTS=1`). CI never needs real credentials.
- Large media is streamed/chunked, never fully buffered.
- Multi-target publishes record one delivery per account and preserve partial success; non-idempotent failures become `OutcomeUnknown` and are reconciled, never blindly retried.
- essentialMix-rs crates are used from crates.io with `"0"` version specs: `emixdb` (pagination), `emixcrypto` (SHA-256, `default-features = false`), `emix` (`terminal` prompts), `emixthreading` (spinner). Not `emixai`/`emixnet`/`emixlog` — reasons in the roadmap.
