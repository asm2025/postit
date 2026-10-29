// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'notification_kind.dart';

part 'kind_switch_dto.g.dart';

/// How one kind of notification reaches the caller.
@JsonSerializable()
class KindSwitchDto {
  const KindSwitchDto({
    required this.kind,
    required this.push,
    this.email,
  });
  
  factory KindSwitchDto.fromJson(Map<String, Object?> json) => _$KindSwitchDtoFromJson(json);
  
  /// Whether this kind is emailed; null for kinds that are never emailed.
  final bool? email;

  /// The notification kind.
  final NotificationKind kind;

  /// Whether this kind is pushed to the caller's devices.
  final bool push;

  Map<String, Object?> toJson() => _$KindSwitchDtoToJson(this);
}
