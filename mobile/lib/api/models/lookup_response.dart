// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'lookup_user_dto.dart';

part 'lookup_response.g.dart';

/// The result of a user lookup.
@JsonSerializable()
class LookupResponse {
  const LookupResponse({
    this.user,
  });
  
  factory LookupResponse.fromJson(Map<String, Object?> json) => _$LookupResponseFromJson(json);
  
  /// The one matching user, or null.
  final LookupUserDto? user;

  Map<String, Object?> toJson() => _$LookupResponseToJson(this);
}
