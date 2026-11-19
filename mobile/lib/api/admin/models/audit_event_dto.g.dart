// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'audit_event_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AuditEventDto _$AuditEventDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['at', 'details', 'id', 'kind'],
    disallowNullValues: const ['at', 'details', 'id', 'kind'],
  );
  return AuditEventDto(
    actor: json['actor'] == null
        ? null
        : UserRef.fromJson(json['actor'] as Map<String, dynamic>),
    at: AuditEventDto._read_at(json['at']),
    details: AuditEventDtoDetails.fromJson(
      json['details'] as Map<String, dynamic>,
    ),
    id: json['id'] as String,
    ip: json['ip'] as String?,
    kind: AuditEventDto._read_kind(json['kind']),
    owner: json['owner'] == null
        ? null
        : UserRef.fromJson(json['owner'] as Map<String, dynamic>),
    requestId: json['request_id'] as String?,
    subject: json['subject'] == null
        ? null
        : UserRef.fromJson(json['subject'] as Map<String, dynamic>),
    additionalProperties:
        AuditEventDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AuditEventDto._readExtras(
            AuditEventDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AuditEventDtoToJson(AuditEventDto instance) =>
    <String, dynamic>{
      'actor': ?instance.actor?.toJson(),
      'at': ?AuditEventDto._write_at(instance.at),
      'details': instance.details.toJson(),
      'id': instance.id,
      'ip': ?instance.ip,
      'kind': ?AuditEventDto._write_kind(instance.kind),
      'owner': ?instance.owner?.toJson(),
      'request_id': ?instance.requestId,
      'subject': ?instance.subject?.toJson(),
    };
