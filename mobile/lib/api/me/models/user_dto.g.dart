// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserDto _$UserDtoFromJson(Map<String, dynamic> json) {
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
  return UserDto(
    approvedAt: UserDto._read_approvedAt(json['approved_at']),
    approvedBy: json['approved_by'] as String?,
    createdAt: UserDto._read_createdAt(json['created_at']),
    displayName: json['display_name'] as String,
    email: json['email'] as String?,
    emailVerified: json['email_verified'] as bool,
    id: json['id'] as String,
    lastSeenAt: UserDto._read_lastSeenAt(json['last_seen_at']),
    role: UserDto._read_role(json['role']),
    status: UserDto._read_status(json['status']),
    updatedAt: UserDto._read_updatedAt(json['updated_at']),
    additionalProperties: UserDto._extras(json, 'additionalProperties') == null
        ? const {}
        : UserDto._readExtras(UserDto._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$UserDtoToJson(UserDto instance) => <String, dynamic>{
  'approved_at': ?UserDto._write_approvedAt(instance.approvedAt),
  'approved_by': ?instance.approvedBy,
  'created_at': ?UserDto._write_createdAt(instance.createdAt),
  'display_name': instance.displayName,
  'email': ?instance.email,
  'email_verified': instance.emailVerified,
  'id': instance.id,
  'last_seen_at': ?UserDto._write_lastSeenAt(instance.lastSeenAt),
  'role': ?UserDto._write_role(instance.role),
  'status': ?UserDto._write_status(instance.status),
  'updated_at': ?UserDto._write_updatedAt(instance.updatedAt),
};
