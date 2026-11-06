// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_model_config_dto.dart';

part 'update_ai_provider_dto.g.dart';

/// Changes a provider. Absent members keep their value.
@JsonSerializable()
class UpdateAiProviderDto {
  const UpdateAiProviderDto({
    this.apiKey,
    this.baseUrl,
    this.defaultModel,
    this.headers,
    this.makeDefault,
    this.models,
    this.name,
  });
  
  factory UpdateAiProviderDto.fromJson(Map<String, Object?> json) => _$UpdateAiProviderDtoFromJson(json);
  
  /// Replaces the API key. Write-only: never returned.
  @JsonKey(name: 'api_key')
  final String? apiKey;

  /// New base URL.
  @JsonKey(name: 'base_url')
  final String? baseUrl;

  /// New default model; added to the models when missing.
  @JsonKey(name: 'default_model')
  final String? defaultModel;

  /// Replaces every header and its value, by header name. Write-only: only the names are.
  /// returned.
  final Map<String, String>? headers;

  /// Makes it the owner's default provider. `false` changes nothing.
  @JsonKey(name: 'make_default')
  final bool? makeDefault;

  /// Replaces every configured model, by model ID.
  final Map<String, AiModelConfigDto>? models;

  /// New display name.
  final String? name;

  Map<String, Object?> toJson() => _$UpdateAiProviderDtoToJson(this);
}
