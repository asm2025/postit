// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_usage_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiUsageDto _$AiUsageDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['input_tokens', 'output_tokens'],
    disallowNullValues: const ['input_tokens', 'output_tokens'],
  );
  return AiUsageDto(
    inputTokens: AiUsageDto._read_inputTokens(json['input_tokens']),
    outputTokens: AiUsageDto._read_outputTokens(json['output_tokens']),
    additionalProperties:
        AiUsageDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiUsageDto._readExtras(
            AiUsageDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiUsageDtoToJson(AiUsageDto instance) =>
    <String, dynamic>{
      'input_tokens': ?AiUsageDto._write_inputTokens(instance.inputTokens),
      'output_tokens': ?AiUsageDto._write_outputTokens(instance.outputTokens),
    };
