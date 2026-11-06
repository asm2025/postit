// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_modalities_dto.dart';
import 'ai_model_limit_dto.dart';

part 'ai_model_config_dto.g.dart';

/// One configured model.
@JsonSerializable()
class AiModelConfigDto {
  const AiModelConfigDto({
    this.limit,
    this.modalities,
    this.name,
  });
  
  factory AiModelConfigDto.fromJson(Map<String, Object?> json) => _$AiModelConfigDtoFromJson(json);
  
  /// Token limits.
  final AiModelLimitDto? limit;

  /// What the model accepts; `image` in `input` sends it images.
  final AiModalitiesDto? modalities;

  /// Display name.
  final String? name;

  Map<String, Object?> toJson() => _$AiModelConfigDtoToJson(this);
}
