// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'lookup_user_dto.g.dart';

/// A user found by email.
@JsonSerializable()
class LookupUserDto {
  const LookupUserDto({
    required this.displayName,
    required this.id,
  });
  
  factory LookupUserDto.fromJson(Map<String, Object?> json) => _$LookupUserDtoFromJson(json);
  
  /// The user's current name.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// User ID; pass it as `delegate_id`.
  final String id;

  Map<String, Object?> toJson() => _$LookupUserDtoToJson(this);
}
