// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_ai_provider_dto_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateAiProviderDto$Response _$CreateAiProviderDto$ResponseFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(json, allowedKeys: const ['default_model', 'opencode', 'preset']);
  return CreateAiProviderDto$Response(
    defaultModel: json['default_model'] as String?,
    opencode: json['opencode'] == null
        ? null
        : CreateAiProviderDtoOpencode.fromJson(
            json['opencode'] as Map<String, dynamic>,
          ),
    preset: json['preset'] as String?,
  );
}

Map<String, dynamic> _$CreateAiProviderDto$ResponseToJson(
  CreateAiProviderDto$Response instance,
) => <String, dynamic>{
  'default_model': ?instance.defaultModel,
  'opencode': ?instance.opencode?.toJson(),
  'preset': ?instance.preset,
};
