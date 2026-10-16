// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'user_ref.g.dart';

/// A user reference in the audit log. `deleted` is true for a pseudonym left by user.
/// deletion (a UUID v8, which postit never generates otherwise). `display_name` is the.
/// user's current name, or null for a pseudonym or a user no longer present.
@JsonSerializable()
class UserRef {
  const UserRef({
    required this.deleted,
    required this.id,
    this.displayName,
  });
  
  factory UserRef.fromJson(Map<String, Object?> json) => _$UserRefFromJson(json);
  
  /// Whether the user has been deleted.
  final bool deleted;

  /// The user's current name, when known.
  @JsonKey(name: 'display_name')
  final String? displayName;

  /// User ID, or the pseudonym that replaced it.
  final String id;

  Map<String, Object?> toJson() => _$UserRefToJson(this);
}
