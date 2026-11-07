// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'agent_descriptor.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AgentDescriptor _$AgentDescriptorFromJson(Map<String, dynamic> json) =>
    AgentDescriptor(
      brandColor: json['brand_color'] as String,
      capabilities: (json['capabilities'] as List<dynamic>)
          .map((e) => Capability.fromJson(e as String))
          .toList(),
      configSchemaVersion: (json['config_schema_version'] as num).toInt(),
      connect: ConnectMethod.fromJson(json['connect'] as Map<String, dynamic>),
      displayName: json['display_name'] as String,
      formats: (json['formats'] as List<dynamic>)
          .map((e) => FormatSpec.fromJson(e as Map<String, dynamic>))
          .toList(),
      kind: AgentKind.fromJson(json['kind'] as String),
      optionsSchema: json['options_schema'],
      platform: json['platform'] as String,
      quotaBuckets: (json['quota_buckets'] as List<dynamic>)
          .map((e) => QuotaBucket.fromJson(e as Map<String, dynamic>))
          .toList(),
      nativeSchedule: json['native_schedule'] == null
          ? null
          : NativeScheduleWindow.fromJson(
              json['native_schedule'] as Map<String, dynamic>,
            ),
    );

Map<String, dynamic> _$AgentDescriptorToJson(AgentDescriptor instance) =>
    <String, dynamic>{
      'brand_color': instance.brandColor,
      'capabilities': instance.capabilities
          .map((e) => _$CapabilityEnumMap[e]!)
          .toList(),
      'config_schema_version': instance.configSchemaVersion,
      'connect': instance.connect,
      'display_name': instance.displayName,
      'formats': instance.formats,
      'kind': _$AgentKindEnumMap[instance.kind]!,
      'native_schedule': instance.nativeSchedule,
      'options_schema': instance.optionsSchema,
      'platform': instance.platform,
      'quota_buckets': instance.quotaBuckets,
    };

const _$CapabilityEnumMap = {
  Capability.commenter: 'commenter',
  Capability.inboxReader: 'inbox_reader',
  Capability.metricsReader: 'metrics_reader',
  Capability.directSender: 'direct_sender',
  Capability.nativeScheduler: 'native_scheduler',
  Capability.senderDomainCheck: 'sender_domain_check',
  Capability.$unknown: r'$unknown',
};

const _$AgentKindEnumMap = {
  AgentKind.broadcast: 'broadcast',
  AgentKind.direct: 'direct',
  AgentKind.$unknown: r'$unknown',
};
