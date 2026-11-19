// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'notification_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

NotificationDto _$NotificationDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['created_at', 'id', 'kind', 'payload', 'severity'],
    disallowNullValues: const ['created_at', 'id', 'kind', 'severity'],
  );
  return NotificationDto(
    createdAt: NotificationDto._read_createdAt(json['created_at']),
    id: json['id'] as String,
    kind: NotificationDto._read_kind(json['kind']),
    ownerId: json['owner_id'] as String?,
    payload: NotificationDto._read_payload(json['payload']),
    readAt: NotificationDto._read_readAt(json['read_at']),
    severity: NotificationDto._read_severity(json['severity']),
    additionalProperties:
        NotificationDto._extras(json, 'additionalProperties') == null
        ? const {}
        : NotificationDto._readExtras(
            NotificationDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$NotificationDtoToJson(NotificationDto instance) =>
    <String, dynamic>{
      'created_at': ?NotificationDto._write_createdAt(instance.createdAt),
      'id': instance.id,
      'kind': ?NotificationDto._write_kind(instance.kind),
      'owner_id': ?instance.ownerId,
      'payload': NotificationDto._write_payload(instance.payload),
      'read_at': ?NotificationDto._write_readAt(instance.readAt),
      'severity': ?NotificationDto._write_severity(instance.severity),
    };
