// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_draft_dto.dart';
import 'ai_usage_dto.dart';
import 'message.dart';
import 'violation.dart';

part 'generation_dto.g.dart';

/// A generation's result. The draft is not saved: the user reviews it and saves it as a post.
@JsonSerializable()
class GenerationDto {
  const GenerationDto({
    required this.draft,
    required this.generationId,
    required this.model,
    required this.notes,
    required this.providerId,
    required this.violations,
    this.usage,
  });
  
  factory GenerationDto.fromJson(Map<String, Object?> json) => _$GenerationDtoFromJson(json);
  
  /// The draft.
  final AiDraftDto draft;

  /// The generation record, for `GET /api/v1/ai/generations`.
  @JsonKey(name: 'generation_id')
  final String generationId;

  /// The model used.
  final String model;

  /// Notes for the user, such as `ai.media_ignored`.
  final List<Message> notes;

  /// The provider used.
  @JsonKey(name: 'provider_id')
  final String providerId;

  /// Tokens spent, when the provider reported them.
  final AiUsageDto? usage;

  /// What is still wrong, by account ID; accounts with nothing wrong are left out.
  final Map<String, List<Violation>> violations;

  Map<String, Object?> toJson() => _$GenerationDtoToJson(this);
}
