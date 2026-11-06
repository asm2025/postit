// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_delegate_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiDelegateProviderDto _$AiDelegateProviderDtoFromJson(
  Map<String, dynamic> json,
) => AiDelegateProviderDto(
  id: json['id'] as String,
  isDefault: json['is_default'] as bool,
  models: (json['models'] as List<dynamic>)
      .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  name: json['name'] as String,
  preset: json['preset'] as String,
  view: json['view'],
  defaultModel: json['default_model'] as String?,
);

Map<String, dynamic> _$AiDelegateProviderDtoToJson(
  AiDelegateProviderDto instance,
) => <String, dynamic>{
  'default_model': instance.defaultModel,
  'id': instance.id,
  'is_default': instance.isDefault,
  'models': instance.models,
  'name': instance.name,
  'preset': instance.preset,
  'view': instance.view,
};
