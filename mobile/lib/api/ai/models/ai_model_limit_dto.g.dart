// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_limit_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelLimitDto _$AiModelLimitDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(json, allowedKeys: const ['context', 'output']);
  return AiModelLimitDto(
    context: AiModelLimitDto._read_context(json['context']),
    output: AiModelLimitDto._read_output(json['output']),
  );
}

Map<String, dynamic> _$AiModelLimitDtoToJson(AiModelLimitDto instance) =>
    <String, dynamic>{
      'context': ?AiModelLimitDto._write_context(instance.context),
      'output': ?AiModelLimitDto._write_output(instance.output),
    };
