// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'vapid_key_dto.g.dart';

/// The deployment's VAPID public key, for `PushManager.subscribe({ applicationServerKey })`.
@JsonSerializable()
class VapidKeyDto {
  const VapidKeyDto({
    required this.key,
  });
  
  factory VapidKeyDto.fromJson(Map<String, Object?> json) => _$VapidKeyDtoFromJson(json);
  
  /// The uncompressed P-256 public key, base64url without padding.
  final String key;

  Map<String, Object?> toJson() => _$VapidKeyDtoToJson(this);
}
