# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

`postly` is a Rust (edition 2024) CLI for authenticating social accounts, drafting/generating posts, and publishing them immediately or on a schedule to TikTok, Facebook, Instagram, YouTube, and X.

The repository is currently a **starter**: a single-package workspace (`members = ["."]`) whose `src/main.rs` is a hello-world. Dependencies in `Cargo.toml` (tokio, reqwest, serde, chrono, dotenv, …) are placeholders for the planned work. Workspace metadata (version, edition, license, authors, repository) lives in `[workspace.package]` and is inherited by packages via `*.workspace = true`.

The implementation plan is `!ref/plans/01. social publishing platform roadmap.md` (the `!ref/` folder is untracked reference material). Read it before starting any feature work; phases are labeled A1…G3 with an explicit dependency map.

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

## Target architecture (per roadmap)

The plan converts the repo into a multi-crate workspace pinned to Rust 1.98.1 (`rust-toolchain.toml`), with the binary moved to `crates/app`:

- `crates/core` — platform-independent domain: `Platform` enum, `Post`, `PostOverrides`, `MediaAsset` (streaming media input), `PublishReceipt`, `AccountStatus`, structured errors, pre-network validation, and the async `PlatformClient` trait (authenticate, publish, delete, health check). No platform API logic here.
- `crates/auth` — OAuth 2.0 + PKCE, local CLI callback, token refresh, token store trait (OS keyring impl + in-memory test impl).
- Shared HTTP layer — reqwest/rustls, timeouts, tracing, redacted diagnostics, retry classification with backoff+jitter, governor rate limiting.
- `crates/data` — SQLite via SQLx: migrations, drafts, scheduled jobs, per-platform delivery attempts, receipts, idempotency keys. Stores token *references* only, never token values.
- `crates/platforms/{x,youtube,facebook,instagram,tiktok}` — each implements `PlatformClient`; all platform-specific OAuth, validation, rate limits, and upload protocols (YouTube resumable upload, TikTok chunked/pull-from-URL, Instagram container workflow) stay inside the crate.
- `crates/scheduler` — durable queue on `data` repositories: leases, retry budgets, dead-letter, per-platform delivery state, idempotency keyed by post/platform/version.
- `crates/content` — provider-neutral LLM generation behind a feature flag; propagates `disclose_ai_generated` into `Post`.
- `crates/app` — CLI (`auth login`, `generate`, `post`, `publish --dry-run`, `status`, history/retry/cancel); constructs clients via factory/registry keyed by `Platform`, not directly in command handlers.

Cross-cutting rules from the roadmap:

- Shared code consumes `PlatformClient` and core models only; platform SDK/API types must not leak out of platform crates.
- Real publishing is behind an explicit publish path; dry-run (validation without remote mutation) must work everywhere. Live publishing is disabled by default in dev and CI.
- Platform tests use mocked HTTP (wiremock); live smoke tests are opt-in with dedicated test accounts. CI never needs real credentials.
- Never log tokens, auth codes, API keys, or sensitive URLs; redaction is tested.
- Large media is streamed/chunked, never fully buffered.
- Multi-platform publishes record one delivery per platform and preserve partial success.
