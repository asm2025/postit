// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_ai_provider_dto_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateAiProviderDto$Request _$CreateAiProviderDto$RequestFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    allowedKeys: const ['api_key', 'default_model', 'opencode', 'preset'],
  );
  return CreateAiProviderDto$Request(
    apiKey: json['api_key'] as String?,
    defaultModel: json['default_model'] as String?,
    opencode: json['opencode'] == null
        ? null
        : CreateAiProviderDtoOpencode.fromJson(
            json['opencode'] as Map<String, dynamic>,
          ),
    preset: json['preset'] as String?,
  );
}

Map<String, dynamic> _$CreateAiProviderDto$RequestToJson(
  CreateAiProviderDto$Request instance,
) => <String, dynamic>{
  'api_key': ?instance.apiKey,
  'default_model': ?instance.defaultModel,
  'opencode': ?instance.opencode?.toJson(),
  'preset': ?instance.preset,
};
