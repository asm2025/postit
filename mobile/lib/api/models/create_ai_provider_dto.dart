// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'create_ai_provider_dto.g.dart';

/// Adds a provider: from a preset (`preset`, plus `api_key` unless the preset is local, and an.
/// optional `default_model`), or from a pasted opencode configuration (`opencode`). Exactly one.
/// of `preset` and `opencode`.
@JsonSerializable()
class CreateAiProviderDto {
  const CreateAiProviderDto({
    this.apiKey,
    this.defaultModel,
    this.opencode,
    this.preset,
  });
  
  factory CreateAiProviderDto.fromJson(Map<String, Object?> json) => _$CreateAiProviderDtoFromJson(json);
  
  /// The API key. Write-only: never returned.
  @JsonKey(name: 'api_key')
  final String? apiKey;

  /// The model used when a request names none.
  @JsonKey(name: 'default_model')
  final String? defaultModel;

  /// A pasted opencode `provider` entry, optionally wrapped as `{ "provider": { "<id>":.
  /// {...} } }`. Its `options.apiKey` and `options.headers` values are stored sealed and never.
  /// returned; `{env:...}` and `{file:...}` values are refused.
  final dynamic opencode;

  /// The preset key, from `GET /api/v1/ai/presets`.
  final String? preset;

  Map<String, Object?> toJson() => _$CreateAiProviderDtoToJson(this);
}
