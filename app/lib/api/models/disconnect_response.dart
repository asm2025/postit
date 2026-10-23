// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'disconnect_response.g.dart';

/// What disconnecting an account did.
@JsonSerializable()
class DisconnectResponse {
  const DisconnectResponse({
    required this.cancelledDeliveries,
    required this.nativeScheduledRemaining,
  });
  
  factory DisconnectResponse.fromJson(Map<String, Object?> json) => _$DisconnectResponseFromJson(json);
  
  /// Pending and parked deliveries that were cancelled.
  @JsonKey(name: 'cancelled_deliveries')
  final int cancelledDeliveries;

  /// Deliveries still held for a later time on the platform's side.
  @JsonKey(name: 'native_scheduled_remaining')
  final int nativeScheduledRemaining;

  Map<String, Object?> toJson() => _$DisconnectResponseToJson(this);
}
