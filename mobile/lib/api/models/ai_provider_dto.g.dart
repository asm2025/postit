// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_provider_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Map<String, dynamic> _$AiProviderDtoToJson(AiProviderDto instance) =>
    <String, dynamic>{};

AiProviderDtoAiDelegateProviderDto _$AiProviderDtoAiDelegateProviderDtoFromJson(
  Map<String, dynamic> json,
) => AiProviderDtoAiDelegateProviderDto(
  defaultModel: json['defaultModel'] as String?,
  id: json['id'] as String,
  isDefault: json['isDefault'] as bool,
  models: (json['models'] as List<dynamic>)
      .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  name: json['name'] as String,
  preset: json['preset'] as String,
  view: json['view'],
);

Map<String, dynamic> _$AiProviderDtoAiDelegateProviderDtoToJson(
  AiProviderDtoAiDelegateProviderDto instance,
) => <String, dynamic>{
  'defaultModel': instance.defaultModel,
  'id': instance.id,
  'isDefault': instance.isDefault,
  'models': instance.models,
  'name': instance.name,
  'preset': instance.preset,
  'view': instance.view,
};

AiProviderDtoAiOwnerProviderDto _$AiProviderDtoAiOwnerProviderDtoFromJson(
  Map<String, dynamic> json,
) => AiProviderDtoAiOwnerProviderDto(
  baseUrl: json['baseUrl'] as String?,
  defaultModel: json['defaultModel'] as String?,
  hasKey: json['hasKey'] as bool,
  headerNames: (json['headerNames'] as List<dynamic>)
      .map((e) => e as String)
      .toList(),
  id: json['id'] as String,
  isDefault: json['isDefault'] as bool,
  models: (json['models'] as List<dynamic>)
      .map((e) => AiModelDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  name: json['name'] as String,
  preset: json['preset'] as String,
  protocol: AiProtocolDto.fromJson(json['protocol'] as String),
  view: json['view'],
);

Map<String, dynamic> _$AiProviderDtoAiOwnerProviderDtoToJson(
  AiProviderDtoAiOwnerProviderDto instance,
) => <String, dynamic>{
  'baseUrl': instance.baseUrl,
  'defaultModel': instance.defaultModel,
  'hasKey': instance.hasKey,
  'headerNames': instance.headerNames,
  'id': instance.id,
  'isDefault': instance.isDefault,
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
