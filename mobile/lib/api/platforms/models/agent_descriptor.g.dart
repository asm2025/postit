// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'agent_descriptor.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AgentDescriptor _$AgentDescriptorFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'brand_color',
      'capabilities',
      'config_schema_version',
      'connect',
      'display_name',
      'formats',
      'guide',
      'kind',
      'options_schema',
      'platform',
      'quota_buckets',
    ],
    disallowNullValues: const [
      'brand_color',
      'capabilities',
      'config_schema_version',
      'connect',
      'display_name',
      'formats',
      'guide',
      'kind',
      'options_schema',
      'quota_buckets',
    ],
  );
  return AgentDescriptor(
    brandColor: json['brand_color'] as String,
    capabilities: AgentDescriptor._read_capabilities(json['capabilities']),
    configSchemaVersion: AgentDescriptor._read_configSchemaVersion(
      json['config_schema_version'],
    ),
    connect: ConnectMethod.fromJson(json['connect'] as Map<String, dynamic>),
    displayName: json['display_name'] as String,
    formats: (json['formats'] as List<dynamic>)
        .map((e) => FormatSpec.fromJson(e as Map<String, dynamic>))
        .toList(),
    guide: ConnectGuide.fromJson(json['guide'] as Map<String, dynamic>),
    kind: AgentDescriptor._read_kind(json['kind']),
    nativeSchedule: json['native_schedule'] == null
        ? null
        : NativeScheduleWindow.fromJson(
            json['native_schedule'] as Map<String, dynamic>,
          ),
    optionsSchema: OptionsSchema.fromJson(
      json['options_schema'] as Map<String, dynamic>,
    ),
    platform: AgentDescriptor._read_platform(json['platform']),
    quotaBuckets: (json['quota_buckets'] as List<dynamic>)
        .map((e) => QuotaBucket.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        AgentDescriptor._extras(json, 'additionalProperties') == null
        ? const {}
        : AgentDescriptor._readExtras(
            AgentDescriptor._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AgentDescriptorToJson(
  AgentDescriptor instance,
) => <String, dynamic>{
  'brand_color': instance.brandColor,
  'capabilities': ?AgentDescriptor._write_capabilities(instance.capabilities),
  'config_schema_version': ?AgentDescriptor._write_configSchemaVersion(
    instance.configSchemaVersion,
  ),
  'connect': instance.connect.toJson(),
  'display_name': instance.displayName,
  'formats': instance.formats.map((e) => e.toJson()).toList(),
  'guide': instance.guide.toJson(),
  'kind': ?AgentDescriptor._write_kind(instance.kind),
  'native_schedule': ?instance.nativeSchedule?.toJson(),
  'options_schema': instance.optionsSchema.toJson(),
  'platform': AgentDescriptor._write_platform(instance.platform),
  'quota_buckets': instance.quotaBuckets.map((e) => e.toJson()).toList(),
};
