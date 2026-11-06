// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_generation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiGenerationDto _$AiGenerationDtoFromJson(Map<String, dynamic> json) =>
    AiGenerationDto(
      createdAt: DateTime.parse(json['created_at'] as String),
      id: json['id'] as String,
      mode: AiModeDto.fromJson(json['mode'] as String),
      model: json['model'] as String,
      outcome: AiGenerationOutcomeDto.fromJson(json['outcome'] as String),
      actor: json['actor'] as String?,
      inputTokens: (json['input_tokens'] as num?)?.toInt(),
      outputTokens: (json['output_tokens'] as num?)?.toInt(),
      postId: json['post_id'] as String?,
      prompt: json['prompt'] as String?,
      providerId: json['provider_id'] as String?,
      providerName: json['provider_name'] as String?,
    );

Map<String, dynamic> _$AiGenerationDtoToJson(AiGenerationDto instance) =>
    <String, dynamic>{
      'actor': instance.actor,
      'created_at': instance.createdAt.toIso8601String(),
      'id': instance.id,
      'input_tokens': instance.inputTokens,
      'mode': _$AiModeDtoEnumMap[instance.mode]!,
      'model': instance.model,
      'outcome': _$AiGenerationOutcomeDtoEnumMap[instance.outcome]!,
      'output_tokens': instance.outputTokens,
      'post_id': instance.postId,
      'prompt': instance.prompt,
      'provider_id': instance.providerId,
      'provider_name': instance.providerName,
    };

const _$AiModeDtoEnumMap = {
  AiModeDto.post: 'post',
  AiModeDto.reply: 'reply',
  AiModeDto.rewrite: 'rewrite',
  AiModeDto.$unknown: r'$unknown',
};

const _$AiGenerationOutcomeDtoEnumMap = {
  AiGenerationOutcomeDto.ok: 'ok',
  AiGenerationOutcomeDto.invalidOutput: 'invalid_output',
  AiGenerationOutcomeDto.failed: 'failed',
  AiGenerationOutcomeDto.$unknown: r'$unknown',
};
