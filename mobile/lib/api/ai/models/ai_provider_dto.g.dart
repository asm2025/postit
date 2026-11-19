// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiDelegateProviderDto _$AiDelegateProviderDtoFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['id', 'is_default', 'models', 'name', 'preset'],
    disallowNullValues: const ['id', 'is_default', 'models', 'name', 'preset'],
  );
  return AiDelegateProviderDto(
    defaultModel: json['default_model'] as String?,
    id: json['id'] as String,
    isDefault: json['is_default'] as bool,
    models: (json['models'] as List<dynamic>)
        .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    name: json['name'] as String,
    preset: json['preset'] as String,
    additionalProperties:
        AiDelegateProviderDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiDelegateProviderDto._readExtras(
            AiDelegateProviderDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiDelegateProviderDtoToJson(
  AiDelegateProviderDto instance,
) => <String, dynamic>{
  'default_model': ?instance.defaultModel,
  'id': instance.id,
  'is_default': instance.isDefault,
  'models': instance.models.map((e) => e.toJson()).toList(),
  'name': instance.name,
  'preset': instance.preset,
};

AiOwnerProviderDto _$AiOwnerProviderDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'has_key',
      'header_names',
      'id',
      'is_default',
      'models',
      'name',
      'preset',
      'protocol',
    ],
    disallowNullValues: const [
      'has_key',
      'header_names',
      'id',
      'is_default',
      'models',
      'name',
      'preset',
      'protocol',
    ],
  );
  return AiOwnerProviderDto(
    baseUrl: json['base_url'] as String?,
    defaultModel: json['default_model'] as String?,
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
    protocol: AiOwnerProviderDto._read_protocol(json['protocol']),
    additionalProperties:
        AiOwnerProviderDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiOwnerProviderDto._readExtras(
            AiOwnerProviderDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiOwnerProviderDtoToJson(AiOwnerProviderDto instance) =>
    <String, dynamic>{
      'base_url': ?instance.baseUrl,
      'default_model': ?instance.defaultModel,
      'has_key': instance.hasKey,
      'header_names': instance.headerNames,
      'id': instance.id,
      'is_default': instance.isDefault,
      'models': instance.models.map((e) => e.toJson()).toList(),
      'name': instance.name,
      'preset': instance.preset,
      'protocol': ?AiOwnerProviderDto._write_protocol(instance.protocol),
    };
