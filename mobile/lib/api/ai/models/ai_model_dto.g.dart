// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelDto _$AiModelDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['id', 'images', 'kind', 'source'],
    disallowNullValues: const ['id', 'images', 'kind', 'source'],
  );
  return AiModelDto(
    id: json['id'] as String,
    images: json['images'] as bool,
    kind: AiModelDto._read_kind(json['kind']),
    name: json['name'] as String?,
    source: AiModelDto._read_source(json['source']),
    additionalProperties:
        AiModelDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiModelDto._readExtras(
            AiModelDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiModelDtoToJson(AiModelDto instance) =>
    <String, dynamic>{
      'id': instance.id,
      'images': instance.images,
      'kind': ?AiModelDto._write_kind(instance.kind),
      'name': ?instance.name,
      'source': ?AiModelDto._write_source(instance.source),
    };
