// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_model_source_dto.dart';

part 'ai_model_dto.g.dart';

/// One model of a provider.
@JsonSerializable()
class AiModelDto {
  const AiModelDto({
    required this.id,
    required this.images,
    required this.source,
    this.name,
  });
  
  factory AiModelDto.fromJson(Map<String, Object?> json) => _$AiModelDtoFromJson(json);
  
  /// The model ID.
  final String id;

  /// Whether the model is shown images and video frames.
  final bool images;

  /// Display name, when known.
  final String? name;

  /// Where it comes from.
  final AiModelSourceDto source;

  Map<String, Object?> toJson() => _$AiModelDtoToJson(this);
}
