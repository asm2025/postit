// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'quota_costs.dart';
import 'quota_period.dart';
import 'quota_scope.dart';

part 'quota_bucket.g.dart';

/// A calendar-aligned quota the platform enforces.
@JsonSerializable()
class QuotaBucket {
  const QuotaBucket({
    required this.costs,
    required this.defaultLimit,
    required this.id,
    required this.period,
    required this.resetTimeZone,
    required this.scope,
  });
  
  factory QuotaBucket.fromJson(Map<String, Object?> json) => _$QuotaBucketFromJson(json);
  
  /// What each operation costs.
  final QuotaCosts costs;

  /// Allowance per period when the deployment sets no override.
  @JsonKey(name: 'default_limit')
  final int defaultLimit;

  /// Bucket name, unique within the agent.
  final String id;

  /// How often it resets.
  final QuotaPeriod period;

  /// IANA time zone the period is aligned to. Stored, not resolved, here.
  @JsonKey(name: 'reset_time_zone')
  final String resetTimeZone;

  /// What the bucket is shared by.
  final QuotaScope scope;

  Map<String, Object?> toJson() => _$QuotaBucketToJson(this);
}
