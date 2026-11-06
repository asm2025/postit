// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_modalities_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiModalitiesDto _$AiModalitiesDtoFromJson(Map<String, dynamic> json) =>
    AiModalitiesDto(
      input: (json['input'] as List<dynamic>?)
          ?.map((e) => e as String)
          .toList(),
    );

Map<String, dynamic> _$AiModalitiesDtoToJson(AiModalitiesDto instance) =>
    <String, dynamic>{'input': instance.input};
