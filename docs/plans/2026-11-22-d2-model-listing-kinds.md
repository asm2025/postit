# D2 follow-up: mark non-text models in the model listing

Status: done 2026-11-22, option 1 (flag, never hide). Implemented in `model_kind.rs`, `ModelView.kind`, `AiModelDto.kind`; the OpenAPI files and the web and mobile clients were regenerated; unit tests cover the RouteLLM IDs and chat IDs, and the service and API tests cover the field. Found 2026-11-22 while live-testing the Abacus RouteLLM preset (see `2026-11-06-d2-drafter-verification.md`).

## Problem

`GET /ai/providers/{id}/models` merges the configured models with the provider's live list. RouteLLM's list holds over 100 entries, and many cannot draft text: image generators (`gpt_image2`, `flux_pro`, `midjourney`), speech models (`gpt-transcribe`, `gemini-2.5-flash-preview-tts`), and audio models (`gpt-audio-mini`). A person picking a model in the D4 screen can pick one that fails at generate time.

## What the providers give us

The OpenAI-shaped `GET /models` entry has only `id`, `object`, `created`, and `owned_by` (RouteLLM adds `name`). It has no capability or modality field, so the type of a model cannot be read from it. OpenRouter's own listing has `architecture.output_modalities`, but the drafter calls the OpenAI-shaped route and does not read it. Anthropic's listing holds chat models only.

## Options

1. **Flag, do not hide (recommended).** Add `kind` to the listing entry (`text` or `other`), set only for remote-only models whose ID matches a short, documented pattern list (`image`, `tts`, `transcribe`, `whisper`, `audio`, `embed`, `flux`, `midjourney`, `ideogram`, `recraft`, `seedream`, `nano_banana`, `wan`, `imagine`). D4 may dim or group them. A wrong guess costs a label, not a missing model. A model the person configured by hand is never flagged.
2. **Hide.** The same patterns, but matching remote models are left out. Smaller answer, but a wrong guess makes a real model impossible to pick, except by typing its ID.
3. **Do nothing.** Leave it to D4 and the person's judgment.

## If option 1 is chosen

- `backend/crates/drafter/src/service.rs`: `ModelView` gains `kind`; the pattern list lives in one place, `presets.rs` or a new `model_kind.rs`, with unit tests on the RouteLLM IDs above and on real chat IDs (`route-llm`, `gpt-5`, `claude-opus-5`, `gemini-3.5-flash`, `qwen3.8-max`) that must stay `text`.
- `backend/crates/api/src/dto_ai.rs`: the DTO field, then `cargo xtask openapi` to regenerate `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`.
- `docs/specs/2026-11-06-d2-drafter-design.md` and contract `03. publishing roadmap.md` (the `/models` route): state that the listing flags non-text models by name and never hides one.
- The API test for the listing gains a case with a non-text remote ID.
- Close the matching note in `2026-11-06-d2-drafter-verification.md` (Abacus entry) when done.

## Decision

Option 1, chosen by the author on 2026-11-22. The D4 screen decides how to show `other` models (dim or group them).
