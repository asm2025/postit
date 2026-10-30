// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'notification_kind.dart';
import 'severity.dart';

part 'notification_dto.g.dart';

/// One notification. `payload` is the object `kind` names. Labels in it (account name, platform, the other party's name) are the ones.
/// recorded with the notification, so an old row reads the same after a rename or a revoked.
/// grant. Clients must tolerate kinds added later.
@JsonSerializable()
class NotificationDto {
  const NotificationDto({
    required this.createdAt,
    required this.id,
    required this.kind,
    required this.payload,
    required this.severity,
    this.ownerId,
    this.readAt,
  });
  
  factory NotificationDto.fromJson(Map<String, Object?> json) => _$NotificationDtoFromJson(json);
  
  /// When it was recorded.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// The notification.
  final String id;

  /// What happened.
  final NotificationKind kind;

  /// The workspace owner the notice is about; absent for deployment notices. A delegate.
  /// uses it to tell the workspaces they act in apart.
  @JsonKey(name: 'owner_id')
  final String? ownerId;
  final dynamic payload;

  /// When the caller read it.
  @JsonKey(name: 'read_at')
  final DateTime? readAt;

  /// How urgent it is.
  final Severity severity;

  Map<String, Object?> toJson() => _$NotificationDtoToJson(this);
}
