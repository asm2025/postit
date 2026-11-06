// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_ai_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdateAiProviderDto _$UpdateAiProviderDtoFromJson(Map<String, dynamic> json) =>
    UpdateAiProviderDto(
      apiKey: json['api_key'] as String?,
      baseUrl: json['base_url'] as String?,
      defaultModel: json['default_model'] as String?,
      headers: (json['headers'] as Map<String, dynamic>?)?.map(
        (k, e) => MapEntry(k, e as String),
      ),
      makeDefault: json['make_default'] as bool?,
      models: (json['models'] as Map<String, dynamic>?)?.map(
        (k, e) =>
            MapEntry(k, AiModelConfigDto.fromJson(e as Map<String, dynamic>)),
      ),
      name: json['name'] as String?,
    );

Map<String, dynamic> _$UpdateAiProviderDtoToJson(
  UpdateAiProviderDto instance,
) => <String, dynamic>{
  'api_key': instance.apiKey,
  'base_url': instance.baseUrl,
  'default_model': instance.defaultModel,
  'headers': instance.headers,
  'make_default': instance.makeDefault,
  'models': instance.models,
  'name': instance.name,
};
