# D2 Drafter: design

Date: 2026-11-06. Source: plan 03 (`docs/contracts/03. publishing roadmap.md`), phase D2, plus "Product guidelines", "Brand profile and writing style", "Posts, variants, and kinds", "Recurring posts", "Inbox and replies", the REST surface rows marked D2, and the `[ai]` config keys from A2. Plan 01 covers the capability table, the owner-only list, the data model of `ai_providers` and `ai_generations`, and their retention. The user chose the phase and asked for the design to follow the roadmap as written, without a question round. Every decision the roadmap leaves open is marked **(call)**: it is the designer's, not the user's, and is open to review.

## Intent

D2 gives postit its AI drafter: a user brings their own key for an AI service, and postit writes post text for each chosen account in that account's voice and limits, a reply to a comment, or a fresh rewrite of a recurring post. The drafter only drafts. It never creates a post, never publishes, and never touches publishing credentials. A person reviews every draft (product guideline "AI drafts, people approve").

D2 comes before D3 in the build order (C3, D2, D3) because D3's REST surface and the D4 and D6 composers build on it, and E1 (rewrite each time) and E2 (draft reply) call it.

## Goals

- Everything under plan 03 phase D2, meeting its exit criteria: the drafter produces validated, disclosure-flagged variants for any configured provider in every mode, without touching publishing credentials.
- A new crate `postit-drafter` at `backend/crates/drafter/`, behind the server's `ai` Cargo feature (on by default).
- The `LlmProvider` trait and two protocols: `openai-compatible` and native `anthropic`.
- The preset catalog (seven cloud services, four local servers, plus custom), with base URLs and model listing verified at phase start (below).
- Opencode-shaped provider rows with the key sealed.
- The three modes `post`, `reply`, and `rewrite`, with prompts built from the brand profile, the account's writing style, the target's formats and limits, the language, and the user's prompt.
- Media as context: images, and frames plus duration for a video, through a core `FrameExtractor` that `postit-media` implements.
- Structured output, core validation per target, and one repair attempt.
- Core validation that counts the link and tags for platforms that publish them inside the text (`TextRules.includes_link_and_tags`), so a draft the drafter calls valid is one the engine calls valid too.
- The D2 routes: `/ai/providers` CRUD, `POST /ai/providers/{id}/test`, `GET /ai/providers/{id}/models`, `POST /ai/generate`, `GET /ai/generations`, and `GET /ai/presets` **(call: added so clients can render the preset list; it is not in the roadmap's table)**.
- The AI audit events (`ai_provider_created`, `ai_provider_updated`, `ai_provider_deleted`, `ai_provider_key_changed`).
- `backend/openapi.json`, the TypeScript types, and the Dart client regenerated.

## Non-goals

- Client screens: the AI providers screen, the composer's AI panel, the "get your key" guide texts and their Arabic (D4, D6). D2 serves the data those screens need.
- Streaming tokens to the client (plan 03: SSE is a non-goal). A generate call answers once.
- Image, video, or audio generation (plan 01 non-goal). The output is always text.
- Deployment-level AI providers (plan 01 non-goal).
- Saving a draft as a post. That is D3's post create; D2 offers the hook that links a generation to the post it became.
- The recurrence tick that calls `rewrite` (E1) and the inbox that feeds `reply` (E2). D2 builds both modes and their service calls and tests them with seeded rows; E1 and E2 wire them in.
- Tool calling, function calling, conversation memory across requests, and fine-tuning.
- Reading opencode's own files or environment. postit stores the shape only.
- Choosing a model for the user. The user picks a default per provider.

## Anti-goals

- A key, a header value that carries a secret, or a full provider request in a log line, span, error, `Debug` output, audit detail, generation record, or draft.
- A delegate seeing a key, a header value, or a provider's config. Delegates see names and models only.
- A draft that publishes, or a post created without the user saving it.
- Any provider request to a private or loopback host while `ai.allow_private_endpoints` is off, redirects included.
- A prompt stored when the owner has not turned on `store_ai_prompts`.
- Silently dropping a target. Every requested target comes back, with its draft or with what went wrong.
- Comment text from strangers (reply mode) treated as instructions. It is quoted data in the prompt.
- Inventing links. The drafter never writes a URL that the user did not give.

## Constraints

- Crate order (plan 01): `drafter` depends on `config`, `core`, `http`, `data`, and `vault`; it does not depend on `postit-media`, `postit-publishing`, `postit-oauth`, or any agent crate. `postit-publishing` may later depend on it (E1).
- Every outbound request goes through `postit-http`: the public client with `AddressPolicy { allow_private: ai.allow_private_endpoints }`, sent through `send_with_retry`.
- `OwnerScope` is the only access check (plan 01). Provider configuration is owner-only; generating and listing providers and models need `edit` with every target inside the account limit.
- Single migration policy: schema changes edit `backend/crates/data/migrations/0001_init.sql`.
- Secrets in `secrecy` types, sealed with `postit-vault`, redaction tested through the shared no-secret-leak harness.
- Quality gates in `CLAUDE.md`, plus `cargo xtask openapi --check` and `cargo deny`.
- `[ai]` already exists in `postit-config` (`request_timeout`, default 120 seconds; `allow_private_endpoints`, default false), marked "Reserved". D2 starts reading it and drops the word.

## Phase-start findings

Read from each provider's official docs on 2026-11-06. Verify again if the phase slips. Base URLs are prefilled and editable by the user.

| Preset | Protocol | Base URL | Model list | Structured output | Images | Notes |
|---|---|---|---|---|---|---|
| Google Gemini | openai-compatible | `https://generativelanguage.googleapis.com/v1beta/openai/` | `GET models` | yes | `image_url` data URL | Free tier on Flash models (content may be used to improve Google's products; the guide says so). Compatibility is labeled beta. Some models cannot turn reasoning off. |
| OpenAI | openai-compatible | `https://api.openai.com/v1` | `GET models` | yes (`json_schema`) | `image_url` data URL | Newer models take `max_completion_tokens` and refuse `max_tokens`, and reasoning models refuse a non-default `temperature`; the preset sends `max_completion_tokens` and no temperature. The docs site refused automated reads; the shape is the long-standing Chat Completions API. Wiremock fixtures follow it. |
| Anthropic | anthropic | `https://api.anthropic.com` | `GET /v1/models`, paged (`after_id`, `has_more`) | `output_config.format` with `json_schema`, no beta header | `image` block, base64 source | Headers `x-api-key` and `anthropic-version`. The docs' examples show `2026-10-21`; the builder confirms the current value and keeps it in one constant. Its OpenAI-compatible layer is limited (no `json_object`, empty token details), so the native API is used, as the roadmap says. |
| OpenRouter | openai-compatible | `https://openrouter.ai/api/v1` | `GET models` | yes, per model | `image_url` | Optional `HTTP-Referer` and `X-OpenRouter-Title` headers; the preset sends `X-OpenRouter-Title: postit`. Usage always returned. |
| Abacus AI RouteLLM | openai-compatible | `https://routellm.abacus.ai/v1` | `GET models` | yes (`json_schema`, `json_object`) | content arrays with images (shape not confirmed) | Needs a ChatLLM subscription; no free API tier. Error body `{error:{message,type,code}}`. |
| MiniMax | openai-compatible | `https://api.minimax.io/v1` | none documented | not documented | `image_url` | Reasoning may arrive inside `content` wrapped in `<think>` tags; the preset sends `reasoning_split: true` and the parser strips a leading `<think>` block anyway. `temperature` is ignored. |
| Moonshot (Kimi) | openai-compatible | `https://api.moonshot.ai/v1` | not confirmed | yes (`json_schema`) | `image_url` data URL | Reasoning arrives in `reasoning_content`, which is ignored. Docs moved to `platform.kimi.ai`. |
| Ollama (local) | openai-compatible | `http://localhost:11434/v1` | `GET models` | `json_object` only confirmed | base64 data URL only | Key ignored. |
| llama.cpp server (local) | openai-compatible | `http://localhost:9931/v1` | `GET models` | grammar JSON schema on its native API; OpenAI `response_format` not confirmed | experimental, with `--mmproj` | The README's default port is 9931; Docker images often use 8080. Key optional (`--api-key`). |
| vLLM (local) | openai-compatible | `http://localhost:8000/v1` | `GET models` | yes (`json_schema`) | not confirmed | Needs a chat template. |
| LM Studio (local) | openai-compatible | `http://localhost:1234/v1` | `GET models` | yes (`json_schema`), small models unreliable | yes | Key placeholder. |

**Consumer subscriptions.** ChatGPT Plus, Claude Pro, and Gemini Advanced cannot be used by third-party apps. The guide says so plainly (roadmap). D2 carries a `subscription_note` flag on those presets for the clients.

**Opencode's shape.** opencode's docs (updated 2026-11-05) document `provider.<id>` with `npm`, `name`, `options` (`baseURL`, `apiKey`, `headers`, `timeout`), and `models.<id>` with `name`, `limit.context`, `limit.output`, and `modalities.input`/`output`. Its published JSON schema marks `provider` deprecated in favor of a new `providers` key (`package` for `npm`, provider-level `headers`). **(call)** postit follows the documented `provider` entry, which is what users paste today; the `providers` form is not accepted until opencode documents it. `{env:...}` and `{file:...}` values are refused with a violation, since postit never reads the server's environment or files on a user's behalf.

**Local presets in Docker.** `localhost` inside the postit container is the container itself. The guide tells self-hosters to use `host.docker.internal` (or the host's address) with `ai.allow_private_endpoints` on.

## Decisions

### Crate layout

`backend/crates/drafter/` (`postit-drafter`): `provider` (the `LlmProvider` trait and request and response types), `openai` and `anthropic` (the two protocols), `presets` (the catalog), `config` (the opencode-shaped config, parsing, and the `npm` to protocol map), `prompt` (prompt assembly per mode), `output` (the JSON schema, parsing, and mapping to drafts), `service` (`DrafterService`), `error`, and a `testkit` feature with a scripted `LlmProvider`. Plan 01 lists `LlmProvider` among the `dyn` traits; it uses `async-trait` like the others.

### `LlmProvider`

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
    async fn models(&self, deadline: Instant) -> Result<Vec<RemoteModel>, LlmError>;
}
```

- `ChatRequest`: `model`, `system`, `messages` (each a role and parts: text or image, the image as bytes plus MIME type), `temperature`, `max_tokens`, `output: OutputMode` (`JsonSchema { name, schema }`, `JsonObject`, or `Text`), and a `deadline`. Its `Debug` shows sizes and counts only, never the prompt, the text, or image bytes. `models` takes a deadline too, since a paged listing makes several requests.
- `ChatResponse`: `text`, `usage: Option<Usage>` (input and output tokens; missing `usage` is `None`, never an error), `finish: Finish` (`Stop`, `Length`, `Refused`, `Other`).
- `LlmError` maps to one reason each: `key_rejected` (401, 403), `model_not_found` (404, or a provider error naming the model), `rate_limited` (429, with `Retry-After` when sent), `bad_request` (other 4xx), `unavailable` (5xx, connection refused), `timeout`, `address_refused` (the address policy), `invalid_response` (a body that is not the protocol's shape, a redirect, or a body over the cap). An error carries its reason and `Retry-After` only, never text from the provider's body, which may echo the request.
- **Retries (call):** requests are sent `Idempotency::NonIdempotent`. A lost response is not retried, because a second call costs the user again. A connection refused before sending, a `429`, and a `503` with `Retry-After` are retried while the deadline allows, at most twice. A `Retry-After` longer than the time left answers `rate_limited` with the provider's value at once. `postit-http` reports a not-idempotent 5xx, timeout, or dropped connection as an unknown outcome without its status; the drafter maps that to `timeout` when its deadline has passed and to `unavailable` otherwise. Success bodies are read up to 4 MiB.
- **OpenAI-compatible:** `POST {base}/chat/completions` with `Authorization: Bearer`, the configured headers, image parts as `image_url` data URLs, `response_format` from `OutputMode`, and the token limit as `max_tokens`, or as `max_completion_tokens` for the OpenAI preset (its newer models refuse `max_tokens`) or when the config's `options.token_param` says so. `temperature` is sent only when the config sets one, since reasoning models refuse any value but the default. Tolerant reading: unknown fields ignored, `usage` optional, `reasoning_content` ignored, a leading `<think>...</think>` block stripped from `content`, `content` as a string or as an array of text parts. Preset extras (MiniMax's `reasoning_split`, OpenRouter's title header) come from the preset, never from code paths keyed on a host name. Configured query parameters are appended (roadmap: "configurable base URL, headers, and query parameters").
- **Anthropic:** `POST {base}/v1/messages` with `x-api-key` and `anthropic-version`, `system` as the top-level field, image blocks with base64 sources, `output_config.format` for `JsonSchema`, `max_tokens` (required by the API). `models()` follows `has_more` and `after_id` to the end, at most 10 pages. A base URL ending in `/v1` is stored without it, since the protocol adds `/v1/...` itself.
- **Token limit (call):** 8,192 output tokens, lowered to the model's `limit.output` when the config gives one. Thinking models (Gemini 2.5, OpenAI's reasoning models) count their thinking against this limit, and 2,048 left too little room for the JSON answer after it.
- **Structured output by preset (call):** each preset declares the best `OutputMode` it supports (`JsonSchema` for Gemini, OpenAI, Anthropic, OpenRouter, RouteLLM, Moonshot, vLLM, and LM Studio; `JsonObject` for Ollama; `Text` for MiniMax and llama.cpp). Custom providers default to `JsonObject` and can set `json_schema` in the config. Whatever the mode, the system prompt also describes the JSON shape, and the parser accepts a JSON object inside a fenced code block, so a provider that ignores `response_format` still works.

### Provider rows

`ai_providers` keeps its columns. `provider_key` is the preset key (`gemini`, `openai`, `anthropic`, `openrouter`, `routellm`, `minimax`, `moonshot`, `ollama`, `llamacpp`, `vllm`, `lmstudio`) or `custom`. `config` holds the opencode `provider` entry without secrets:

```json
{
  "npm": "@ai-sdk/openai-compatible",
  "name": "My provider",
  "protocol": "openai-compatible",
  "options": { "baseURL": "https://api.example.com/v1", "headers": ["X-Org"], "query": { "api-version": "2026-01-01" }, "timeout": 60000 },
  "models": { "my-model": { "name": "My model", "limit": { "context": 128000, "output": 4096 }, "modalities": { "input": ["text", "image"] } } },
  "default_model": "my-model",
  "structured_output": "json_object"
}
```

- `npm` maps to `protocol` as the roadmap says (`@ai-sdk/openai-compatible` and `@ai-sdk/openai` to `openai-compatible`; `@ai-sdk/anthropic` to `anthropic`; `@ai-sdk/google` to `openai-compatible` with the Gemini base URL). An explicit `protocol` wins. Any other `npm` value without `protocol` is a violation.
- **Secrets (call):** the sealed `api_key` column holds a `ProviderSecret { api_key: Option<SecretString>, headers: BTreeMap<String, SecretString> }`. A pasted `options.apiKey` and every `options.headers` value move into it; `config.options.headers` keeps only the header names, so the owner sees which headers are set without seeing them. The column keeps its name and its `postit:sealed` comment and stays in the vault reseal registry.
- `default_model`, `structured_output`, `options.query`, `options.token_param`, and `options.temperature` are postit additions to the shape, ignored by opencode. Paste accepts opencode's own `model: "provider/model"` form only as a hint for `default_model`.
- **Image input (call):** a model takes images when its `modalities.input` lists `image`. With no `modalities`, the preset's default applies (Gemini, OpenAI, Anthropic, OpenRouter: images on; the rest: off). The user can always set it per model.
- **Base URL checks:** `https` required unless the host is private and `ai.allow_private_endpoints` is on; no credentials in the URL; local presets are refused (`validation_failed`, with a violation naming `ai.allow_private_endpoints`) while that setting is off. The address policy still applies at send time, since a public name can resolve to a private address.
- **Default:** at most one default per owner (the existing partial unique index). Creating the owner's first provider makes it the default. Deleting the default leaves none; generation then needs `provider_id` or answers `ai_provider_required`.
- Delete keeps the owner's generation history: `ai_generations.provider_id` becomes `NULL` (the existing foreign key).

### Media as context

New in `postit-core`:

```rust
#[async_trait]
pub trait FrameExtractor: Send + Sync {
    /// A still for the model: images downscaled, a GIF's first frame.
    async fn still(&self, asset: &MediaAsset) -> Result<ContextImage, MediaError>;
    /// `count` frames spread over a video, plus its duration.
    async fn frames(&self, asset: &MediaAsset, count: u32) -> Result<VideoContext, MediaError>;
}
```

`ContextImage` is JPEG bytes with width and height; `VideoContext` is the frames and `duration_ms`. `postit-media` implements it: stills with the `image` crate it already uses, frames with `ffmpeg` behind its `ffprobe` feature (the same binary that cuts video thumbnails). Without `ffmpeg`, `frames` returns `MediaError::Unsupported` and the drafter goes on with the duration from the asset's probe data, if any. `postit-server` injects the implementation; the drafter never depends on `postit-media`.

**(call)** Long side at most 1,568 pixels, JPEG quality 85, at most 4 images per request and 4 frames per video, in the post's media order. Images over those counts are left out and the response says so. Anthropic's docs recommend 1,568 pixels as the size that is not downscaled again.

When the chosen model takes no images, no media is sent, and the result carries the note `ai.media_ignored` ("This model reads text only, so your media was not shown to it"). The drafter goes on.

### Modes and prompt assembly

The request: `{ mode, prompt, targets[], language?, media_ids?, comment_id?, provider_id?, model? }` (roadmap). **(calls)** `prompt` is at most 8,000 characters, `targets` 1 to 20 account IDs, `media_ids` at most 10.

- **`post`:** one variant per target. The system prompt states the rules (write only text and hashtags, never invent links or facts, never pose as a person, keep within limits); the brand profile (description, audience, offerings, words to avoid, up to five examples); then per target: the platform's display name, the format the post will most likely use with its text limit and measure, whether tags are a separate field, and the account's writing style (tone, emoji use, hashtag habits, notes). The user's prompt and the link, when given, close it. For an email target the drafter also writes a subject, a preheader, and body paragraphs.
- **`reply`:** `comment_id` names an inbox item; exactly one target, the item's account. The prompt holds the text that was published to that account (the item's delivery, then its job's frozen snapshot, so a later edit of the post does not change it), the comment as quoted data with its author's display name, the brand profile, and the account's writing style. The reply is in the comment's language unless `language` is set (roadmap). The variant is a `comment` kind with `reply_to` from the item. An item with no `remote_post_id` cannot be answered (`ReplyTo` needs one) and is refused with a violation at `comment_id`.
- **`rewrite`:** `prompt` is the text to rewrite. One variant per target, a fresh wording of the same message within the target's limits. E1 calls `DrafterService::rewrite` in-process with the post's text and its targets; the route takes the same input for testing from a client.
- **Language (call):** `language` when given; else the account's writing style `language`; else the brand profile's `default_language`; else, in reply mode, the comment's language; else the acting user's UI locale. Language tags are BCP 47 strings passed through to the prompt.
- **Delimiting:** user content (prompt, examples, comment text, post text) sits in tagged blocks the system prompt names as data, so text inside cannot change the rules. A model can still be talked around; a person reviews every draft, which is the real guard.
- Prompt assembly is a pure function from its inputs to the `ChatRequest`, so each mode is snapshot-tested.

### Output, validation, and repair

- **Schema:** one JSON object, `{ "variants": [ { "target": "<account id>", "text": "...", "tags": ["..."], "email": { "subject": "...", "preheader": "...", "paragraphs": ["..."] } } ] }`, with `email` only for email targets. Tags are stripped of a leading `#` and deduplicated, and kept for every target: the agent decides how its platform shows them (Telegram appends them to the text). No descriptor says whether a platform has a separate tags field, and none is needed.
- **Mapping to a draft:** the result has the same shape as a post plus overrides, so D3's post create can take it as is. The shared `Post` holds the first variant's text, the media from `media_ids` in order, the user's link, and `disclose_ai_generated: true`. Each target's `PostOverride` holds its own text and tags; an email target's override holds an `EmailDocument` with the subject, the preheader, the language (`ar` for Arabic, else `en-US`), and one `Text` block per paragraph. **(call)** Media is not placed into the email body; the user places images in the block editor.
- **Validation:** each variant is resolved and run through `postit_core::validate_target` with the platform's descriptor and options schema from the `AgentRegistry`, `Schedule::Now`, and the request's media. The agent's own `validate` is not called: it is the publish path's, and it may call the platform. For that to hold, core validation must see everything the platform counts: a format whose `TextRules.includes_link_and_tags` is set (Telegram today; Bluesky and Mastodon later) is measured on the composed string (text, then the link unless the text holds it, then `#tags`), through `postit_core::post_text`, which moves from `postit-agent-kit` into core. The target's brief names the format `pick_format` picks with the request's media attached, so a photo post is written to the caption limit.
- **Repair:** when the output is not valid JSON in the shape, or any variant has a text-length, tag, or email violation, one repair request goes out with the previous output and the violations rendered as plain English (`text is 412 characters; the limit is 300`). Violations about media or options are not the model's to fix and do not trigger a repair. **(call)** After the repair, variants that still violate are returned with their violations, so the user fixes them by hand. A response still not parseable after the repair answers `ai_provider_failed` with reason `invalid_output`.
- **Deadline:** both calls share one deadline, `ai.request_timeout` from the start of the request.

### Generation records and spend

- One `ai_generations` row per generate request that the provider answered, written after the answer, with usage summed over the repair call. A request that never reached the provider writes none: nothing was spent.
- **Schema change (call):** `ai_generations` gains `outcome TEXT NOT NULL CHECK (outcome IN ('ok', 'invalid_output', 'failed'))` so the spend view shows paid calls that produced nothing usable.
- **`prompt_hash` (call):** HMAC-SHA-256 over the owner's ID and the user's prompt, keyed by a key derived from the vault (`Vault::derive_key("ai_prompt_hash")`). A bare SHA-256 would let anyone reading the database guess short prompts the owner chose not to store. The hash only marks repeats of one owner's prompt, so a vault key rotation changing it is acceptable. `prompt` stores the user's prompt only when the owner's `store_ai_prompts` is on, read at generation time. The assembled system prompt is never stored; it is rebuilt from code and profile.
- `created_by` is the actor, so a delegate's generations show who made them. `post_id` stays `NULL`; D3 calls `DrafterService::link_generation(scope, generation_id, post_id)` when a draft is saved.
- `GET /ai/generations`: owner-only, newest first, paged with the shared `page` and `page_size` parameters like every other list, with actor, mode, provider name (or "deleted provider"), model, outcome, token counts, and the stored prompt when present. Retention already clears prompts after 30 days and rows after one year.

### Routes and access

| Route | Access | Notes |
|---|---|---|
| `GET /ai/presets` | any signed-in user | Key, name, protocol, base URL, guide URL, `local`, `available` (false for local presets while private endpoints are off), `subscription_note`. |
| `GET /ai/providers` | `edit` | Owner: config without secrets, `has_key`, header names. Delegate: ID, name, preset, models, default. |
| `POST /ai/providers`, `PATCH /ai/providers/{id}`, `DELETE /ai/providers/{id}` | owner | Create from a preset (key plus optional default model) or from a pasted opencode entry. `PATCH` with a new key writes `ai_provider_key_changed`; other changes `ai_provider_updated`. Owner-only through a delegation answers `owner_only`. |
| `POST /ai/providers/{id}/test` | owner | **(call)** one tiny chat (`Reply with OK`, at most 5 output tokens; any successful answer counts, since a thinking model may spend all 5 thinking) to the default model, plus a model listing when the protocol has one. Answers `{ ok, reason?, models_found? }`. Writes no generation row. |
| `GET /ai/providers/{id}/models` | `edit` | Config models merged with the provider's live list when it has one; a live list failure returns the config models with `remote_error`. |
| `POST /ai/generate` | `edit`, every target in limit | `ai_provider_required` (409) when no provider is given and the owner has no default, or the given one is gone. Answers 200 with `generation_id`, provider and model, the draft (`post`, `overrides`), per-target `violations`, `notes`, and `usage`. |
| `GET /ai/generations` | owner | As above. |

- **Error codes:** `ai_provider_required` (already in the roadmap's list) and **(call)** one new code, `ai_provider_failed` (502), with the reason in `detail` (`key_rejected`, `model_not_found`, `rate_limited`, `bad_request`, `unavailable`, `timeout`, `address_refused`, `invalid_response`, `invalid_output`), the same pattern as `test_send_refused`. A provider `429` sets `Retry-After` on our 502 when the provider gave one. The roadmap's code list gains it.
- **Timeouts:** `generate`, `test`, and `models` move to the custom-limits route group with a timeout of `ai.request_timeout` plus 5 seconds, so the drafter's own deadline answers first with a clear reason.
- **Rate limit (call):** a new per-user limiter `rate_limit.ai_requests` (20 per minute, burst 5) on `generate`, `test`, and `models`. D3's list of per-user limits includes generation; D2 adds it now because the route lands now.
- **Feature:** `postit-api` gains an `ai` feature (on by default) that compiles the `/ai` routes and the `AppState.drafter` field; the server's `ai` feature (on by default) turns it on, adds `postit-drafter`, and wires the service. A build with `--no-default-features` has no `/ai` routes. OpenAPI is generated with default features.
- **Isolation tests** for the four callers (owner, delegate at each level, delegate limited to other accounts, unrelated user) on every route.

### Data layer

- `postit-data`: `AiProviderRepo` (create, get, list, update, delete, set default; owner-scoped; the sealed secret bound to `(ai_providers, api_key, id)`), `AiGenerationRepo` (insert, link to a post, page), and a read-only `InboxItemRepo::get` scoped by owner and account limit, returning an `InboxItemRow` (core already has an agent-facing `InboxItem`), enough for reply mode. E2 grows the inbox repository. Creating a provider and changing the default lock the owner's row, so two concurrent requests never both make a default.
- `postit-vault` gains `Vault::derive_key(purpose)` for the prompt hash key.
- `AuditEventKind` gains the four AI events. Details carry the provider ID, preset key, and name, never config values.
- `delete_user` needs no new step: `ai_providers` and `ai_generations` cascade with the owner.

### Configuration

`[ai]` keeps its two keys. `docs/configuration.md` documents them and the new `rate_limit.ai_requests`. No AI setting is deployment-level beyond these.

## Testing

- **Protocols (wiremock):** request shape per protocol (headers, image parts, `response_format` or `output_config`, query parameters), a missing `usage`, `content` as an array, a `<think>` block stripped, `reasoning_content` ignored, a fenced JSON answer, Anthropic model paging, each error status to its reason, a `429` with `Retry-After` retried within the deadline, a lost response not retried, the deadline cutting a slow answer.
- **Presets:** one fixture per preset with its quirks (MiniMax think tags and `reasoning_split`, OpenRouter title header, Moonshot reasoning field, RouteLLM error body, Ollama key ignored), and local presets refused while private endpoints are off.
- **Address policy:** a provider whose name resolves to a private address is refused with `address_refused`; a redirect is not followed.
- **Config:** opencode paste round trips, `npm` mapping, explicit `protocol` winning, unknown `npm` refused, `{env:...}` and `{file:...}` refused, `apiKey` and header values moved into the sealed secret and absent from `config`.
- **Prompt assembly (insta snapshots):** each mode, with and without a brand profile and writing style, an email target, an Arabic target, a reply with hostile comment text, and media notes.
- **Media context:** images sent to an image model, frames plus duration for a video, a text-only model with images attached (no image sent, note present), `ffmpeg` missing.
- **Validation and repair (scripted provider):** a valid first answer (one call), an over-limit text repaired (two calls), a Telegram-like target over its limit only once the link and tags are added (repaired), a photo target briefed with the caption limit, a still-invalid answer returned with violations, unparseable twice giving `invalid_output`, media and option violations not triggering a repair.
- **Records:** one row per answered request with summed usage and the right outcome; prompt stored only with `store_ai_prompts`; no row when the provider never answered.
- **Routes (HTTP level, mock agent registry):** every route, `ai_provider_required`, `ai_provider_failed` reasons, the four-caller isolation matrix, a delegate never seeing secrets or config, a limited delegate refused for a target outside the limit.
- **Leaks:** the shared no-secret-leak harness over provider create, test, generate, and failures: no key, header value, or prompt in logs, spans, errors, or audit rows.
- **Live (opt-in):** `POSTIT_LIVE_TESTS=1` with `POSTIT_LIVE_GEMINI_KEY` runs one `post`-mode generation against Gemini's free tier; `#[ignore]` otherwise. CI never calls a real provider.

## Human verification

A `-verification.md` entry at plan end: run the live Gemini test with a real key, and paste a real opencode provider entry from the user's own setup.

## Left to the builder

- Internal function signatures and module boundaries, as long as the behavior above holds.
- The exact English wording of the system prompts, within the rules above, and of the `ai.*` messages.
- The guide URLs per preset (each provider's API-key page).
- The current `anthropic-version` value, confirmed against Anthropic's docs and kept in one constant.
