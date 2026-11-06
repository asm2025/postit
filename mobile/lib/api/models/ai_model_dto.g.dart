// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelDto _$AiModelDtoFromJson(Map<String, dynamic> json) => AiModelDto(
  id: json['id'] as String,
  images: json['images'] as bool,
  source: AiModelSourceDto.fromJson(json['source'] as String),
  name: json['name'] as String?,
);

Map<String, dynamic> _$AiModelDtoToJson(AiModelDto instance) =>
    <String, dynamic>{
      'id': instance.id,
      'images': instance.images,
      'name': instance.name,
      'source': _$AiModelSourceDtoEnumMap[instance.source]!,
    };

const _$AiModelSourceDtoEnumMap = {
  AiModelSourceDto.config: 'config',
  AiModelSourceDto.remote: 'remote',
  AiModelSourceDto.both: 'both',
  AiModelSourceDto.$unknown: r'$unknown',
};
