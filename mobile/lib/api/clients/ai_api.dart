// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/ai_model_list_dto.dart';
import '../models/ai_preset_dto.dart';
import '../models/ai_provider_dto.dart';
import '../models/ai_test_result_dto.dart';
import '../models/create_ai_provider_dto.dart';
import '../models/generate_request_dto.dart';
import '../models/generation_dto.dart';
import '../models/page_ai_generation_dto.dart';
import '../models/update_ai_provider_dto.dart';

part 'ai_api.g.dart';

@RestApi()
abstract class AiApi {
  factory AiApi(Dio dio, {String? baseUrl}) = _AiApi;

  /// Drafts text for each target in its limits, validated against its platform, with one.
  /// repair when the model's answer is not usable. Nothing is published and no post is created:.
  /// the draft has the shape of a post plus per-account overrides, for the user to review and.
  /// save. Targets that still break a rule come back with their `violations`. Needs `edit`, with.
  /// every target inside the caller's account limit; charges the per-user AI bucket.
  ///
  /// # Errors.
  ///
  /// 404 for a target, media, or comment outside the caller's scope; 409 `ai_provider_required`.
  /// when no provider is given and the owner has no default, or the given one is gone; 422.
  /// `validation_failed` with `violations` (prompt over 8,000 characters, not 1 to 20 targets,.
  /// over 10 media, media not ready, a comment that cannot be answered); 502 `ai_provider_failed`.
  /// with the reason in `detail` (`Retry-After` when the provider is rate limiting the key).
  @POST('/api/v1/ai/generate')
  Future<GenerationDto> generateAiDraft({
    @Body() required GenerateRequestDto body,
  });

  /// The owner's generations, newest first, for the spend view: who asked, the mode, the.
  /// provider and model, the outcome, and the tokens. The prompt only when the owner keeps.
  /// prompts. Owner-only.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/ai/generations')
  Future<PageAiGenerationDto> listAiGenerations({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// The preset catalog: seven cloud services, four local servers. Local presets are not.
  /// `available` while this server refuses private endpoints (`ai.allow_private_endpoints`).
  /// Any signed-in user, in any workspace they act in.
  ///
  /// # Errors.
  ///
  /// Only the shared authentication and rate-limit refusals.
  @GET('/api/v1/ai/presets')
  Future<List<AiPresetDto>> listAiPresets();

  /// The owner's providers, oldest first. The owner sees each configuration without its.
  /// secrets (`view: owner`); a delegate sees names and models only (`view: delegate`). Needs.
  /// `edit`.
  ///
  /// # Errors.
  ///
  /// 403 `capability_required` below `edit`.
  @GET('/api/v1/ai/providers')
  Future<List<AiProviderDto>> listAiProviders();

  /// Adds a provider, from a preset (`preset` with `api_key` unless the preset is local) or from.
  /// a pasted opencode entry (`opencode`). The key and header values are sealed and never.
  /// returned. The owner's first provider becomes the default. Recorded as.
  /// `ai_provider_created`. Owner-only.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed`: `field` is `preset` when neither or both of `preset` and.
  /// `opencode` are sent; otherwise `violations` lists every rule broken (an unknown preset, a.
  /// missing key, a local preset while private endpoints are off, an `{env:...}` value, a bad base.
  /// URL).
  @POST('/api/v1/ai/providers')
  Future<AiProviderDto> createAiProvider({
    @Body() required CreateAiProviderDto body,
  });

  /// One of the owner's providers, in the view the caller may see: the owner's configuration.
  /// without secrets, or a delegate's names and models. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 403 `capability_required` below `edit`; 404 for an unknown provider.
  ///
  /// [id] - Provider ID.
  @GET('/api/v1/ai/providers/{id}')
  Future<AiProviderDto> getAiProvider({
    @Path('id') required String id,
  });

  /// Deletes a provider. Its generations stay in the history, with no provider. Deleting the.
  /// default leaves the owner with none. Recorded as `ai_provider_deleted`. Owner-only.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown provider.
  ///
  /// [id] - Provider ID.
  @DELETE('/api/v1/ai/providers/{id}')
  Future<void> deleteAiProvider({
    @Path('id') required String id,
  });

  /// Changes a provider. A new `api_key` or `headers` is recorded as `ai_provider_key_changed`;.
  /// any other change, the default included, as `ai_provider_updated`. Owner-only.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown provider; 422 `validation_failed` with `violations`.
  ///
  /// [id] - Provider ID.
  @PATCH('/api/v1/ai/providers/{id}')
  Future<AiProviderDto> updateAiProvider({
    @Path('id') required String id,
    @Body() required UpdateAiProviderDto body,
  });

  /// A provider's models: the configured ones merged with its live list, when it has one. A.
  /// failed live list still returns the configured models, with `remote_error`. Needs `edit`;.
  /// charges the per-user AI bucket.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown provider.
  ///
  /// [id] - Provider ID.
  @GET('/api/v1/ai/providers/{id}/models')
  Future<AiModelListDto> listAiModels({
    @Path('id') required String id,
  });

  /// Tests the connection: one tiny chat (`Reply with OK`, at most 5 output tokens) to the.
  /// default model, plus the live model list when the provider has one. A provider failure is.
  /// `ok: false` with its `reason`, not an error. Writes no generation. Owner-only; charges the.
  /// per-user AI bucket.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown provider; 422 `validation_failed` (`violations` at `default_model`) when.
  /// there is no model to chat with and no model list.
  ///
  /// [id] - Provider ID.
  @POST('/api/v1/ai/providers/{id}/test')
  Future<AiTestResultDto> testAiProvider({
    @Path('id') required String id,
  });
}
