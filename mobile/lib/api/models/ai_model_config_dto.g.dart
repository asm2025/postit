// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_model_config_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModelConfigDto _$AiModelConfigDtoFromJson(Map<String, dynamic> json) =>
    AiModelConfigDto(
      limit: json['limit'] == null
          ? null
          : AiModelLimitDto.fromJson(json['limit'] as Map<String, dynamic>),
      modalities: json['modalities'] == null
          ? null
          : AiModalitiesDto.fromJson(
              json['modalities'] as Map<String, dynamic>,
            ),
      name: json['name'] as String?,
    );

Map<String, dynamic> _$AiModelConfigDtoToJson(AiModelConfigDto instance) =>
    <String, dynamic>{
      'limit': instance.limit,
      'modalities': instance.modalities,
      'name': instance.name,
    };
