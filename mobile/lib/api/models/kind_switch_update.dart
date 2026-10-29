// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'notification_kind.dart';

part 'kind_switch_update.g.dart';

/// Changes one kind's switches.
@JsonSerializable()
class KindSwitchUpdate {
  const KindSwitchUpdate({
    required this.kind,
    this.email,
    this.push,
  });
  
  factory KindSwitchUpdate.fromJson(Map<String, Object?> json) => _$KindSwitchUpdateFromJson(json);
  
  /// Email on or off; null or absent leaves the stored choice. Rejected with.
  /// `validation_failed` for kinds that are never emailed.
  final bool? email;

  /// The notification kind to change.
  final NotificationKind kind;

  /// Push on or off; null or absent leaves the stored choice.
  final bool? push;

  Map<String, Object?> toJson() => _$KindSwitchUpdateToJson(this);
}
