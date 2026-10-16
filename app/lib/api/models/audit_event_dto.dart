// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'audit_event_kind_dto.dart';
import 'user_ref.dart';

part 'audit_event_dto.g.dart';

/// One entry of the audit log.
@JsonSerializable()
class AuditEventDto {
  const AuditEventDto({
    required this.at,
    required this.details,
    required this.id,
    required this.kind,
    this.actor,
    this.ip,
    this.owner,
    this.requestId,
    this.subject,
  });
  
  factory AuditEventDto.fromJson(Map<String, Object?> json) => _$AuditEventDtoFromJson(json);
  
  /// Who performed the action; none for system actions.
  final UserRef? actor;

  /// When the event was recorded.
  final DateTime at;

  /// Event details. Every `*_user_id` value is a `UserRef` object.
  final dynamic details;

  /// Event ID.
  final String id;

  /// Client IP, until it is cleared by retention.
  final String? ip;

  /// The event kind.
  final AuditEventKindDto kind;

  /// Owner of the workspace the action touched.
  final UserRef? owner;

  /// `X-Request-Id` of the request that caused the event.
  @JsonKey(name: 'request_id')
  final String? requestId;

  /// Who the action was about.
  final UserRef? subject;

  Map<String, Object?> toJson() => _$AuditEventDtoToJson(this);
}
