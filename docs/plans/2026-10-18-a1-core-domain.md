# A1: Core Domain Models and the Agent Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `postit-core` with every shared type and trait plan 03 phase A1 lists (platform identity, descriptors, schemas, formats and validation, posts, email documents, media, the agent and capability contracts, registry, receipts, delivery state machine, recurrence rule, errors), documented and tested, with no platform, HTTP, or database code.

**Architecture:** Pure library work inside `backend/crates/core`. One module per concern; value types are serde-serializable and their serde form is the public wire format (pinned by JSON tests); wire types derive `utoipa::ToSchema` behind an optional `openapi` feature. Async contracts use `async-trait` (P1 decision). Core never names `postit-http`: agents receive an opaque `HttpHandle` that `postit-agent-kit` (B5) downcasts.

**Tech Stack:** Rust 2024, serde/serde_json, chrono, uuid, url, secrecy, bytes, futures, thiserror, async-trait, unicode-segmentation, unicode-normalization, utoipa (optional).

**Spec:** `docs/specs/2026-10-18-a1-core-domain-design.md`, which argues from `docs/plans/03. publishing roadmap.md` (phase A1 and the sections "Concepts", "Formats and validation", "Publishing", "Delivery states", "Rescheduling, cancelling, and retrying", "Sending an email campaign", "Recurring posts", "Inbox and replies", "Media"). Read both.

## Global Constraints

- Edition 2024, `rust-version = "1.98"`; workspace lints: `unsafe_code = "forbid"`, clippy `all` + `pedantic` warn, `unwrap_used`/`expect_used`/`panic` deny (tests may `#[allow]` these locally, as existing core tests do). `#![warn(missing_docs)]` stays: every public item has a doc comment.
- Dependencies are declared in `backend/Cargo.toml` `[workspace.dependencies]` and used with `.workspace = true`. No `reqwest`, `sqlx`, `axum`, `postit-http`, or any platform SDK in core.
- No closed platform enum and no platform names in core code (test fixtures may use `"email"`, `"telegram"`, `"tiktok"` as strings). The word "plugin" never appears; update `async_trait_check.rs`'s doc comment, which still says `PlatformClient`/`PlatformPlugin`, to `AgentClient`/`Agent`.
- Every async trait used as `dyn` uses `#[async_trait::async_trait]` and is `Send + Sync`.
- Every type that crosses the API derives `Serialize`/`Deserialize` (`Serialize` only where noted) and `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]`. Enum tags and field names are `snake_case`. Unit enums: `#[serde(rename_all = "snake_case")]`. Data enums: `#[serde(tag = "type", rename_all = "snake_case")]` unless a task says otherwise.
- The `ToSchema` output must describe the serde output. A type with a hand-written or string `Serialize` (`PlatformId`, `AspectRatio`, `RecurrenceRule`, `ConnectMethod`, `OptionsSchema`) gets a hand-written `utoipa::ToSchema` impl (strings as `type: string`; `OptionsSchema` as a free-form object; `ConnectMethod` as the `method`-tagged object with an optional `schema`), never a derive that would describe its Rust fields. Fields skipped by serde (`icon_png`) carry `#[cfg_attr(feature = "openapi", schema(ignore))]`. Task 14 tests this.
- Secrets are `secrecy::SecretString`; no `Debug`/`Display`/`Serialize` output ever contains a secret value.
- Large media content is a stream (`ByteStream`), never `Vec<u8>`; the only embedded bytes are the descriptor icon (`&'static [u8]`).
- Existing `UserId`, `AccountId`, `PostId`, `AuditEventId`, `Clock`, `SystemClock`, `TestClock`, `IdGenerator` keep their signatures.
- Quality gates after every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test -p postit-core --all-features`. The last task runs the full workspace gates.
- Commits: Conventional Commits, scope `core`, ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Schema with a keyword outside the subset** (`oneOf`, `pattern`, `$ref`, `type: array`, an unknown `x-postit-*`): parse must fail naming the keyword, never silently drop it. Task 3 tests each.
2. **Text in Arabic, with emoji ZWJ sequences, combining marks, or URLs** measured against a limit: graphemes and X weighting must match what the platform counts, or users get rejections the UI said were fine. Task 4 pins vectors.
3. **A schedule time 30 seconds in the past, or exactly at the native window edge**: must be accepted (within 1 minute / inclusive bounds), not flagged. Task 8 tests both edges.
4. **User retry on an `OutcomeUnknown` or `Parked(outcome_unresolvable)` delivery without acknowledgement**: must be refused with `AcknowledgementRequired`, never silently re-published. Task 10 tests it.
5. **RRULE with lowercase keys, trailing semicolons, `COUNT` and `UNTIL` together, or `BYDAY=1MO`**: rejected with a specific error (lowercase and trailing `;` are rejected too: canonical input only), never partially parsed. Task 11 tests each.

---

### Task 1: Dependencies, new IDs, `PlatformId`, `Message`, `openapi` feature

**Files:**
- Modify: `backend/Cargo.toml` (workspace deps), `backend/crates/core/Cargo.toml`, `backend/crates/core/src/lib.rs`, `backend/crates/core/src/ids.rs`, `backend/crates/core/src/async_trait_check.rs` (doc comment only)
- Create: `backend/crates/core/src/platform.rs`, `backend/crates/core/src/message.rs`

**Interfaces:**
- Produces: `MediaAssetId`, `PublishJobId`, `DeliveryId`, `DirectSendId`, `CampaignId` (same `id_newtype!` macro); `PlatformId` with `PlatformId::new(&str) -> Result<PlatformId, PlatformIdError>`, `as_str(&self) -> &str`, `FromStr`, `Display`, validating `Deserialize`, `Ord`; `Message { key: Cow<'static, str>, en: Cow<'static, str> }` with `const fn new_static(key: &'static str, en: &'static str) -> Message`.

- [ ] **Step 1: Add workspace dependencies** in `backend/Cargo.toml`: `bytes = "1"`, `unicode-segmentation = "1"`, `unicode-normalization = "0.1"`. Core's `Cargo.toml` gains `bytes`, `futures`, `secrecy`, `serde_json`, `thiserror`, `url`, `unicode-segmentation`, `unicode-normalization` (all `.workspace = true`) and `utoipa = { workspace = true, optional = true }` with `[features] openapi = ["dep:utoipa"]`. Add `proptest.workspace = true` to dev-dependencies.

- [ ] **Step 2: Write failing tests** in `platform.rs`:

```rust
#[test] fn accepts_lowercase_ids() { for id in ["email", "telegram", "x", "google_business"] { assert!(PlatformId::new(id).is_ok(), "{id}"); } }
#[test] fn rejects_invalid_ids() { for id in ["", "Email", "1x", "tik-tok", "tik tok", "ü", &"a".repeat(33)] { assert!(PlatformId::new(id).is_err(), "{id}"); } }
#[test] fn deserialize_validates() { assert!(serde_json::from_str::<PlatformId>("\"TikTok\"").is_err()); assert_eq!(serde_json::from_str::<PlatformId>("\"tiktok\"").unwrap().as_str(), "tiktok"); }
#[test] fn serializes_as_plain_string() { assert_eq!(serde_json::to_value(PlatformId::new("email").unwrap()).unwrap(), json!("email")); }
```

and in `message.rs`: `message_serializes_as_key_and_en` asserting `json!({"key": "tiktok.privacy.label", "en": "Who can view"})`.

- [ ] **Step 3: Run** `cargo test -p postit-core` from `backend/`. Expected: compile failure (types missing).

- [ ] **Step 4: Implement.** `PlatformId` rule: ASCII, first char `a-z`, rest `a-z0-9_`, length 1..=32. `PlatformIdError` is a `thiserror` enum (`Empty`, `TooLong`, `InvalidChar`). Re-export everything from `lib.rs`; update the crate doc line to describe the new scope. Fix the `async_trait_check.rs` doc names.

- [ ] **Step 5: Run** `cargo test -p postit-core --all-features` and clippy. Expected: PASS.

- [ ] **Step 6: Commit** `feat(core): add platform IDs, messages, and new ID newtypes`.

---

### Task 2: Violations and suggested fixes

**Files:**
- Create: `backend/crates/core/src/validation/mod.rs`, `backend/crates/core/src/validation/violation.rs`

**Interfaces:**
- Produces:
  - `enum ViolationCode` (unit, `snake_case`, `Copy`, `as_str()`, `ALL: &[ViolationCode]`), exactly: `ScheduleInPast`, `NativeScheduleOutsideWindow`, `FormatUnsupported`, `TextRequired`, `TextTooLong`, `MediaRequired`, `MediaCount`, `MediaMissing`, `MediaKindNotAllowed`, `MediaTypeNotAllowed`, `MediaTooLarge`, `MediaDimensions`, `MediaAspectRatio`, `MediaDuration`, `MediaNotReady`, `ReplyToRequired`, `ReplyToNotAllowed`, `OverrideMediaNotInPost`, `EmailDocumentRequired`, `EmailSubjectRequired`, `OptionUnknown`, `OptionRequired`, `OptionType`, `OptionNotAllowed`, `OptionTooShort`, `OptionTooLong`, `OptionOutOfRange`, `OptionConsentRequired`, `AgentRule`. Each variant's doc comment lists its parameter names and types (this list is what D0 clients translate).
  - `enum SuggestedFix { Fit { aspect: AspectRatio }, Trim { max_ms: u64 }, RemoveMedia { index: Option<usize> }, ShortenText { max: u32 } }`.
  - `struct AspectRatio { width: u32, height: u32 }` (serializes as the string `"9:16"`; `FromStr`; `as_f64()`), lives here because both fixes and formats use it.
  - `struct Violation { code: ViolationCode, path: Option<String>, params: serde_json::Map<String, Value>, fix: Option<SuggestedFix> }` with builder methods `Violation::new(code)`, `.at(path: impl Into<String>)`, `.param(key: &str, value: impl Into<Value>)`, `.fix(SuggestedFix)`, and `Violation::agent_rule(message: Message) -> Violation` (sets code `AgentRule` and params `{"message": {"key", "en"}}`).

- [ ] **Step 1: Write failing tests**: `violation_wire_format` asserting

```rust
let v = Violation::new(ViolationCode::TextTooLong).at("text").param("max", 280).param("actual", 301).param("measure", "x_weighted").fix(SuggestedFix::ShortenText { max: 280 });
assert_eq!(serde_json::to_value(&v).unwrap(), json!({"code":"text_too_long","path":"text","params":{"max":280,"actual":301,"measure":"x_weighted"},"fix":{"type":"shorten_text","max":280}}));
```

plus `agent_rule_carries_message`, `aspect_ratio_round_trips` (`"9:16"` both ways, rejects `"9x16"`, `"0:1"`), and `codes_are_unique_snake_case` (iterate `ALL`, check `as_str()` equals serde output and no duplicates).

- [ ] **Step 2: Run** `cargo test -p postit-core violation`. Expected: compile failure.
- [ ] **Step 3: Implement** the types above. `path` and `fix` skip serializing when `None`; `params` skips when empty.
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add validation violations and suggested fixes`.

