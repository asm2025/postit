# D2 Drafter: human verification

Work only a person can do: every item needs a real AI provider key, which costs money or is tied to a person's account, or the user's own opencode setup. CI never calls a real provider, so none of it is in the tests. Each entry names the item, why an agent cannot do it, and its trigger.

## A live Gemini draft

- **Item:** run `backend/crates/drafter/tests/live.rs` with a real Google AI Studio key: `POSTIT_LIVE_TESTS=1 POSTIT_LIVE_GEMINI_KEY=... cargo test -p postit-drafter --test live -- --ignored`, on the preset's suggested default model (`gemini-2.5-flash` unless `POSTIT_LIVE_GEMINI_MODEL` names another). It asserts a non-empty draft that core validation accepts, `usage` present, and `finish` `Stop`. If it stops at the length limit, the default 8,192 output tokens are too few for the model's thinking; if the schema is refused, Gemini's OpenAI-compatible layer does not take the `json_schema` the preset sends (see the schema entry below).
- **Why an agent cannot:** it needs a person's Google key and calls Google for real.
- **Trigger:** before D4 ships the AI panel.

## A real opencode provider entry

- **Item:** paste a real `provider` entry from the user's own opencode configuration into `POST /ai/providers` (`{"opencode": {...}}`), with the key pasted in place of any `{env:...}` value, and generate once with it. Confirm the entry is accepted field for field, the key and header values are gone from the stored config, and the draft comes back.
- **Why an agent cannot:** it needs the user's own opencode setup and the key behind it.
- **Trigger:** D4 (the providers screen).

## OpenAI and Anthropic keys through the test route

- **Item:** create one provider from the OpenAI preset and one from the Anthropic preset with real keys, and call `POST /ai/providers/{id}/test` on each, then generate once with each. This confirms against the live APIs that OpenAI takes `max_completion_tokens` (and no `temperature`), and that Anthropic accepts the pinned `anthropic-version` `2026-10-21` (`ANTHROPIC_VERSION` in `backend/crates/drafter/src/anthropic.rs`).
- **Why an agent cannot:** it needs paid keys for both services.
- **Trigger:** D4.

## Anthropic's request and paging shape

- **Item:** with the Anthropic key above, confirm the three things built from Anthropic's docs alone: the version value `2026-10-21` is current; `output_config.format` with a `json_schema` is accepted with no beta header; and `GET /v1/models` pages with `after_id` in the request and `has_more` and `last_id` in the answer (`GET /ai/providers/{id}/models` lists every model, over more than one page if the account has that many). If the version moved, change the one constant.
- **Why an agent cannot:** it needs a paid Anthropic key; the docs were the only source at build time.
- **Trigger:** D4, with the entry above.

## Structured output accepted by Gemini and Anthropic

- **Item:** in the generations above, confirm that Gemini's OpenAI-compatible endpoint and Anthropic's Messages API both accept the drafter's answer schema (`output_schema` in `backend/crates/drafter/src/prompt.rs`: a `variants` array with an `enum` of target IDs, `additionalProperties: false`, and an `email` object only for email targets) rather than refusing it with a 400. A refusal shows as `ai_provider_failed` with `bad_request`; the fix is to simplify the schema or set that preset's structured output to `json_object`.
- **Why an agent cannot:** it needs real keys for both services.
- **Trigger:** with the Gemini and Anthropic entries above.

## Anthropic ignores a configured temperature

- **Item:** with the Anthropic provider above, set `options.temperature` in its config and generate once: the request must still succeed, since the drafter never sends `temperature` to Anthropic (the pinned version refuses it). If Anthropic accepts `temperature` again in a later version, decide whether to send it.
- **Why an agent cannot:** it needs a paid Anthropic key.
- **Trigger:** with the Anthropic entries above.

## Presets the docs left unconfirmed

Each entry below needs that service's account, subscription, or a model running on real hardware, and sends real requests. None of it can be done from CI or by an agent without those.

- **Item:** Abacus AI RouteLLM with images: set `modalities.input` to include `image` on a vision model (the preset leaves images off by default), generate once with an image attached, and confirm RouteLLM accepts the OpenAI `image_url` data-URL content array (its docs show content arrays with images but not their exact shape). If it works, consider turning the preset's `images_by_default` on; if not, adapt the part shape.
- **Why an agent cannot:** RouteLLM needs a paid ChatLLM subscription.
- **Trigger:** D4.

- **Item:** Moonshot (Kimi) model listing: call `GET {base}/models` with a real key. The preset sets `lists_models: false` because the docs did not confirm the route; if it answers in the OpenAI shape, turn listing on in `presets.rs`.
- **Why an agent cannot:** it needs a Moonshot key.
- **Trigger:** D4.

- **Item:** llama.cpp server: confirm which port a current `llama-server` listens on by default (the README says 9931; Docker images often use 8080) and fix the preset's base URL if needed. Then generate once and check whether it honors OpenAI `response_format`. The preset sends `Text` and relies on the prompt plus the fenced-JSON parser; if `json_schema` works, switch the preset to it.
- **Why an agent cannot:** it needs a model downloaded and served on real hardware.
- **Trigger:** D4.

- **Item:** vLLM with images: serve a vision model with vLLM and generate once with an image attached, to confirm it takes the `image_url` data-URL parts. The docs did not confirm it, so the preset's images are off by default.
- **Why an agent cannot:** it needs a GPU host serving a vision model.
- **Trigger:** D4.

- **Item:** a local model server from the Docker stack: with `POSTIT__AI__ALLOW_PRIVATE_ENDPOINTS=true`, point a local preset (Ollama, say) at `http://host.docker.internal:11434/v1`, test it with `POST /ai/providers/{id}/test`, and generate once. Confirm the address policy allows it, and that with the flag off the same base URL is refused when saved.
- **Why an agent cannot:** it needs a model server on the Docker host and the running stack.
- **Trigger:** D4, or G1 if self-hosters are told about it first.
