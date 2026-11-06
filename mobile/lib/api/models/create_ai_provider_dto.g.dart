// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'create_ai_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CreateAiProviderDto _$CreateAiProviderDtoFromJson(Map<String, dynamic> json) =>
    CreateAiProviderDto(
      apiKey: json['api_key'] as String?,
      defaultModel: json['default_model'] as String?,
      opencode: json['opencode'],
      preset: json['preset'] as String?,
    );

Map<String, dynamic> _$CreateAiProviderDtoToJson(
  CreateAiProviderDto instance,
) => <String, dynamic>{
  'api_key': instance.apiKey,
  'default_model': instance.defaultModel,
  'opencode': instance.opencode,
  'preset': instance.preset,
};
