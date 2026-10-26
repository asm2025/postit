// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'delegation_user_dto.dart';
import 'ended_reason_dto.dart';
import 'level_dto.dart';

part 'delegation_dto.g.dart';

/// A delegation: the owner lets the delegate act in the owner's workspace.
@JsonSerializable()
class DelegationDto {
  const DelegationDto({
    required this.accountIds,
    required this.allAccounts,
    required this.createdAt,
    required this.delegate,
    required this.id,
    required this.level,
    required this.owner,
    required this.updatedAt,
    this.createdBy,
    this.endedAt,
    this.endedReason,
    this.expiresAt,
    this.updatedBy,
  });
  
  factory DelegationDto.fromJson(Map<String, Object?> json) => _$DelegationDtoFromJson(json);
  
  /// The accounts the delegate reaches when `all_accounts` is false (possibly none: posts.
  /// and media only). Empty when `all_accounts` is true.
  @JsonKey(name: 'account_ids')
  final List<String> accountIds;

  /// Whether the delegate reaches every account; when false, only `account_ids`.
  @JsonKey(name: 'all_accounts')
  final bool allAccounts;

  /// When it was granted.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// Who granted it.
  @JsonKey(name: 'created_by')
  final String? createdBy;

  /// The user it lets in.
  final DelegationUserDto delegate;

  /// When it ended; null while live.
  @JsonKey(name: 'ended_at')
  final DateTime? endedAt;

  /// Why it ended; null while live.
  @JsonKey(name: 'ended_reason')
  final EndedReasonDto? endedReason;

  /// When it stops working; null for never.
  @JsonKey(name: 'expires_at')
  final DateTime? expiresAt;

  /// Delegation ID.
  final String id;

  /// What the delegate may do.
  final LevelDto level;

  /// The user whose workspace it opens.
  final DelegationUserDto owner;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  /// Who last changed it.
  @JsonKey(name: 'updated_by')
  final String? updatedBy;

  Map<String, Object?> toJson() => _$DelegationDtoToJson(this);
}
