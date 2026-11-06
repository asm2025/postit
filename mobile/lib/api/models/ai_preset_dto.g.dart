// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_preset_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiPresetDto _$AiPresetDtoFromJson(Map<String, dynamic> json) => AiPresetDto(
  available: json['available'] as bool,
  baseUrl: json['base_url'] as String,
  guideUrl: json['guide_url'] as String,
  key: json['key'] as String,
  local: json['local'] as bool,
  name: json['name'] as String,
  protocol: AiProtocolDto.fromJson(json['protocol'] as String),
  subscriptionNote: json['subscription_note'] as bool,
);

Map<String, dynamic> _$AiPresetDtoToJson(AiPresetDto instance) =>
    <String, dynamic>{
      'available': instance.available,
      'base_url': instance.baseUrl,
      'guide_url': instance.guideUrl,
      'key': instance.key,
      'local': instance.local,
      'name': instance.name,
      'protocol': _$AiProtocolDtoEnumMap[instance.protocol]!,
      'subscription_note': instance.subscriptionNote,
    };

const _$AiProtocolDtoEnumMap = {
  AiProtocolDto.openaiCompatible: 'openai-compatible',
  AiProtocolDto.anthropic: 'anthropic',
  AiProtocolDto.$unknown: r'$unknown',
};
