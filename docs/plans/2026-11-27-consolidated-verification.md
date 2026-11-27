# Consolidated human verification (open items from D1, C1/C2, D2)

Checks only a person can do, moved here on 2026-11-27 from three older verification files so the open items live in one place. Each entry names its origin, why an agent cannot do it, and its trigger. Mark an entry resolved with the date and result; when every entry is resolved, `git mv` this file to `2026-11-27-consolidated-verification-done.md`.

Origins: D1 = `2026-10-29-d1-publishing-engine-notifications-verification-done.md`, C1/C2 = `2026-10-30-c1-c2-contacts-email-agent-verification-done.md`, D2 = `2026-11-06-d2-drafter-verification-done.md`. Each origin file holds the full history of its entries.

## Email and push (D1, C1/C2)

### A campaign through Papercut, checked in its UI (C1/C2)
- **Item:** with the Docker stack and Papercut, send a campaign with an image, a button, and open tracking on, to a list with one merge-field contact, one without, and one `unsubscribed`. In Papercut confirm: one message per subscribed contact with an HTML and a plain-text part; merge values and fallbacks right; the footer shows the postal address and the unsubscribe link; `List-Unsubscribe` and `List-Unsubscribe-Post` headers present; the image loads; the button goes through `/public/t/c/…` to its destination; the unsubscribed contact got nothing. Follow the unsubscribe link: the page asks to confirm, and confirming marks the contact `unsubscribed`. Send `POST /api/v1/accounts/{id}/test-email`: it arrives without the unsubscribe headers and its footer link shows the "this was a test" page.
- **Why an agent cannot:** needs Papercut's UI and a person's browser session on `postit.local`.
- **Trigger:** any time (the D3 exit flow passed on 2026-11-27 with the basic campaign checks); repeat once in D4 when the web composer can send a campaign.

### Live alert and digest email (D1)
- **Item:** with a real SMTP provider, confirm a critical notice sends one alert email about two minutes later (several notices of one job coalesced into one email), the daily comment digest arrives at 08:00 in the user's time zone, and both render correctly in common mail clients. (The development alert email for a failed delivery was already received on 2026-11-08.)
- **Why an agent cannot:** needs a real provider account, credentials, and real mailboxes.
- **Trigger:** plan 03 G1, with the identity-mail checks.

### Rendering in real mail clients (C1/C2)
- **Item:** send a campaign (every block type, a long text block, an image with a link) to Gmail (web and Android), Outlook (web and desktop), and Apple Mail (macOS and iOS), at desktop and phone widths. Confirm the layout holds, Gmail does not clip it (HTML under 100 KiB), the plain-text part reads well, and the message avoids spam with SPF, DKIM, and DMARC set (check the `dns-check` findings for the sending domain).
- **Why an agent cannot:** needs real mailboxes, a real sending relay and domain, and a person looking at each client.
- **Trigger:** plan 03 G1, once a public sending domain exists.

### One-click unsubscribe from a real mail client (C1/C2)
- **Item:** in Gmail and Apple Mail (and Outlook where offered), use the client's own "Unsubscribe" button on a received campaign (RFC 8058 one-click). Confirm the contact becomes `unsubscribed` with an `unsubscribed` suppression and the next campaign skips the address. Open the footer link in a browser: the `GET` page changes nothing until the confirm button is pressed.
- **Why an agent cannot:** the request comes from the mail provider's servers to a public `server.public_url`; needs a public deployment, a real mailbox, and a person clicking.
- **Trigger:** plan 03 G1.