---

### Task 3: Options and credentials schema subset

**Files:**
- Create: `backend/crates/core/src/schema/mod.rs`, `backend/crates/core/src/schema/parse.rs`, `backend/crates/core/src/schema/validate.rs`, `backend/crates/core/src/schema/credentials.rs`

**Interfaces:**
- Consumes: `Message`, `Violation`, `ViolationCode` (Tasks 1–2).
- Produces:
  - `struct OptionsSchema { fields: Vec<SchemaField> }` with `OptionsSchema::from_json_str(&str) -> Result<OptionsSchema, SchemaError>`, `empty() -> OptionsSchema`, `fields(&self) -> &[SchemaField]`, `validate(&self, values: &serde_json::Map<String, Value>) -> Vec<Violation>`, `parse_credentials(&self, values: serde_json::Map<String, Value>) -> Result<CredentialFields, Vec<Violation>>`, and `Serialize` (emits JSON Schema, fields in declaration order).
  - `struct SchemaField { name: String, label: Message, help: Option<Message>, required: bool, kind: FieldKind, no_default: bool, secret: bool, consent_text: Option<Message>, depends_on: Option<DependsOn> }`.
  - `enum FieldKind { String { min_length: Option<u32>, max_length: Option<u32>, default: Option<String> }, MultilineText { max_length: Option<u32>, default: Option<String> }, Enum { values: Vec<EnumValue>, default: Option<String> }, Boolean { default: Option<bool> }, Integer { minimum: Option<i64>, maximum: Option<i64>, default: Option<i64> }, DateTime }`; `struct EnumValue { value: String, label: Message }`; `struct DependsOn { field: String, equals: Value }`.
  - `struct CredentialFields` (ordered map of name to `CredentialValue`; `get_plain(&self, name) -> Option<&Value>`, `get_secret(&self, name) -> Option<&SecretString>`; `Debug` prints secret values as `"[redacted]"`); `enum CredentialValue { Plain(Value), Secret(SecretString) }`.
  - `enum SchemaError` (`thiserror`): `UnsupportedKeyword { path: String, keyword: String }`, `UnsupportedType { path: String, ty: String }`, `Invalid { path: String, reason: String }`.

**Accepted JSON (the whole subset).** Top level: `type: "object"` (required), `properties` (required, object), `required` (array of property names), `additionalProperties` (only `false`). Per property: `type` (`string` | `boolean` | `integer`), `format` (only `"date-time"`, only with `string`), `enum` (strings only, with `string`), `default`, `minLength`/`maxLength` (string), `minimum`/`maximum` (integer), and hints `x-postit-label` (required, `{key, en}`), `x-postit-help`, `x-postit-enum-labels` (array of `{key, en}` parallel to `enum`, required when `enum` is present), `x-postit-multiline` (bool, `string` without `enum`/`format`), `x-postit-no-default` (bool; forbids `default`; the field counts as required), `x-postit-secret` (bool), `x-postit-consent-text` (`{key, en}`; only on `boolean`; value must be `true`), `x-postit-depends-on` (`{field, equals}`; `field` must name an earlier property). Anything else is `UnsupportedKeyword`. Parse raw structs with `#[serde(deny_unknown_fields)]`, and deserialize `properties` with a custom map visitor into `Vec<(String, RawField)>` so declaration order is kept without enabling serde_json's `preserve_order` (which would change key order workspace-wide).

**Validation rules** (each violation `.at("options.<name>")`, params as listed in Task 2 docs): key not in schema, `OptionUnknown`; required (or `no_default`) and absent while its `depends_on` holds, `OptionRequired`; a field whose `depends_on` does not hold is ignored even if present; wrong JSON type or non-RFC 3339 date-time, `OptionType {expected}`; enum miss, `OptionNotAllowed {allowed}`; length outside bounds (counted in graphemes), `OptionTooShort {min}`/`OptionTooLong {max}`; integer outside bounds, `OptionOutOfRange {min, max}`; consent field not `true`, `OptionConsentRequired`. All violations at once, in field order.

