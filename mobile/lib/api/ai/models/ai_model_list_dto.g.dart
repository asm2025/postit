// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelListDto _$AiModelListDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['models'],
    disallowNullValues: const ['models'],
  );
  return AiModelListDto(
    models: (json['models'] as List<dynamic>)
        .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    remoteError: AiModelListDto._read_remoteError(json['remote_error']),
    additionalProperties:
        AiModelListDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiModelListDto._readExtras(
            AiModelListDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiModelListDtoToJson(AiModelListDto instance) =>
    <String, dynamic>{
      'models': instance.models.map((e) => e.toJson()).toList(),
      'remote_error': ?AiModelListDto._write_remoteError(instance.remoteError),
    };
