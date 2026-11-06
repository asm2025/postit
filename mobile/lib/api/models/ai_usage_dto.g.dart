// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_usage_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiUsageDto _$AiUsageDtoFromJson(Map<String, dynamic> json) => AiUsageDto(
  inputTokens: (json['input_tokens'] as num).toInt(),
  outputTokens: (json['output_tokens'] as num).toInt(),
);

Map<String, dynamic> _$AiUsageDtoToJson(AiUsageDto instance) =>
    <String, dynamic>{
      'input_tokens': instance.inputTokens,
      'output_tokens': instance.outputTokens,
    };