- [ ] **Step 1: Write failing tests** in `schema/mod.rs` tests (fixtures as inline JSON strings):
  - `parses_every_field_kind` (one schema with string, multiline, enum, boolean, integer, date-time; assert `fields()[i].kind` for each, and order equals declaration order even when names are not alphabetical, e.g. `["title", "privacy", "allow_comments"]`).
  - `rejects_out_of_subset` table: `oneOf`, `pattern`, `$ref`, `"type":"array"`, `"type":"number"`, `"format":"email"`, `"additionalProperties":true`, `"x-postit-unknown":1`, `enum` without `x-postit-enum-labels`, `x-postit-no-default` with `default`, `x-postit-consent-text` on a string, `x-postit-depends-on` naming a later or unknown field. Each asserts the matching `SchemaError` variant and that `path`/`keyword` names the culprit.
  - `serializes_back_to_json_schema` (parse then `serde_json::to_string`; assert the string's property order and that `serde_json::from_str::<Value>` of it equals the input `Value`).
  - `validate_reports_all_violations` (values with an unknown key, a missing required field, a wrong type, an enum miss, too long text: assert the exact codes vector in field order, unknown keys last).
  - `depends_on_skips_hidden_fields`, `no_default_field_is_required`, `consent_must_be_true`, `date_time_must_be_rfc3339`.
  - `credentials_split_secrets` (schema with `bot_token` secret and `chat` plain; `parse_credentials` puts `bot_token` in `Secret`; `format!("{:?}", fields)` does not contain the token text).
- [ ] **Step 2: Run** `cargo test -p postit-core schema`. Expected: compile failure.
- [ ] **Step 3: Implement** parse, serialize, validate, and `parse_credentials` (validates first; returns the violations on failure).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the options and credentials schema subset`.

---

### Task 4: Text measurement

**Files:**
- Create: `backend/crates/core/src/validation/text.rs`

**Interfaces:**
- Produces: `enum TextMeasure { Graphemes, XWeighted, Utf16Units, Bytes }` (unit, wire type); `fn measure(text: &str, measure: TextMeasure) -> u32` (saturating).

**X weighting** (from X's twitter-text v3 config; cite it in the doc comment): NFC-normalize first (`unicode-normalization`); then each whitespace-delimited token starting with `http://` or `https://` counts 23; every other grapheme counts 2 if it is an emoji sequence (more than one `char` and containing any of U+200D, U+FE0F, U+20E3, U+1F1E6–U+1F1FF, U+1F3FB–U+1F3FF), otherwise the sum over its `char`s of 1 if the code point is in U+0000–U+10FF, U+2000–U+200D, U+2010–U+201F, U+2032–U+2037, else 2. Bare domains without a scheme are counted as text (documented limitation).

- [ ] **Step 1: Write failing tests** (`measures_table`), each `assert_eq!(measure(input, m), expected)`:

| input | Graphemes | XWeighted | Utf16Units | Bytes |
|---|---|---|---|---|
| `"Hello"` | 5 | 5 | 5 | 5 |
| `"مرحبا"` | 5 | 5 | 5 | 10 |
| `"你好"` | 2 | 4 | 2 | 6 |
| `"👨‍👩‍👧"` | 1 | 2 | 8 | 18 |
| `"e\u{301}"` | 1 | 1 | 2 | 3 |
| `"see https://example.com/a/very/long/path"` | 40 | 27 | 40 | 40 |
| `"🇸🇦"` | 1 | 2 | 4 | 8 |
| `"1\u{FE0F}\u{20E3}"` | 1 | 2 | 3 | 7 |

  Plus `x_weighted_280_ascii_fits` (`"a".repeat(280)` measures 280) and `arabic_with_harakat_counts_code_points` (`"مَرْحَبًا"`: Graphemes 5, XWeighted 9).
- [ ] **Step 2: Run** `cargo test -p postit-core text`. Expected: compile failure.
- [ ] **Step 3: Implement** `measure`.
- [ ] **Step 4: Run** tests. Expected: PASS. If a table value disagrees with the implementation, recheck the value against the rules above before changing either; the table is the contract.
- [ ] **Step 5: Commit** `feat(core): add text measurement`.

---

### Task 5: Media types and traits

**Files:**
- Create: `backend/crates/core/src/media.rs`

**Interfaces:**
- Consumes: `MediaAssetId`, `UserId`.
- Produces:
  - `type ByteStream = futures::stream::BoxStream<'static, std::io::Result<bytes::Bytes>>;`
  - `enum MediaKind { Image, Video }`; `enum MediaAssetState { Uploading, Ready, Failed }`.
  - `struct MediaAsset { id: MediaAssetId, owner_id: UserId, kind: MediaKind, mime: String, bytes: u64, width: Option<u32>, height: Option<u32>, duration_ms: Option<u64>, sha256: Option<String>, state: MediaAssetState, derived_from: Option<MediaAssetId> }` with `aspect(&self) -> Option<f64>` and `is_vertical(&self) -> bool`.
  - `enum MediaSource { Stored(MediaAssetId), PublicUrl(url::Url), RemoteRef(String) }`.
  - `struct ProbeInfo { width: Option<u32>, height: Option<u32>, duration_ms: Option<u64> }`.
  - `enum MediaError` (`thiserror`): `NotFound`, `NotReady`, `Io(String)`, `Unsupported(String)`.
  - Traits (async via `async-trait`, `Send + Sync`): `MediaStore { async fn open(&self, asset: &MediaAsset) -> Result<ByteStream, MediaError>; }`, `MediaProbe { async fn probe(&self, path: &std::path::Path, mime: &str) -> Result<ProbeInfo, MediaError>; }`, `MediaHost { fn public_url(&self, asset: &MediaAsset, ttl: std::time::Duration) -> Result<url::Url, MediaError>; }` (sync).

- [ ] **Step 1: Write failing tests**: `asset_round_trips` (JSON round trip of a video asset), `aspect_and_orientation` (1080×1920 is vertical with aspect 0.5625; missing dimensions give `None`/`false`), `media_source_wire_format` (`json!({"type":"public_url","url":"https://media.example/a"})`).
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add media types and storage traits`.

---

### Task 6: Formats and the agent descriptor

**Files:**
- Create: `backend/crates/core/src/format.rs`, `backend/crates/core/src/descriptor.rs`, `backend/crates/core/src/oauth.rs`

**Interfaces:**
- Consumes: `PlatformId`, `Message`, `AspectRatio`, `TextMeasure`, `MediaKind`, `OptionsSchema`.
- Produces:
  - `enum FormatId { Text, Link, Image, Carousel, Video, ShortVideo, Story, Reel, HtmlEmail }` (closed set).
  - `struct FormatSpec { id: FormatId, text: TextRules, media: MediaRules, selected_by: Option<FormatSelector> }`; `struct TextRules { measure: TextMeasure, max: u32, required: bool }`; `struct MediaRules { min_count: u32, max_count: u32, kinds: Vec<MediaKind>, mime_types: Vec<String>, max_bytes: Option<u64>, min_width: Option<u32>, max_width: Option<u32>, min_height: Option<u32>, max_height: Option<u32>, aspect_ratios: Vec<AspectRange>, min_duration_ms: Option<u64>, max_duration_ms: Option<u64>, safe_zones: Vec<SafeZone> }` (`MediaRules::none()` for text-only formats); `struct AspectRange { min: AspectRatio, max: AspectRatio, preferred: AspectRatio }`; `struct SafeZone { x: f32, y: f32, width: f32, height: f32, label: Message }` (fractions of the frame, 0..=1); `struct FormatSelector { option: String, value: String }`.
  - `enum AgentKind { Broadcast, Direct }`; `enum Capability { Commenter, InboxReader, MetricsReader, DirectSender, NativeScheduler }` (`Ord`).
  - `struct NativeScheduleWindow { min_lead_secs: u64, max_lead_secs: u64 }`.
  - `struct QuotaBucket { id: String, scope: QuotaScope, period: QuotaPeriod, default_limit: u32, reset_time_zone: String, costs: QuotaCosts }`; `enum QuotaScope { App, Account }`; `enum QuotaPeriod { Day, Month }` (calendar-aligned in `reset_time_zone`, an IANA name that core stores but does not resolve; A2/D1 check it against the time zone database); `struct QuotaCosts { publish: u32, comment: u32, inbox_poll: u32, metrics: u32 }` (0 means the operation does not count). Matches plan 03 "Quotas"; rolling windows are not buckets (the agent returns `AgentError::Quota`).
  - `enum ConnectMethod { OAuth(OAuthProviderSpec), Credentials(OptionsSchema) }` with a hand-written `Serialize` emitting `{"method":"oauth"}` or `{"method":"credentials","schema":{…}}` (provider URLs and scopes are not sent to clients).
  - `oauth.rs`: `#[non_exhaustive] pub struct OAuthProviderSpec { pub authorize_url: Url, pub token_url: Url, pub revoke_url: Option<Url>, pub scopes: Vec<String>, pub scope_separator: String, pub pkce: Pkce, pub client_auth: ClientAuth, pub extra_authorize_params: Vec<(String, String)>, pub refresh: RefreshPolicy, pub default_access_ttl: Option<std::time::Duration> }` with `OAuthProviderSpec::new(authorize_url: Url, token_url: Url) -> Self` (defaults: no revoke, no scopes, separator `" "`, `Pkce::S256`, `ClientAuth::Basic`, no extra params, `RefreshPolicy { issued: true, rotates: false }`, no default TTL) and chainable `with_revoke_url`, `with_scopes`, `with_scope_separator`, `with_pkce`, `with_client_auth`, `with_extra_authorize_param`, `with_refresh`, `with_default_access_ttl`; `enum Pkce { S256, None }`; `enum ClientAuth { Basic, PostBody, None }`; `struct RefreshPolicy { issued: bool, rotates: bool }`.
  - `struct AgentDescriptor { platform: PlatformId, display_name: String, icon_png: &'static [u8], brand_color: String, kind: AgentKind, connect: ConnectMethod, formats: Vec<FormatSpec>, capabilities: BTreeSet<Capability>, config_schema_version: u32, native_schedule: Option<NativeScheduleWindow>, quota_buckets: Vec<QuotaBucket>, options_schema: OptionsSchema }`: `Serialize` only, `icon_png` skipped (served by `GET /platforms/{id}/icon`); `validate(&self) -> Result<(), DescriptorError>`; `supports_native_schedule(&self) -> bool`.
  - `enum DescriptorError` (`thiserror`): `NoFormats`, `DuplicateFormat(FormatId)`, `InvalidBrandColor(String)`, `EmptyIcon`, `DirectWithoutCapability` (a `Direct` agent must declare `DirectSender`), `NativeScheduleWithoutCapability` (a native window requires `NativeScheduler`), `UnknownSelectorOption(String)` (a `selected_by.option` not in `options_schema`), `DuplicateQuotaBucket(String)`, `InvalidQuotaBucket(String)` (zero `default_limit`, empty `reset_time_zone`, or all costs 0).

- [ ] **Step 1: Write failing tests**: `descriptor_validate_accepts_minimal` (one `text` format, `#1A2B3C`, broadcast, credentials connect), `descriptor_validate_rejects` table (one case per `DescriptorError` variant), `connect_method_hides_oauth_details` (serialized `OAuth` equals `json!({"method":"oauth"})`), `oauth_spec_defaults` (asserts each default above), `format_spec_round_trips` (JSON round trip of a `short_video` spec with a safe zone and aspect range).
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.** `brand_color` must match `#` + 6 hex digits.
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add formats and the agent descriptor`.

---

### Task 7: Posts, overrides, email documents, variants

**Files:**
- Create: `backend/crates/core/src/post.rs`, `backend/crates/core/src/email.rs`, `backend/crates/core/src/account.rs`

**Interfaces:**
- Consumes: IDs, `PlatformId`, `Violation`.
- Produces:
  - `account.rs`: `struct AccountRef { platform: PlatformId, account: AccountId }` (`Ord`, `Hash`).
  - `email.rs`: `struct EmailDocument { subject: String, preheader: Option<String>, blocks: Vec<EmailBlock> }`; `enum EmailBlock { Heading { text: String, level: HeadingLevel }, Text { text: String }, Image { asset: MediaAssetId, alt: String, link: Option<Url> }, Button { label: String, url: Url }, Divider }`; `enum HeadingLevel { H1, H2 }`; `EmailDocument::media_ids(&self) -> Vec<MediaAssetId>`.
  - `post.rs`: `enum PostKind { Post, Comment }`; `struct ReplyTo { remote_post_id: String, remote_comment_id: Option<String>, url: Option<Url> }`; `struct MediaItem { asset: MediaAssetId, alt_text: Option<String> }`; `struct Post { kind: PostKind, text: String, media: Vec<MediaItem>, link: Option<Url>, tags: Vec<String>, disclose_ai_generated: bool, reply_to: Option<ReplyTo> }`; `struct PostOverride { text: Option<String>, link: Option<Url>, tags: Option<Vec<String>>, media: Option<Vec<MediaAssetId>>, email: Option<EmailDocument>, options: serde_json::Map<String, Value> }`; `type PostOverrides = BTreeMap<AccountId, PostOverride>`; `struct PostVersion(pub i64)` (`Copy`, `Ord`, transparent); `struct Variant { target: AccountRef, kind: PostKind, text: String, media: Vec<MediaItem>, link: Option<Url>, tags: Vec<String>, disclose_ai_generated: bool, reply_to: Option<ReplyTo>, email: Option<EmailDocument>, options: serde_json::Map<String, Value> }`; `struct PublishSnapshot { post_id: PostId, post_version: PostVersion, variants: Vec<Variant> }` with `variant_for(&self, account: AccountId) -> Option<&Variant>`.
  - `fn resolve_variant(post: &Post, target: &AccountRef, override_: Option<&PostOverride>) -> Result<Variant, Vec<Violation>>`: override fields replace post fields when `Some`; `media` override is a subset selection that keeps each item's alt text from the post and preserves the override's order; an override media ID not in the post is `OverrideMediaNotInPost` at `overrides.<account>.media[i]`; `kind`/`reply_to`/`disclose_ai_generated` come from the post only.
  - `Post::kind_violations(&self) -> Vec<Violation>`: `Comment` without `reply_to` is `ReplyToRequired`; `Post` with `reply_to` is `ReplyToNotAllowed`.

- [ ] **Step 1: Write failing tests**: `resolve_without_override_copies_post`, `override_replaces_fields`, `media_subset_keeps_alt_text_and_order`, `media_not_in_post_is_violation`, `email_override_carries_document`, `comment_needs_reply_to`, `post_rejects_reply_to`, `email_block_wire_format` (`json!({"type":"button","label":"Shop","url":"https://shop.example/"})`), `snapshot_round_trips`, `snapshot_variant_for`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add posts, overrides, email documents, and variants`.

---

### Task 8: Validation engine

**Files:**
- Create: `backend/crates/core/src/validation/engine.rs`, `backend/crates/core/src/validation/format_pick.rs`

**Interfaces:**
- Consumes: Tasks 2–7.
- Produces:
  - `enum Schedule { Now, At { at: DateTime<Utc>, native: bool } }`.
  - `struct ValidationInput<'a> { descriptor: &'a AgentDescriptor, options_schema: &'a OptionsSchema, variant: &'a Variant, media: &'a [MediaAsset], schedule: Schedule, now: DateTime<Utc> }` (`media` is the variant's media resolved in order; `options_schema` is the account-specific schema from `publish_options` when the caller has it, else the descriptor's).
  - `fn validate_target(input: &ValidationInput<'_>) -> Vec<Violation>`: the pure part of the pipeline. The publishing engine (D1) appends the agent's own `AgentClient::validate` result; this function does not call agents.
  - `fn pick_format<'a>(descriptor: &'a AgentDescriptor, variant: &Variant, media: &[MediaAsset]) -> Result<&'a FormatSpec, Violation>`.

**Pipeline order** (all violations collected): (1) `Post::kind_violations`-equivalent on the variant; (2) schedule: `At { at }` earlier than `now - 60s` is `ScheduleInPast` (60 s exactly is accepted); `native: true` on a descriptor without `native_schedule` is not a violation: that target uses postit scheduling (plan 03, "Publishing" step 1), so the window check is skipped; for a native target, the lead `at - now` outside `[min_lead_secs, max_lead_secs]` inclusive is `NativeScheduleOutsideWindow {min_lead_secs, max_lead_secs}`; (3) format pick; on failure skip step 4; (4) format rules: text (`TextRequired`, `TextTooLong {measure, max, actual}` with fix `ShortenText`), media lookup (`media` is matched to `variant.media` and to `variant.email`'s `media_ids()` by asset ID; an ID with no asset in the slice, such as one deleted before a retry, is `MediaMissing {asset_id}` at `media[i]` or `email.blocks[i]`, and that item is skipped in the per-item checks), media count (`MediaRequired` when 0 < min, `MediaCount {min, max, actual}` with fix `RemoveMedia { index: None }` when too many), per item `MediaNotReady`, `MediaKindNotAllowed {kind}` (fix `RemoveMedia { index: Some(i) }`), `MediaTypeNotAllowed {mime}`, `MediaTooLarge {max_bytes, actual}`, `MediaDimensions {min_width, max_width, min_height, max_height, actual_width, actual_height}` (absent bounds omitted), `MediaAspectRatio {allowed, actual}` with fix `Fit { aspect: first range's preferred }`, `MediaDuration {min_ms, max_ms, actual}` with fix `Trim { max_ms }` when too long; `HtmlEmail` without `variant.email` is `EmailDocumentRequired`, with an empty subject `EmailSubjectRequired`; (5) `options_schema.validate(&variant.options)`.

**Format pick:** if any declared format has `selected_by` whose option value matches `variant.options[option]`, use it. Otherwise build the candidate list from the content and take the first declared one: `variant.email.is_some()` gives `[HtmlEmail]`; no media gives `[Text, Link]` (`[Link, Text]` when `variant.link` is set); one image gives `[Image, Text, Carousel]`; several images give `[Carousel, Text]`; one vertical video whose duration fits a declared `ShortVideo`'s `max_duration_ms` gives `[ShortVideo, Reel, Video, Text]`; another single video gives `[Video, Text]`; mixed or several videos give `[Carousel]`. A candidate `Text` only qualifies with media if its `MediaRules` allow that count and kind. No candidate is `FormatUnsupported {inferred}` where `inferred` is the first candidate's id.

- [ ] **Step 1: Write failing tests** using a fixture descriptor builder in the test module (formats `text` (graphemes 500, images 0..=4), `short_video` (9:16 preferred, 0.5..=0.6 range, ≤ 60 000 ms), `image`): `pick_text_without_media`, `pick_short_video_for_vertical_clip`, `pick_falls_back_to_text_with_images`, `selector_wins_over_inference`, `unsupported_format_reports_inferred`, `collects_every_violation_at_once` (too long text + wrong aspect + bad option: three codes), `aspect_violation_suggests_fit` (`Fit { aspect: "9:16" }`), `long_video_suggests_trim`, `schedule_59s_past_ok_61s_past_rejected`, `native_window_edges_inclusive`, `native_on_unsupported_agent_uses_postit_scheduling` (no violation even with a lead outside any window), `deleted_media_is_missing` (variant references two assets, slice holds one: one `MediaMissing` at `media[1]`, no panic, other checks still run), `missing_email_image_is_missing`, `html_email_requires_document_and_subject`, `comment_without_reply_to`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the validation engine`.

---

### Task 9: Agent errors, receipts, and value types

**Files:**
- Create: `backend/crates/core/src/error.rs`, `backend/crates/core/src/receipt.rs`, `backend/crates/core/src/inbox.rs`, `backend/crates/core/src/metrics.rs`

**Interfaces:**
- Produces:
  - `enum AgentError` (`thiserror`, `Debug` never includes secrets): `Authentication { message: String }`, `Authorization { missing_scopes: Vec<String> }`, `InvalidCredentials { field: Option<String> }`, `Validation(Vec<Violation>)`, `RateLimited { retry_after: Option<Duration> }`, `Quota { bucket: String, resets_at: Option<DateTime<Utc>> }`, `Transport { message: String }`, `Unsupported { capability: Capability }` (also used for unsupported optional methods; `Capability` is reused), `Remote { status: Option<u16>, code: Option<String>, message: String }`, `OutcomeUnknown { message: String }`; method `retry_class(&self) -> RetryClass`.
  - `enum RetryClass { Retryable, RetryAfter(Duration), Permanent, NeedsReconnect, QuotaExceeded, Ambiguous }`. Mapping: `Authentication` is `NeedsReconnect`; `Authorization`, `InvalidCredentials`, `Validation`, `Unsupported` are `Permanent`; `RateLimited` is `RetryAfter(d)` or `Retryable` without a duration; `Quota` is `QuotaExceeded`; `Transport` is `Retryable`; `Remote` with `status >= 500` or `None` is `Retryable`, `429` is `Retryable`, other 4xx `Permanent`; `OutcomeUnknown` is `Ambiguous`.
  - `receipt.rs`: `struct PublishReceipt { remote_id: Option<String>, remote_url: Option<Url>, container_ref: Option<String>, remote_publish_at: Option<DateTime<Utc>> }`; `enum PublishOutcome { Published(PublishReceipt), Processing(PublishReceipt), Simulated { planned: Vec<String> } }`; `enum ReconcileOutcome { Found(PublishReceipt), Absent, Unresolvable }`; `enum AccountStatus { Active, NeedsReconnect, Revoked }`; `struct AccountInfo { remote_id: String, display_name: String, handle: Option<String>, avatar_url: Option<Url>, status: AccountStatus }`; `struct DiscoveredAccount { remote_id: String, display_name: String, handle: Option<String>, avatar_url: Option<Url> }`; `struct Discovered { accounts: Vec<DiscoveredAccount> }`.
  - `inbox.rs`: `struct InboxItem { remote_comment_id: String, remote_post_id: String, parent_remote_id: Option<String>, author_handle: String, author_display_name: Option<String>, text: String, created_at: DateTime<Utc> }`; `struct InboxPage { items: Vec<InboxItem>, cursor: Option<String> }`.
  - `metrics.rs`: `struct MetricSnapshot { views, likes, comments, shares, saves, clicks: Option<u64>, collected_at: DateTime<Utc> }`; `struct AccountMetrics { followers: Option<u64>, reach: Option<u64> }`.

- [ ] **Step 1: Write failing tests**: `retry_classification_table` (one assertion per mapping above, including `Remote` with 500, 429, 403, and `None`), `agent_error_display_has_no_secret_fields` (construct each variant; `to_string()` is non-empty), `outcome_wire_format` (`json!({"type":"processing","remote_id":"v1","remote_url":null,"container_ref":"c1","remote_publish_at":null})`; use `#[serde(tag = "type")]` with the receipt flattened), `value_types_round_trip` for `InboxItem`, `MetricSnapshot`, `AccountInfo`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.** `Duration` fields serialize as whole seconds where serialized (`AgentError` is not serialized).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add agent errors, receipts, and value types`.

---

### Task 10: Delivery state machine and job status

**Files:**
- Create: `backend/crates/core/src/delivery.rs`

**Interfaces:**
- Produces:
  - `enum ParkReason { OwnerDisabled, AccountNeedsReconnect, PlatformUnavailable, PlatformDisabled, LivePublishDisabled, QuotaExceeded, OutcomeUnresolvable }`.
  - `enum DeliveryState { Pending, Claimed, InFlight, Processing, OutcomeUnknown, Parked(ParkReason), Published, Failed, DeadLetter, Cancelled, Simulated }` with `#[serde(tag = "state", content = "reason", rename_all = "snake_case")]` (`{"state":"pending"}`, `{"state":"parked","reason":"quota_exceeded"}`), `is_terminal(&self) -> bool`, `as_str(&self) -> &'static str`.
  - `enum ResolvedOutcome { Published, Failed }`.
  - `enum DeliveryEvent { Claim, Cancel, StartRemote, Park(ParkReason), RevalidationFailed, LeaseExpired, Published, Accepted, RetryableFailure, Shutdown, RetryBudgetExhausted, PermanentFailure, Ambiguous, Simulated, ProcessingTimeout, ReconcileFound, ReconcileAbsent, ReconcileUnresolvable, Unpark, UserRetry { acknowledged_outcome_unknown: bool }, UserResolve(ResolvedOutcome) }`.
  - `enum TransitionError { NotAllowed { from: DeliveryState, event: DeliveryEvent }, AcknowledgementRequired }` (`thiserror`).
  - `fn transition(from: DeliveryState, event: DeliveryEvent) -> Result<DeliveryState, TransitionError>`.
  - `enum JobStatus { Scheduled, Publishing, Published, Cancelled, Failed, Partial }`; `struct DeliveryView { state: DeliveryState, next_attempt_at: DateTime<Utc>, awaiting_native_publish: bool }`; `fn derive_job_status(deliveries: &[DeliveryView], now: DateTime<Utc>) -> JobStatus`.

**Allowed transitions** (everything else is `NotAllowed`; no self-transitions):

| From | Event | To |
|---|---|---|
| Pending | Claim | Claimed |
| Pending | Cancel | Cancelled |
| Claimed | StartRemote | InFlight |
| Claimed | Park(r), r ≠ OutcomeUnresolvable | Parked(r) |
| Claimed | RevalidationFailed | Failed |
| Claimed | LeaseExpired | Pending |
| Claimed | Cancel | Cancelled |
| Claimed | Shutdown | Pending |
| InFlight | Published | Published |
| InFlight | Park(QuotaExceeded), Park(AccountNeedsReconnect) | Parked(r) |
| InFlight | Accepted | Processing |
| InFlight | RetryableFailure, Shutdown | Pending |
| InFlight | RetryBudgetExhausted | DeadLetter |
| InFlight | PermanentFailure | Failed |
| InFlight | Ambiguous, LeaseExpired | OutcomeUnknown |
| InFlight | Cancel | Cancelled |
| InFlight | Simulated | Simulated |
| Processing | Published | Published |
| Processing | PermanentFailure | Failed |
| Processing | ProcessingTimeout | OutcomeUnknown |
| Processing | Cancel | Cancelled |
| OutcomeUnknown | ReconcileFound | Published |
| OutcomeUnknown | ReconcileAbsent | Pending |
| OutcomeUnknown | ReconcileUnresolvable | Parked(OutcomeUnresolvable) |
| OutcomeUnknown | UserRetry { true } | Pending |
| OutcomeUnknown | UserResolve(o) | o |
| Parked(r), r ≠ OutcomeUnresolvable | Unpark | Pending |
| Parked(r), r ≠ OutcomeUnresolvable | UserRetry { _ } | Pending |
| Parked(OutcomeUnresolvable) | UserRetry { true } | Pending |
| Parked(OutcomeUnresolvable) | UserResolve(o) | o |
| Parked(_) | Cancel | Cancelled |
| DeadLetter | UserRetry { _ } | Pending |
| DeadLetter | UserResolve(o) | o |
| Failed | UserRetry { _ } | Pending |

`UserRetry { acknowledged_outcome_unknown: false }` from `OutcomeUnknown` or `Parked(OutcomeUnresolvable)` returns `AcknowledgementRequired` (not `NotAllowed`).

Rows beyond the plan 03 table (designer's calls, recorded in the spec; D1 updates the plan 03 table): `Claimed` + `Shutdown` releases a lease before any remote call; `InFlight` + `Park` covers an agent returning `AgentError::Quota` for a rolling window (plan 03 "Quotas") or `AgentError::Authentication` (`RetryClass::NeedsReconnect`) before any mutating call. `ReconcileFound` also covers a late worker reporting its receipt (plan 03 "Fenced writes"); document this on the event.

**Job status:** if every delivery is terminal: all `Published`/`Simulated` is `Published`; all `Cancelled` is `Cancelled`; none `Published`/`Simulated` is `Failed`; otherwise `Partial`. Else `Scheduled` when every non-terminal delivery is `Pending` with `next_attempt_at > now` or `Processing` with `awaiting_native_publish`; otherwise `Publishing`. An empty slice is `Failed` (documented: a job always has deliveries).

- [ ] **Step 1: Write failing tests**: `allowed_transitions` (iterate the table above as data; assert each), `everything_else_is_refused` (for every `DeliveryState` × `DeliveryEvent` combination over all `ParkReason`s and both `UserRetry` flags and both `UserResolve` outcomes, assert that any pair not in the table returns `Err`), `retry_without_ack_requires_acknowledgement` (both states), `parked_outcome_unresolvable_never_unparks`, `state_wire_format` (the two JSON shapes above), `terminal_states` (exactly Published, Failed, DeadLetter, Cancelled, Simulated), `job_status_table` (one case per rule, including partial and native-scheduled-scheduled).
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement** `transition` as one `match (from, event)`.
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the delivery state machine and job status`.

---

### Task 11: Recurrence rule (RRULE subset)

**Files:**
- Create: `backend/crates/core/src/recurrence.rs`

**Interfaces:**
- Produces: `struct RecurrenceRule { frequency: Frequency, interval: u16, end: RecurrenceEnd }`; `enum Frequency { Daily, Weekly { days: BTreeSet<Weekday> }, Monthly { day: u8 } }`; `enum Weekday { Mo, Tu, We, Th, Fr, Sa, Su }` (`Ord` in that order); `enum RecurrenceEnd { Never, Count(NonZeroU32), Until(DateTime<Utc>) }`; `FromStr` (error `RecurrenceError`), `Display` (canonical string), `Serialize`/`Deserialize` as the canonical string (wire type with `value_type = String` for utoipa); `enum RecurrenceError` (`thiserror`): `Empty`, `Malformed(String)`, `DuplicateKey(String)`, `UnsupportedKey(String)`, `UnsupportedFrequency(String)`, `MissingKey(&'static str)`, `InvalidValue { key: &'static str, value: String }`, `CountAndUntil`, `NotAllowedFor { key: &'static str, frequency: &'static str }`.

**Accepted grammar:** uppercase `KEY=VALUE` pairs joined by `;`, no leading `RRULE:`, no empty segments. Keys: `FREQ` (required; `DAILY`, `WEEKLY`, `MONTHLY`), `INTERVAL` (1..=365, default 1), `BYDAY` (`WEEKLY` only and required there; comma list of `MO`..`SU`, no ordinals, no duplicates), `BYMONTHDAY` (`MONTHLY` only and required there; one value 1..=31), `COUNT` (1..=1000), `UNTIL` (`YYYYMMDDTHHMMSSZ`, UTC only). Canonical output: `FREQ`, then `INTERVAL` only when > 1, then `BYDAY` in Monday-first order or `BYMONTHDAY`, then `COUNT` or `UNTIL`.

- [ ] **Step 1: Write failing tests**: `accepts_subset` table (`"FREQ=DAILY"`, `"FREQ=WEEKLY;BYDAY=TU,MO"` canonicalizes to `"FREQ=WEEKLY;BYDAY=MO,TU"`, `"FREQ=MONTHLY;BYMONTHDAY=31;COUNT=12"`, `"FREQ=DAILY;INTERVAL=2;UNTIL=20271231T235959Z"`, `"FREQ=WEEKLY;INTERVAL=1;BYDAY=FR"` canonicalizes without `INTERVAL`); `rejects_outside_subset` table asserting the error variant: `""`, `"freq=daily"`, `"FREQ=DAILY;"`, `"RRULE:FREQ=DAILY"`, `"FREQ=YEARLY"`, `"FREQ=HOURLY"`, `"FREQ=WEEKLY"` (missing BYDAY), `"FREQ=WEEKLY;BYDAY=1MO"`, `"FREQ=WEEKLY;BYDAY=MO,MO"`, `"FREQ=DAILY;BYDAY=MO"`, `"FREQ=MONTHLY;BYMONTHDAY=-1"`, `"FREQ=MONTHLY;BYMONTHDAY=1,15"`, `"FREQ=DAILY;COUNT=3;UNTIL=20271231T000000Z"`, `"FREQ=DAILY;UNTIL=20271231"`, `"FREQ=DAILY;INTERVAL=0"`, `"FREQ=DAILY;BYHOUR=9"`, `"FREQ=DAILY;WKST=MO"`, `"FREQ=DAILY;FREQ=WEEKLY"`; `serde_uses_canonical_string`; a `proptest` that any generated valid `RecurrenceRule` round-trips through `Display`/`FromStr`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement** the parser (split, key dispatch, then cross-key checks).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the recurrence rule subset`.

---

### Task 12: Agent context and contracts

**Files:**
- Create: `backend/crates/core/src/agent/mod.rs`, `backend/crates/core/src/agent/context.rs`, `backend/crates/core/src/agent/client.rs`, `backend/crates/core/src/agent/capability.rs`, `backend/crates/core/src/agent/event.rs`
- Modify: `backend/crates/core/src/async_trait_check.rs` (add the compile test for every new `dyn` trait)

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `context.rs`: `trait HttpHandle: Send + Sync + 'static { fn as_any(&self) -> &dyn std::any::Any; }` (doc: implemented by `postit-http`, downcast by `postit-agent-kit`); `struct HttpHandles { pub default: Arc<dyn HttpHandle>, pub user_hosts: Arc<dyn HttpHandle> }` (`user_hosts` applies the outbound address policy for user-supplied hosts); `struct OAuthTokens { access_token: SecretString, refresh_token: Option<SecretString>, expires_at: Option<DateTime<Utc>>, scopes: Vec<String> }` (manual redacting `Debug`); `enum Credentials { OAuth(OAuthTokens), Fields(CredentialFields) }`; `#[async_trait] trait CredentialStore: Send + Sync { async fn credentials(&self) -> Result<Credentials, AgentError>; async fn store_refreshed(&self, tokens: OAuthTokens) -> Result<(), AgentError>; }`; `trait LinkSigner: Send + Sync { fn unsubscribe_url(&self, recipient: &DirectRecipient) -> Url; fn click_url(&self, send_id: DirectSendId, destination: &Url) -> Url; fn open_pixel_url(&self, send_id: DirectSendId) -> Url; }`; `struct AgentContext { pub config: serde_json::Value, pub http: HttpHandles, pub credentials: Arc<dyn CredentialStore>, pub media_store: Arc<dyn MediaStore>, pub media_host: Arc<dyn MediaHost>, pub link_signer: Arc<dyn LinkSigner>, pub clock: Arc<dyn Clock> }` (one context per grant); `struct CredentialCheckContext { pub config: serde_json::Value, pub http: HttpHandles, pub clock: Arc<dyn Clock> }`; `struct EventContext { pub config: serde_json::Value, pub clock: Arc<dyn Clock> }`.
  - `event.rs`: `struct InboundEvent { pub kind: String, pub path_params: BTreeMap<String, String>, pub headers: Vec<(String, Vec<u8>)>, pub raw_body: Bytes }` with `header(&self, name: &str) -> Option<&[u8]>` (case-insensitive); `struct EventResponse { pub status: u16, pub content_type: Option<String>, pub body: Bytes }` with `EventResponse::empty(status: u16)`.
  - `client.rs`: `enum PublishMode { Live, DryRun }`; `struct PublishRequest<'a> { pub variant: &'a Variant, pub mode: PublishMode, pub native_publish_at: Option<DateTime<Utc>>, pub container_ref: Option<&'a str>, pub idempotency_key: DeliveryId }`; `#[async_trait] trait PublishProgress: Send + Sync { async fn save_container_ref(&self, container_ref: &str) -> Result<(), AgentError>; fn cancel_requested(&self) -> bool; }` (doc: agents check `cancel_requested` only at safe checkpoints: before the first mutating call and between upload chunks); `struct ReconcileRequest<'a> { pub variant: &'a Variant, pub container_ref: Option<&'a str>, pub attempted_at: DateTime<Utc>, pub idempotency_key: DeliveryId }`; and the traits:

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn descriptor(&self) -> &AgentDescriptor;
    fn config_schema(&self) -> Option<&OptionsSchema> { None }
    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError>;
    async fn handle_event(&self, ctx: &EventContext, event: InboundEvent) -> Result<EventResponse, AgentError>; // default: Unsupported
    async fn verify_credentials(&self, ctx: &CredentialCheckContext, fields: &CredentialFields) -> Result<Discovered, AgentError>; // default: Unsupported
}

#[async_trait]
pub trait AgentClient: Send + Sync {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError>;
    async fn discover_accounts(&self) -> Result<Discovered, AgentError>;
    async fn publish_options(&self, account: &AccountRef) -> Result<OptionsSchema, AgentError>;
    async fn validate(&self, variant: &Variant, media: &[MediaAsset]) -> Result<Vec<Violation>, AgentError>;
    async fn publish(&self, request: PublishRequest<'_>, progress: &dyn PublishProgress) -> Result<PublishOutcome, AgentError>;
    async fn status(&self, account: &AccountRef, receipt: &PublishReceipt) -> Result<PublishOutcome, AgentError>;
    async fn reconcile(&self, request: ReconcileRequest<'_>) -> Result<ReconcileOutcome, AgentError>;
    async fn revoke(&self) -> Result<(), AgentError>; // default: Unsupported
    fn commenter(&self) -> Option<&dyn Commenter> { None }
    fn inbox(&self) -> Option<&dyn InboxReader> { None }
    fn metrics(&self) -> Option<&dyn MetricsReader> { None }
    fn direct(&self) -> Option<&dyn DirectSender> { None }
    fn native_scheduler(&self) -> Option<&dyn NativeScheduler> { None }
}
```

  (The `Unsupported` defaults return `AgentError::Unsupported` with the closest `Capability`; for `handle_event`, `verify_credentials`, and `revoke` add a doc note that `Unsupported` there means "not offered", and B5's conformance suite treats it so.)
  - `capability.rs`: `struct RemoteTarget { pub reply_to: ReplyTo }`; `#[async_trait] trait Commenter: Send + Sync { async fn resolve_target(&self, account: &AccountRef, url: &Url) -> Result<RemoteTarget, AgentError>; async fn comment(&self, request: PublishRequest<'_>, progress: &dyn PublishProgress) -> Result<PublishOutcome, AgentError>; async fn reconcile_comment(&self, request: ReconcileRequest<'_>) -> Result<ReconcileOutcome, AgentError>; }` (refused targets are `AgentError::Validation(vec![Violation::agent_rule(..)])`); `InboxReader { async fn fetch_since(&self, account: &AccountRef, cursor: Option<&str>) -> Result<InboxPage, AgentError>; }`; `MetricsReader { async fn post_metrics(&self, account: &AccountRef, receipt: &PublishReceipt) -> Result<MetricSnapshot, AgentError>; async fn account_metrics(&self, account: &AccountRef) -> Result<AccountMetrics, AgentError>; }`; `struct DirectRecipient { pub send_id: DirectSendId, pub address: String, pub merge_fields: BTreeMap<String, String>, pub locale: String }`; `enum DirectSendOutcome { Sent { remote_id: Option<String> }, Bounced { permanent: bool, reason: String }, Failed { retryable: bool, reason: String } }`; `struct DirectSendResult { pub send_id: DirectSendId, pub outcome: DirectSendOutcome }`; `DirectSender { async fn send_batch(&self, account: &AccountRef, recipients: &[DirectRecipient], variant: &Variant) -> Result<Vec<DirectSendResult>, AgentError>; }`; `NativeScheduler { async fn reschedule(&self, account: &AccountRef, receipt: &PublishReceipt, at: DateTime<Utc>) -> Result<PublishReceipt, AgentError>; async fn delete(&self, account: &AccountRef, receipt: &PublishReceipt) -> Result<(), AgentError>; }`.
  - `fn exposed_capabilities(client: &dyn AgentClient) -> BTreeSet<Capability>` (from the five accessors; B5's conformance suite compares it to `descriptor().capabilities`).

- [ ] **Step 1: Write failing tests** in `agent/mod.rs` with a `FakeAgent`/`FakeClient` (client exposes only `commenter`): `fake_agent_is_dyn_compatible` (build `Box<dyn Agent>`, call `descriptor()`, `create_client` with a context of fakes, `publish` in `DryRun`, assert `Simulated`), `default_optional_methods_are_unsupported` (`handle_event`, `verify_credentials`, `revoke` return `AgentError::Unsupported`), `exposed_capabilities_reports_accessors` (`{Commenter}`), `inbound_event_header_lookup_is_case_insensitive`, `oauth_tokens_debug_redacts`. In `async_trait_check.rs`, add one compile-only function per trait taking `&dyn Trait` for `Agent`, `AgentClient`, `Commenter`, `InboxReader`, `MetricsReader`, `DirectSender`, `NativeScheduler`, `CredentialStore`, `MediaStore`, `MediaProbe`, `MediaHost`, `LinkSigner`, `PublishProgress`, `HttpHandle`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement.** Test fakes live in the test module only (B5 builds the reusable mock agent).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the agent contract and capability traits`.

---

### Task 13: Agent registry

**Files:**
- Create: `backend/crates/core/src/agent/registry.rs`

**Interfaces:**
- Consumes: `Agent`, `AgentDescriptor::validate`.
- Produces: `struct AgentRegistry` with `new() -> Self`, `register(&mut self, agent: Arc<dyn Agent>) -> Result<(), RegistryError>`, `get(&self, id: &PlatformId) -> Option<&Arc<dyn Agent>>`, `iter(&self) -> impl Iterator<Item = &Arc<dyn Agent>>` (ordered by `PlatformId`), `len`, `is_empty`; `enum RegistryError` (`thiserror`): `Duplicate(PlatformId)`, `InvalidDescriptor { platform: PlatformId, source: DescriptorError }`.

- [ ] **Step 1: Write failing tests**: `registers_and_looks_up`, `rejects_duplicate_platform`, `rejects_invalid_descriptor`, `iterates_in_platform_order` (register `telegram` then `email`; iteration yields `email`, `telegram`). Reuse Task 12's test fakes by moving them to a `#[cfg(test)] pub(crate) mod test_support` in `agent/mod.rs`.
- [ ] **Step 2: Run.** Expected: compile failure.
- [ ] **Step 3: Implement** (`BTreeMap<PlatformId, Arc<dyn Agent>>`).
- [ ] **Step 4: Run** tests and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add the agent registry`.

---

### Task 14: Wire-format pins, `openapi` feature check, full gates

**Files:**
- Create: `backend/crates/core/tests/wire_format.rs`
- Modify: `backend/crates/core/src/lib.rs` (crate docs: one paragraph per module group pointing to plan 03)

**Interfaces:**
- Consumes: the public API of Tasks 1–13 only (integration test).

- [ ] **Step 1: Write the pin tests**: one `#[test]` per wire type not already pinned in its module (`AgentDescriptor` serialized with a fixture: assert the full `json!` including `"connect":{"method":"credentials","schema":{…}}` and no `icon_png` key; `Variant`; `PublishSnapshot`; `FormatSpec`; `RecurrenceRule` as `"FREQ=WEEKLY;BYDAY=MO"`; `DeliveryState::Parked(QuotaExceeded)`). These tests are the contract D3 and the clients will rely on: a failing pin means a breaking API change.
  Add `#[cfg(feature = "openapi")] fn schemas_match_wire_format()`: collect the schemas of `AgentDescriptor` and its dependencies with `utoipa::ToSchema::schemas` and assert `AspectRatio`, `PlatformId`, and `RecurrenceRule` are `type: string`, `ConnectMethod` has a required `method` property, `OptionsSchema` is an object with no fixed properties, and `AgentDescriptor` has no `icon_png` property.
- [ ] **Step 2: Run** `cargo test -p postit-core --all-features`. Expected: PASS (if a pin fails, fix the type's serde attributes to match the Global Constraints, not the pin).
- [ ] **Step 3: Verify the feature builds both ways**: `cargo check -p postit-core` and `cargo check -p postit-core --features openapi`. Expected: both succeed. Then `cargo tree -p postit-core -e normal` and confirm no `reqwest`, `sqlx`, `axum`, `hyper`, or `postit-*` crate appears.
- [ ] **Step 4: Run the full workspace gates** from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo check --workspace --all-targets`, `cargo test --workspace`. Expected: all pass. `cargo doc -p postit-core --no-deps` emits no warnings.
- [ ] **Step 5: Commit** `test(core): pin the wire format of shared types`.
