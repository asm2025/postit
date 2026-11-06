// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'ai_usage_dto.g.dart';

/// Tokens a generation spent, summed over the repair call.
@JsonSerializable()
class AiUsageDto {
  const AiUsageDto({
    required this.inputTokens,
    required this.outputTokens,
  });
  
  factory AiUsageDto.fromJson(Map<String, Object?> json) => _$AiUsageDtoFromJson(json);
  
  /// Tokens sent.
  @JsonKey(name: 'input_tokens')
  final int inputTokens;

  /// Tokens received.
  @JsonKey(name: 'output_tokens')
  final int outputTokens;

  Map<String, Object?> toJson() => _$AiUsageDtoToJson(this);
}
