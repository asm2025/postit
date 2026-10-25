// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'role_dto.dart';
import 'status_dto.dart';

part 'me_dto.g.dart';

/// The signed-in user.
@JsonSerializable()
class MeDto {
  const MeDto({
    required this.createdAt,
    required this.displayName,
    required this.emailVerified,
    required this.id,
    required this.role,
    required this.status,
    required this.updatedAt,
    this.approvedAt,
    this.approvedBy,
    this.email,
    this.lastSeenAt,
    this.accountUrl,
  });
  
  factory MeDto.fromJson(Map<String, Object?> json) => _$MeDtoFromJson(json);
  
  /// When an admin approved the user.
  @JsonKey(name: 'approved_at')
  final DateTime? approvedAt;

  /// The admin who approved the user.
  @JsonKey(name: 'approved_by')
  final String? approvedBy;

  /// When the user was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Name shown in the UI.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// Email address from the identity provider, when it supplied one.
  final String? email;

  /// Whether the provider verified the email address.
  @JsonKey(name: 'email_verified')
  final bool emailVerified;

  /// User ID.
  final String id;

  /// When the user last made an authenticated request.
  @JsonKey(name: 'last_seen_at')
  final DateTime? lastSeenAt;

  /// The user's role.
  final RoleDto role;

  /// The user's account status.
  final StatusDto status;

  /// When the user was last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  /// The identity provider's account page (`auth.oidc.account_url`), when configured.
  @JsonKey(name: 'account_url')
  final String? accountUrl;

  Map<String, Object?> toJson() => _$MeDtoToJson(this);
}
