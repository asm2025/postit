// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'role_dto.dart';
import 'status_dto.dart';

part 'patch_user_request.g.dart';

/// Exactly one of `status` or `role`.
@JsonSerializable()
class PatchUserRequest {
  const PatchUserRequest({
    this.role,
    this.status,
  });
  
  factory PatchUserRequest.fromJson(Map<String, Object?> json) => _$PatchUserRequestFromJson(json);
  
  /// The new role.
  final RoleDto? role;

  /// The new status.
  final StatusDto? status;

  Map<String, Object?> toJson() => _$PatchUserRequestToJson(this);
}
