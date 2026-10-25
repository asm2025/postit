// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_user_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchUserRequest _$PatchUserRequestFromJson(Map<String, dynamic> json) =>
    PatchUserRequest(
      role: json['role'] == null
          ? null
          : RoleDto.fromJson(json['role'] as String),
      status: json['status'] == null
          ? null
          : StatusDto.fromJson(json['status'] as String),
    );

Map<String, dynamic> _$PatchUserRequestToJson(PatchUserRequest instance) =>
    <String, dynamic>{
      'role': _$RoleDtoEnumMap[instance.role],
      'status': _$StatusDtoEnumMap[instance.status],
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
