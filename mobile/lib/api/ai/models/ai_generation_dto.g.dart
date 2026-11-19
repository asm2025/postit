// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_generation_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiGenerationDto _$AiGenerationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['created_at', 'id', 'mode', 'model', 'outcome'],
    disallowNullValues: const ['created_at', 'id', 'mode', 'model', 'outcome'],
  );
  return AiGenerationDto(
    actor: json['actor'] as String?,
    createdAt: AiGenerationDto._read_createdAt(json['created_at']),
    id: json['id'] as String,
    inputTokens: AiGenerationDto._read_inputTokens(json['input_tokens']),
    mode: AiGenerationDto._read_mode(json['mode']),
    model: json['model'] as String,
    outcome: AiGenerationDto._read_outcome(json['outcome']),
    outputTokens: AiGenerationDto._read_outputTokens(json['output_tokens']),
    postId: json['post_id'] as String?,
    prompt: json['prompt'] as String?,
    providerId: json['provider_id'] as String?,
    providerName: json['provider_name'] as String?,
    additionalProperties:
        AiGenerationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiGenerationDto._readExtras(
            AiGenerationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiGenerationDtoToJson(
  AiGenerationDto instance,
) => <String, dynamic>{
  'actor': ?instance.actor,
  'created_at': ?AiGenerationDto._write_createdAt(instance.createdAt),
  'id': instance.id,
  'input_tokens': ?AiGenerationDto._write_inputTokens(instance.inputTokens),
  'mode': ?AiGenerationDto._write_mode(instance.mode),
  'model': instance.model,
  'outcome': ?AiGenerationDto._write_outcome(instance.outcome),
  'output_tokens': ?AiGenerationDto._write_outputTokens(instance.outputTokens),
  'post_id': ?instance.postId,
  'prompt': ?instance.prompt,
  'provider_id': ?instance.providerId,
  'provider_name': ?instance.providerName,
};