### Real TLS and STARTTLS against a relay (C1/C2)
- **Item:** connect email accounts to a real SMTP relay with `tls = starttls` (port 587) and with `tls = tls` (port 465); send a test email and a small campaign through each. Confirm the certificate is checked against the configured host name (another name's certificate must fail), login works over the encrypted session, and a relay without STARTTLS is refused rather than used in clear.
- **Why an agent cannot:** needs relay credentials only the maintainer holds.
- **Trigger:** plan 03 G1, or earlier when relay credentials are at hand.

### Real push services and UnifiedPush distributors (D1)
- **Item:** send a push through Chrome (FCM), Firefox (Mozilla autopush), Safari (Apple web push), and a UnifiedPush distributor on Android (for example ntfy, including one on a LAN with `push.allow_private_endpoints` on). Confirm delivery and the `410` cleanup.
- **Why an agent cannot:** needs real browsers and devices subscribed to the live services; CI never calls them.
- **Trigger:** D6 (mobile push, UnifiedPush) and plan 03 G1 (browser services).

## AI drafter providers (D2)

All need a key, a subscription, or hardware, and send real requests. Trigger for every entry: D4 (the providers screen and AI panel).

### OpenAI key through the test route
- **Item:** create a provider from the `openai` preset with a funded key, call `POST /ai/providers/{id}/test`, and generate once. Confirms OpenAI takes `max_completion_tokens` and no `temperature`. History: on 2026-11-21 a free-tier key authenticated and `GET /ai/providers/{id}/models` listed models, but generate answered 502 `ai_provider_failed` with `rate_limited` (no credit; the drafter maps every 429 to `rate_limited`).
- **Why an agent cannot:** needs a funded OpenAI key.

### Anthropic key: request shape, paging, schema, temperature
- **Item:** create a provider from the `anthropic` preset with a real key, test it, and generate once. Confirm: the pinned `anthropic-version` `2026-10-21` (`ANTHROPIC_VERSION` in `backend/crates/drafter/src/anthropic.rs`) is current; `output_config.format` with a `json_schema` is accepted with no beta header, and the drafter's answer schema (`output_schema` in `backend/crates/drafter/src/prompt.rs`) is not refused with a 400 (`ai_provider_failed` with `bad_request`); `GET /v1/models` pages with `after_id`, `has_more`, `last_id` (`GET /ai/providers/{id}/models` lists every model); a configured `options.temperature` still succeeds, since the drafter never sends it to Anthropic. Change the one constant if the version moved.
- **Why an agent cannot:** needs a paid Anthropic key.

### Gemini accepts the structured-output schema
- **Item:** confirm Gemini's OpenAI-compatible endpoint accepts the drafter's answer schema (a refusal shows as `ai_provider_failed` with `bad_request`; fix by simplifying the schema or setting that preset to `json_object`). The live test `live_gemini_post_mode` passed on 2026-11-20 with `gemini-flash-lite-latest`, so this is likely already confirmed; tick it off if a generate with a schema-bearing request has worked.
- **Why an agent cannot:** needs a Google key.

### RouteLLM (Abacus AI)
- **Item:** with a paid ChatLLM subscription and credit, run `POST .../test` and generate; then set `modalities.input` to include `image` on a vision model and generate with an image, confirming RouteLLM accepts the OpenAI `image_url` data-URL content array. If it works, consider turning the preset's `images_by_default` on. History: on 2026-11-22 listing worked (OpenAI shape, `route-llm` listed) but test and generate answered `rate_limited` because the monthly credit was spent.
- **Why an agent cannot:** needs a paid subscription with credit.

### Moonshot (Kimi) model listing
- **Item:** call `GET {base}/models` with a real key. The preset sets `lists_models: false`; if it answers in the OpenAI shape, turn listing on in `presets.rs`.
- **Why an agent cannot:** needs a Moonshot key.

### llama.cpp server
- **Item:** confirm which port a current `llama-server` listens on by default (the README says 9931; Docker images often use 8080) and fix the preset's base URL if needed. Generate once and check whether it honors OpenAI `response_format`; the preset sends `Text` and relies on the prompt plus the fenced-JSON parser, so switch to `json_schema` if it works.
- **Why an agent cannot:** needs a model downloaded and served on real hardware.

### NVIDIA NIM, DeepSeek, and Z.ai presets
- **Item:** create a provider from each preset with a real key (NVIDIA's is free at build.nvidia.com), test, and generate once. Confirm: NVIDIA's `GET {base}/models` answers in the OpenAI shape and whether it honors `response_format` (the preset sends `Text`); DeepSeek's `json_object` mode returns parseable drafts; Z.ai's `GET {base}/models` exists (the preset sets `lists_models: false`) and its `json_object` mode works. Fix the base URL, structured-output mode, or listing flag in `presets.rs` and its table test for any that differ.
- **Why an agent cannot:** needs a key for each service.

### vLLM with images
- **Item:** serve a vision model with vLLM and generate once with an image attached, to confirm it takes the `image_url` data-URL parts. Images are off by default in the preset until confirmed.
- **Why an agent cannot:** needs a GPU host serving a vision model.
