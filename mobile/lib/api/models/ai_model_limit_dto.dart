// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'ai_model_limit_dto.g.dart';

/// A model's token limits.
@JsonSerializable()
class AiModelLimitDto {
  const AiModelLimitDto({
    this.context,
    this.output,
  });
  
  factory AiModelLimitDto.fromJson(Map<String, Object?> json) => _$AiModelLimitDtoFromJson(json);
  
  /// Context window, in tokens.
  final int? context;

  /// Output limit, in tokens; lowers the drafter's 8,192.
  final int? output;

  Map<String, Object?> toJson() => _$AiModelLimitDtoToJson(this);
}
