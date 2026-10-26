// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'status_dto.dart';

part 'delegation_user_dto.g.dart';

/// One side of a delegation: a user by ID, current name, and status.
@JsonSerializable()
class DelegationUserDto {
  const DelegationUserDto({
    required this.displayName,
    required this.id,
    required this.status,
  });
  
  factory DelegationUserDto.fromJson(Map<String, Object?> json) => _$DelegationUserDtoFromJson(json);
  
  /// The user's current name.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// User ID.
  final String id;

  /// The user's account status. A grant only works while both users are `active`.
  final StatusDto status;

  Map<String, Object?> toJson() => _$DelegationUserDtoToJson(this);
}
