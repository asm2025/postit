// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'ai_preset_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AiPresetDto _$AiPresetDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'available',
      'base_url',
      'guide_url',
      'key',
      'local',
      'name',
      'protocol',
      'subscription_note',
    ],
    disallowNullValues: const [
      'available',
      'base_url',
      'guide_url',
      'key',
      'local',
      'name',
      'protocol',
      'subscription_note',
    ],
  );
  return AiPresetDto(
    available: json['available'] as bool,
    baseUrl: json['base_url'] as String,
    guideUrl: json['guide_url'] as String,
    key: json['key'] as String,
    local: json['local'] as bool,
    name: json['name'] as String,
    protocol: AiPresetDto._read_protocol(json['protocol']),
    subscriptionNote: json['subscription_note'] as bool,
    additionalProperties:
        AiPresetDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AiPresetDto._readExtras(
            AiPresetDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AiPresetDtoToJson(AiPresetDto instance) =>
    <String, dynamic>{
      'available': instance.available,
      'base_url': instance.baseUrl,
      'guide_url': instance.guideUrl,
      'key': instance.key,
      'local': instance.local,
      'name': instance.name,
      'protocol': ?AiPresetDto._write_protocol(instance.protocol),
      'subscription_note': instance.subscriptionNote,
    };
