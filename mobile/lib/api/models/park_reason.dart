// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why a delivery is waiting for something outside the engine.
@JsonEnum()
enum ParkReason {
  @JsonValue('owner_disabled')
  ownerDisabled('owner_disabled'),
  @JsonValue('account_needs_reconnect')
  accountNeedsReconnect('account_needs_reconnect'),
  @JsonValue('platform_unavailable')
  platformUnavailable('platform_unavailable'),
  @JsonValue('platform_disabled')
  platformDisabled('platform_disabled'),
  @JsonValue('live_publish_disabled')
  livePublishDisabled('live_publish_disabled'),
  @JsonValue('quota_exceeded')
  quotaExceeded('quota_exceeded'),
  @JsonValue('outcome_unresolvable')
  outcomeUnresolvable('outcome_unresolvable'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const ParkReason(this.json);

  factory ParkReason.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<ParkReason> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
