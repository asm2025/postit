// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_owner_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiOwnerProviderDto _$AiOwnerProviderDtoFromJson(Map<String, dynamic> json) =>
    AiOwnerProviderDto(
      hasKey: json['has_key'] as bool,
      headerNames: (json['header_names'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      id: json['id'] as String,
      isDefault: json['is_default'] as bool,
      models: (json['models'] as List<dynamic>)
          .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      name: json['name'] as String,
      preset: json['preset'] as String,
      protocol: AiProtocolDto.fromJson(json['protocol'] as String),
      view: json['view'],
      baseUrl: json['base_url'] as String?,
      defaultModel: json['default_model'] as String?,
    );

Map<String, dynamic> _$AiOwnerProviderDtoToJson(AiOwnerProviderDto instance) =>
    <String, dynamic>{
      'base_url': instance.baseUrl,
      'default_model': instance.defaultModel,
      'has_key': instance.hasKey,
      'header_names': instance.headerNames,
      'id': instance.id,
      'is_default': instance.isDefault,
      'models': instance.models,
      'name': instance.name,
      'preset': instance.preset,
      'protocol': _$AiProtocolDtoEnumMap[instance.protocol]!,
      'view': instance.view,
    };

const _$AiProtocolDtoEnumMap = {
  AiProtocolDto.openaiCompatible: 'openai-compatible',
  AiProtocolDto.anthropic: 'anthropic',
  AiProtocolDto.$unknown: r'$unknown',
};
