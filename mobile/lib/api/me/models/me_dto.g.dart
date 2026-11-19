// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'me_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MeDto _$MeDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'created_at',
      'display_name',
      'email_verified',
      'id',
      'role',
      'status',
      'updated_at',
    ],
    disallowNullValues: const [
      'created_at',
      'display_name',
      'email_verified',
      'id',
      'role',
      'status',
      'updated_at',
    ],
  );
  return MeDto(
    approvedAt: MeDto._read_approvedAt(json['approved_at']),
    approvedBy: json['approved_by'] as String?,
    createdAt: MeDto._read_createdAt(json['created_at']),
    displayName: json['display_name'] as String,
    email: json['email'] as String?,
    emailVerified: json['email_verified'] as bool,
    id: json['id'] as String,
    lastSeenAt: MeDto._read_lastSeenAt(json['last_seen_at']),
    role: MeDto._read_role(json['role']),
    status: MeDto._read_status(json['status']),
    updatedAt: MeDto._read_updatedAt(json['updated_at']),
    accountUrl: json['account_url'] as String?,
    additionalProperties: MeDto._extras(json, 'additionalProperties') == null
        ? const {}
        : MeDto._readExtras(MeDto._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$MeDtoToJson(MeDto instance) => <String, dynamic>{
  'approved_at': ?MeDto._write_approvedAt(instance.approvedAt),
  'approved_by': ?instance.approvedBy,
  'created_at': ?MeDto._write_createdAt(instance.createdAt),
  'display_name': instance.displayName,
  'email': ?instance.email,
  'email_verified': instance.emailVerified,
  'id': instance.id,
  'last_seen_at': ?MeDto._write_lastSeenAt(instance.lastSeenAt),
  'role': ?MeDto._write_role(instance.role),
  'status': ?MeDto._write_status(instance.status),
  'updated_at': ?MeDto._write_updatedAt(instance.updatedAt),
  'account_url': ?instance.accountUrl,
};
