// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_user_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchUserRequest _$PatchUserRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(json, allowedKeys: const ['role', 'status']);
  return PatchUserRequest(
    role: PatchUserRequest._read_role(json['role']),
    status: PatchUserRequest._read_status(json['status']),
  );
}

Map<String, dynamic> _$PatchUserRequestToJson(PatchUserRequest instance) =>
    <String, dynamic>{
      'role': ?PatchUserRequest._write_role(instance.role),
      'status': ?PatchUserRequest._write_status(instance.status),
    };
