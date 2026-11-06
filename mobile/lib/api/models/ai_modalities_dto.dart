// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'ai_modalities_dto.g.dart';

/// What a model accepts.
@JsonSerializable()
class AiModalitiesDto {
  const AiModalitiesDto({
    this.input,
  });
  
  factory AiModalitiesDto.fromJson(Map<String, Object?> json) => _$AiModalitiesDtoFromJson(json);
  
  /// Input kinds, such as `text` and `image`.
  final List<String>? input;

  Map<String, Object?> toJson() => _$AiModalitiesDtoToJson(this);
}
