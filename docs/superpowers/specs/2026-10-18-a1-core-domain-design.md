# A1 — Core domain models and the agent contract: design

Date: 2026-10-18. Source: plan 03 (`!ref/plans/03. publishing roadmap.md`), phase A1, plus the sections it links: Concepts, Formats and validation, Delivery states, Sending an email campaign, Recurring posts, Localization. Plan 01 holds the engineering standards and crate map. This document does not repeat the plan; it records intent, the decisions the plan left open, and what is left to the builder.

## Intent

Every later phase of plan 03 (vault, media, agents, publishing engine, drafter, clients) talks in the types and traits A1 defines. A1 fixes that vocabulary once, in `postit-core`, so agents can be built in parallel against a stable contract and the shared code never sees platform API types. Getting the contract right matters more than getting it big: anything wrong here is paid for in thirteen agent crates.

## Goals

- Everything listed under plan 03 phase A1, in `postit-core`, documented and tested as the phase lists.
- The contract is data-driven: a new platform is described by its `AgentDescriptor` (formats, capabilities, schemas, icon, color) so clients render it without code changes.
- Validation returns every violation at once, each with a stable `code`, parameters, and an optional suggested fix, so clients render messages in the user's language (D0) and offer "Fit for TikTok" instead of an error.

## Non-goals

- No platform, HTTP, or database code in core (plan exit criterion). No `reqwest`, `sqlx`, or `postit-http` dependency.
- No occurrence expansion for recurrence (E1), no email rendering (C2), no persistence (B2), no conformance suite or mock agent (B5), no vault (A2).
- No real agent.

## Anti-goals

- A closed `Platform` enum, or any core type that names a specific platform.
- A format type hierarchy. Formats are data (plan 03, "Formats and validation").
- A schema system that silently accepts JSON Schema features outside the subset: clients would then show options they cannot render.
- The word "plugin" anywhere.

## Constraints

- Rust 2024, workspace lints (`unsafe_code = "forbid"`, clippy pedantic, `unwrap_used`/`expect_used`/`panic` denied). Workspace-level dependencies only.
- Async trait objects use `async-trait` (P1 decision, `server/crates/core/src/async_trait_check.rs`). The trait-object compile test extends to every new `dyn` trait: `Agent`, `AgentClient`, `Commenter`, `InboxReader`, `MetricsReader`, `DirectSender`, `NativeScheduler`, `CredentialStore`, `MediaStore`, `MediaProbe`, `MediaHost`, `LinkSigner`.
- Prefer maintained libraries over hand-rolled code unless the dependency is too large (project rule). No dependency with an Arabic or Arabic-derived name.
- Existing ID newtypes, `Clock`, and `IdGenerator` keep their public signatures.

## Decisions

