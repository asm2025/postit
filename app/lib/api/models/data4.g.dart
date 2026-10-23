// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'data4.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Data4 _$Data4FromJson(Map<String, dynamic> json) => Data4(
  createdAt: DateTime.parse(json['created_at'] as String),
  displayName: json['display_name'] as String,
  emailVerified: json['email_verified'] as bool,
  id: json['id'] as String,
  role: RoleDto.fromJson(json['role'] as String),
  status: StatusDto.fromJson(json['status'] as String),
  updatedAt: DateTime.parse(json['updated_at'] as String),
  approvedAt: json['approved_at'] == null
      ? null
      : DateTime.parse(json['approved_at'] as String),
  approvedBy: json['approved_by'] as String?,
  email: json['email'] as String?,
  lastSeenAt: json['last_seen_at'] == null
      ? null
      : DateTime.parse(json['last_seen_at'] as String),
);

Map<String, dynamic> _$Data4ToJson(Data4 instance) => <String, dynamic>{
  'approved_at': instance.approvedAt?.toIso8601String(),
  'approved_by': instance.approvedBy,
  'created_at': instance.createdAt.toIso8601String(),
  'display_name': instance.displayName,
  'email': instance.email,
  'email_verified': instance.emailVerified,
  'id': instance.id,
  'last_seen_at': instance.lastSeenAt?.toIso8601String(),
  'role': _$RoleDtoEnumMap[instance.role]!,
  'status': _$StatusDtoEnumMap[instance.status]!,
  'updated_at': instance.updatedAt.toIso8601String(),
};

const _$RoleDtoEnumMap = {
  RoleDto.admin: 'admin',
  RoleDto.member: 'member',
  RoleDto.$unknown: r'$unknown',
};

const _$StatusDtoEnumMap = {
  StatusDto.pending: 'pending',
  StatusDto.active: 'active',
  StatusDto.disabled: 'disabled',
  StatusDto.deleting: 'deleting',
  StatusDto.$unknown: r'$unknown',
};
