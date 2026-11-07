// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'agent_kind.dart';
import 'capability.dart';
import 'connect_method.dart';
import 'format_spec.dart';
import 'native_schedule_window.dart';
import 'options_schema.dart';
import 'platform_id.dart';
import 'quota_bucket.dart';

part 'agent_descriptor.g.dart';

/// Everything the system knows about a platform, declared by its agent.
@JsonSerializable()
class AgentDescriptor {
  const AgentDescriptor({
    required this.brandColor,
    required this.capabilities,
    required this.configSchemaVersion,
    required this.connect,
    required this.displayName,
    required this.formats,
    required this.kind,
    required this.optionsSchema,
    required this.platform,
    required this.quotaBuckets,
    this.nativeSchedule,
  });
  
  factory AgentDescriptor.fromJson(Map<String, Object?> json) => _$AgentDescriptorFromJson(json);
  
  /// Brand color as `#RRGGBB`.
  @JsonKey(name: 'brand_color')
  final String brandColor;

  /// Optional abilities.
  final List<Capability> capabilities;

  /// Version of the agent's stored configuration shape.
  @JsonKey(name: 'config_schema_version')
  final int configSchemaVersion;

  /// How accounts are connected.
  final ConnectMethod connect;

  /// Name shown to people.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// Formats the agent can publish.
  final List<FormatSpec> formats;

  /// Broadcast or direct.
  final AgentKind kind;

  /// Native scheduling window, when the platform schedules posts itself.
  @JsonKey(name: 'native_schedule')
  final NativeScheduleWindow? nativeSchedule;

  /// Per-post options the agent understands.
  @JsonKey(name: 'options_schema')
  final OptionsSchema optionsSchema;

  /// Platform ID.
  final PlatformId platform;

  /// Quotas the platform enforces.
  @JsonKey(name: 'quota_buckets')
  final List<QuotaBucket> quotaBuckets;

  Map<String, Object?> toJson() => _$AgentDescriptorToJson(this);
}