These were open in the plan and are now fixed (all the designer's calls, accepted by the user).

### Layout

`postit-core` gains modules roughly matching the plan's list: platform ID, descriptor, schema, format and validation, post (post, overrides, version, snapshot, variant resolution), email document, media, agent and capability traits, agent context, registry, receipts and account status, delivery state machine, recurrence rule, inbox and metric value types, and errors. Exact file split is the builder's.

### `PlatformId`

Validated lowercase string newtype: ASCII `[a-z][a-z0-9_]*`, bounded length (builder picks, around 32). `FromStr`/`TryFrom<String>`, `Serialize`/`Deserialize` that validates, `Display`. No enum.

### Schemas (publish options and credentials)

- An agent writes its schema as JSON in JSON Schema form (an object schema with `properties`, `required`, and per-property `type`/`format`/`enum`/`minimum`/`maximum`/`maxLength`/`default` as needed by the subset), so clients receive real JSON Schema and render it generically.
- Core parses that JSON into a typed model (`OptionsSchema` with typed fields: string, multi-line text, enum, boolean, integer, date-time) and **rejects any keyword or type outside the subset** with a descriptive error. The `x-postit-*` hints (`no_default`, `consent_text`, `depends_on`, `secret`, `help`) are typed in the model; unknown `x-postit-*` hints are rejected.
- Core validates values (`serde_json::Value` object) against the typed model and returns violations in the same shape as format violations (code + parameters + field path).
- No general JSON Schema validator crate: it would be heavy and would accept the features the subset must reject.
- Labels and help texts are `Message { key, en }` (a message key plus the English default), so the schemas stay translatable in D0 without core depending on any i18n crate. The same `Message` type is used wherever the descriptor carries human text (display names of formats or options, enum labels).

### Text measurement

`TextMeasure` variants: `Graphemes`, `XWeighted`, `Utf16Units`, `Bytes`.

- Graphemes: `unicode-segmentation` (new workspace dependency).
- X-weighted: a small core function implementing X's published weighting rules (code-point ranges weighted 1, everything else 2, URLs counted as a fixed 23, emoji sequences as 2). The Rust `twitter-text` port is unmaintained, so it is not used. The builder records the source of the ranges in a doc comment and tests against X's published conformance examples where available.
- UTF-16 and bytes from the standard library.

### Recurrence rule

A small parser of core's own for the RRULE subset in plan 03 ("Recurring posts"): `FREQ` of `DAILY`, `WEEKLY` (with `BYDAY`), or `MONTHLY` (with a single `BYMONTHDAY`); `INTERVAL`; ending with `COUNT`, `UNTIL`, or neither. Anything else is rejected with a specific error. Parsed into a typed `RecurrenceRule` that serializes back to the canonical RRULE string. The `rrule` crate is not used in A1 because it would bring the time zone database into core; E1 decides how occurrences are computed (and may convert the typed rule to `rrule` there).

### Delivery state machine

`DeliveryState` with `Parked(ParkReason)`; the states, parking reasons, and transitions are the plan 03 "Delivery states" table **plus the moves the "Rescheduling, cancelling, and retrying" section implies** (designer's call, resolving gaps between the two):

- User retry with `acknowledge_outcome_unknown` moves `OutcomeUnknown` to `Pending` (in addition to the reconcile-proved-absent move).
- `Claimed` to `Cancelled` at the worker's first safe checkpoint (no remote call has happened yet, so it is as safe as cancelling `Pending`).
- `Claimed` to `Pending` on graceful shutdown (lease released before any remote call).
- `InFlight` to `Parked(quota_exceeded)` or `Parked(account_needs_reconnect)` when the agent reports a rolling-window quota error ("Quotas") or an authentication failure before any mutating call.
- A late worker's success receipt on an `OutcomeUnknown` delivery uses the same move as reconcile finding the post.
- Rescheduling (new time, incremented `dispatch_gen`) is **not** a state transition: the delivery stays `Pending` and the engine updates its fields. `transition()` has no self-transitions.

D1 updates the plan 03 table to match when it lands. A pure `transition(from, event) -> Result<DeliveryState, TransitionError>` refuses every move the table does not list. Events are named after their causes (claim, lease expired, revalidation failed, retryable failure, user retry, user resolve, reconcile found, and so on) so the engine (D1) reads as the table. Job status derivation from delivery states (plan 03, "Job status") is a pure function in core too, since it is a rule over core types.

### Agent context and HTTP

Core must not name the `postit-http` client. The agent context therefore holds:

- the resolved agent config (`serde_json::Value`, validated against `config_schema()`),
- `Arc<dyn CredentialStore>` (replaces `TokenStore`; OAuth tokens and pasted credentials, secrets as `secrecy` types),
- `Arc<dyn MediaStore>` / `MediaHost` access,
- `Arc<dyn LinkSigner>` (the email unsubscribe/click/open URL builder from "Sending an email campaign"),
- `Arc<dyn Clock>`,
- an **opaque HTTP handle**: a core marker trait (object-safe, `Send + Sync`, with an `as_any()` accessor). `postit-http` implements it for its client (both the normal and the address-policy variant for user-supplied hosts); `postit-agent-kit` (B5) provides the typed accessor that downcasts. Core stays free of HTTP code and dependencies.

`secrecy` is added to core's dependencies for secret-bearing types.

### Other types

- `AccountRef { platform, account }`; targets are `AccountRef`s.
- `Post`, `PostOverrides`, `PostVersion`, `PublishSnapshot`, and a variant resolver (post + override for one target, including the `EmailDocument` for `html_email` targets) as listed in the plan. `Post.kind` is `Post` or `Comment` with `reply_to` required exactly for comments.
- `EmailDocument { subject, preheader, blocks[] }` with blocks heading, text, image (stored media reference), button, divider. The footer is not a block (added by the email agent).
- `MediaAsset`, `MediaSource` (`Stored`, `PublicUrl`, `RemoteRef`), traits `MediaStore`, `MediaProbe`, `MediaHost`. Large media is streamed: trait methods use streams/readers, never `Vec<u8>` for asset content. The icon in `AgentDescriptor` is the exception (`&'static [u8]` embedded PNG).
- `DirectSender::send_batch` takes `DirectRecipient { send_id, address, merge_fields, locale }` per plan 03.
- `AgentError` with the plan's variants, each exposing a retry classification (retryable, retryable after a duration, not retryable, outcome unknown). Secrets never appear in `Display`/`Debug`.
- `AgentRegistry`: `PlatformId -> Arc<dyn Agent>`, duplicate registration is an error, built by the composition root; iteration in stable order.
- Violation codes: one closed list in core with each code's parameters documented, so clients (D0) can translate every code. *Designer's call:* the list includes one generic `agent_rule` code whose parameters carry a `Message { key, en }` supplied by the agent, for platform-specific rules from an agent's own `validate` or from `Commenter::resolve_target` refusals. Agents choose keys under their platform ID (`tiktok.…`); clients translate known keys and fall back to the English default, so a new platform needs no core or client change.

### Wire format and OpenAPI

*Designer's call:* core types that go over the API (descriptors, formats, schemas, violations, posts, overrides, `EmailDocument`, delivery states, recurrence rule) derive `utoipa::ToSchema` behind an optional `openapi` cargo feature that `postit-api` enables. Mirroring the whole descriptor and violation tree as DTOs would duplicate it (DRY rule), and utoipa is schema generation, not HTTP code. Consequence: **the serde form of these types is the public wire format**, so tag and field names are chosen with care and pinned by round-trip snapshot tests. Types with a custom serde form (string newtypes, the JSON Schema form of `OptionsSchema`, the tagged `ConnectMethod`) carry hand-written `ToSchema` impls so the OpenAPI document describes what is actually sent.

### Validation details

*Designer's calls:* a native-schedule request on an agent without a native window is not a violation; that target uses postit scheduling, as plan 03 "Publishing" says. A media or email-image asset the variant references but the caller could not load (deleted before a retry) is the `media_missing` violation. `QuotaBucket` follows plan 03 "Quotas": scope (`app`, `account`), calendar period (`day`, `month`) in a reset time zone, a default limit, and per-operation costs (publish, comment, inbox poll, metrics).

### Agent entry points without HTTP types

*Designer's call:*

- `handle_event(kind, request)` takes a core `InboundEvent { kind, path_params, headers, raw_body }` (headers as name/value byte pairs, raw body bytes so signatures can be checked) and returns a core `EventResponse { status, content_type, body }`. `postit-api` converts to and from axum types.
- `verify_credentials(ctx, fields)` takes a `CredentialCheckContext` (the opaque HTTP handle, address-policy variant, and the clock; no stored credentials yet) plus the submitted fields, and returns `Discovered { accounts: Vec<DiscoveredAccount { remote_id, display_name, handle, avatar_url } > }` or an `AgentError` (`invalid_credentials` for a bad token). B3 may add optional fields.

### `OAuthProviderSpec`

*Designer's call:* fixed now from the reviewed platforms' known needs (TikTok, X, YouTube, LinkedIn, Meta), so descriptors and B7 are not reworked together: authorize URL, token URL, optional revoke URL, scopes and scope separator, PKCE (`S256` or none), client authentication (`basic`, `post_body`, `none`), extra authorize parameters, whether refresh tokens are issued and whether they rotate, and access-token lifetime when the platform does not return one. Whether one grant yields several accounts is not in the spec: `discover_accounts` answers it at connect time. The struct is `#[non_exhaustive]` so B7 can add fields without breaking agents.

## Left to the builder

- Exact module/file split, private helper structure, naming of events and error variants beyond what the plan fixes.
- Exact field names inside value types where the plan is silent, following existing core and API naming.
- Which serde representation (tagging) each enum uses, as long as it is stable and round-trip tested; prefer `snake_case` tags to match API conventions.
- Whether `proptest` (already a workspace dependency) is used for round-trip and transition tests.
- The order of work inside the phase.

## Tests (from the plan)

Serialization round trips; validation matrices per format including suggested fixes; options and credentials schema validation including rejection of out-of-subset features; override resolution including email documents; every allowed and forbidden delivery transition; RRULE subset accepted and rest rejected; registry duplicate rejection; trait-object compile test for every `dyn` trait. Quality gates: `cargo fmt --check`, clippy with `-D warnings`, `cargo check`, `cargo test --workspace`.

## Exit criteria

All shared types documented and tested. Core has no platform-specific, HTTP, or database code.
