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
