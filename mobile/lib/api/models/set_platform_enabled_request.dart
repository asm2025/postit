// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'set_platform_enabled_request.g.dart';

/// Switching a platform on or off.
@JsonSerializable()
class SetPlatformEnabledRequest {
  const SetPlatformEnabledRequest({
    required this.enabled,
  });
  
  factory SetPlatformEnabledRequest.fromJson(Map<String, Object?> json) => _$SetPlatformEnabledRequestFromJson(json);
  
  /// Whether the platform is switched on.
  final bool enabled;

  Map<String, Object?> toJson() => _$SetPlatformEnabledRequestToJson(this);
}
