// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'level_dto.dart';

part 'grant_delegation_request.g.dart';

/// A new delegation in the caller's workspace.
@JsonSerializable()
class GrantDelegationRequest {
  const GrantDelegationRequest({
    required this.delegateId,
    required this.level,
    this.accountIds,
    this.expiresAt,
  });
  
  factory GrantDelegationRequest.fromJson(Map<String, Object?> json) => _$GrantDelegationRequestFromJson(json);
  
  /// Limit the grant to these of the caller's connected accounts; an empty list means posts.
  /// and media only. Absent means every account.
  @JsonKey(name: 'account_ids')
  final List<String>? accountIds;

  /// The user to let in: an `active` user other than the caller (see `/users/lookup`).
  @JsonKey(name: 'delegate_id')
  final String delegateId;

  /// When it stops working, in the future; absent or null for never.
  @JsonKey(name: 'expires_at')
  final DateTime? expiresAt;

  /// What they may do.
  final LevelDto level;

  Map<String, Object?> toJson() => _$GrantDelegationRequestToJson(this);
}
