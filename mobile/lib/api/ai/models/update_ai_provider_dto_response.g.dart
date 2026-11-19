// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_ai_provider_dto_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdateAiProviderDto$Response _$UpdateAiProviderDto$ResponseFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const [
      'base_url',
      'default_model',
      'headers',
      'make_default',
      'models',
      'name',
    ],
  );
  return UpdateAiProviderDto$Response(
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
}

Map<String, dynamic> _$UpdateAiProviderDto$ResponseToJson(
  UpdateAiProviderDto$Response instance,
) => <String, dynamic>{
  'base_url': ?instance.baseUrl,
  'default_model': ?instance.defaultModel,
  'headers': ?instance.headers,
  'make_default': ?instance.makeDefault,
  'models': ?instance.models?.map((k, e) => MapEntry(k, e.toJson())),
  'name': ?instance.name,
};
