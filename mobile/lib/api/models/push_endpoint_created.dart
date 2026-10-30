// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'push_endpoint_created.g.dart';

/// The registered device.
@JsonSerializable()
class PushEndpointCreated {
  const PushEndpointCreated({
    required this.id,
  });
  
  factory PushEndpointCreated.fromJson(Map<String, Object?> json) => _$PushEndpointCreatedFromJson(json);
  
  /// The device's ID, for `DELETE /api/v1/me/push-endpoints/{id}`.
  final String id;

  Map<String, Object?> toJson() => _$PushEndpointCreatedToJson(this);
}
