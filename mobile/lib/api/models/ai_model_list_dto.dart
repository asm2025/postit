// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_failure_reason_dto.dart';
import 'ai_model_dto.dart';

part 'ai_model_list_dto.g.dart';

/// A provider's models: the configured ones, then the ones its live list adds.
@JsonSerializable()
class AiModelListDto {
  const AiModelListDto({
    required this.models,
    this.remoteError,
  });
  
  factory AiModelListDto.fromJson(Map<String, Object?> json) => _$AiModelListDtoFromJson(json);
  
  /// Configured models first, then listed ones the configuration does not name.
  final List<AiModelDto> models;

  /// Why the live list could not be read; the configured models are still returned.
  @JsonKey(name: 'remote_error')
  final AiFailureReasonDto? remoteError;

  Map<String, Object?> toJson() => _$AiModelListDtoToJson(this);
}
