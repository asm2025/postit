// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'audit_event_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AuditEventDto _$AuditEventDtoFromJson(Map<String, dynamic> json) =>
    AuditEventDto(
      at: DateTime.parse(json['at'] as String),
      details: json['details'],
      id: json['id'] as String,
      kind: AuditEventKindDto.fromJson(json['kind'] as String),
      actor: json['actor'] == null
          ? null
          : UserRef.fromJson(json['actor'] as Map<String, dynamic>),
      ip: json['ip'] as String?,
      owner: json['owner'] == null
          ? null
          : UserRef.fromJson(json['owner'] as Map<String, dynamic>),
      requestId: json['request_id'] as String?,
      subject: json['subject'] == null
          ? null
          : UserRef.fromJson(json['subject'] as Map<String, dynamic>),
    );

Map<String, dynamic> _$AuditEventDtoToJson(AuditEventDto instance) =>
    <String, dynamic>{
      'actor': instance.actor,
      'at': instance.at.toIso8601String(),
      'details': instance.details,
      'id': instance.id,
      'ip': instance.ip,
      'kind': _$AuditEventKindDtoEnumMap[instance.kind]!,
      'owner': instance.owner,
      'request_id': instance.requestId,
      'subject': instance.subject,
    };

const _$AuditEventKindDtoEnumMap = {
  AuditEventKindDto.userProvisioned: 'user_provisioned',
  AuditEventKindDto.userApproved: 'user_approved',
  AuditEventKindDto.userDisabled: 'user_disabled',
  AuditEventKindDto.userEnabled: 'user_enabled',
  AuditEventKindDto.roleChanged: 'role_changed',
  AuditEventKindDto.bootstrapAdminGranted: 'bootstrap_admin_granted',
  AuditEventKindDto.userDeleted: 'user_deleted',
  AuditEventKindDto.emailSent: 'email_sent',
  AuditEventKindDto.emailDropped: 'email_dropped',
  AuditEventKindDto.emailFailed: 'email_failed',
  AuditEventKindDto.jobRetried: 'job_retried',
  AuditEventKindDto.jobCancelled: 'job_cancelled',
  AuditEventKindDto.jobDeleted: 'job_deleted',
  AuditEventKindDto.jobTriggered: 'job_triggered',
  AuditEventKindDto.$unknown: r'$unknown',
};
