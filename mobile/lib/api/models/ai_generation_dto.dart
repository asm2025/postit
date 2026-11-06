// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_generation_outcome_dto.dart';
import 'ai_mode_dto.dart';

part 'ai_generation_dto.g.dart';

/// One generation in the owner's spend history.
@JsonSerializable()
class AiGenerationDto {
  const AiGenerationDto({
    required this.createdAt,
    required this.id,
    required this.mode,
    required this.model,
    required this.outcome,
    this.actor,
    this.inputTokens,
    this.outputTokens,
    this.postId,
    this.prompt,
    this.providerId,
    this.providerName,
  });
  
  factory AiGenerationDto.fromJson(Map<String, Object?> json) => _$AiGenerationDtoFromJson(json);
  
  /// Who asked: the owner, or a delegate.
  final String? actor;

  /// When it was made.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Generation ID.
  final String id;

  /// Tokens sent.
  @JsonKey(name: 'input_tokens')
  final int? inputTokens;

  /// What was written.
  final AiModeDto mode;

  /// The model.
  final String model;

  /// How it ended.
  final AiGenerationOutcomeDto outcome;

  /// Tokens received.
  @JsonKey(name: 'output_tokens')
  final int? outputTokens;

  /// The post saved from it, when there is one.
  @JsonKey(name: 'post_id')
  final String? postId;

  /// The prompt, only when the owner keeps prompts (`store_ai_prompts`) and retention has.
  /// not cleared it.
  final String? prompt;

  /// The provider, or null once it was deleted.
  @JsonKey(name: 'provider_id')
  final String? providerId;

  /// The provider's name, or null once it was deleted (clients show "deleted provider").
  @JsonKey(name: 'provider_name')
  final String? providerName;

  Map<String, Object?> toJson() => _$AiGenerationDtoToJson(this);
}
