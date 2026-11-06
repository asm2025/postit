// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_limit_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelLimitDto _$AiModelLimitDtoFromJson(Map<String, dynamic> json) =>
    AiModelLimitDto(
      context: (json['context'] as num?)?.toInt(),
      output: (json['output'] as num?)?.toInt(),
    );

Map<String, dynamic> _$AiModelLimitDtoToJson(AiModelLimitDto instance) =>
    <String, dynamic>{'context': instance.context, 'output': instance.output};
