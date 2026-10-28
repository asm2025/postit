// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'level_dto.dart';

part 'change_delegation_request.g.dart';

/// Changes to a live delegation. Absent fields stay as they are.
@JsonSerializable()
class ChangeDelegationRequest {
  const ChangeDelegationRequest({
    this.accountIds,
    this.allAccounts,
    this.expiresAt,
    this.level,
  });
  
  factory ChangeDelegationRequest.fromJson(Map<String, Object?> json) => _$ChangeDelegationRequestFromJson(json);
  
  /// A new account limit, replacing the old one; an empty list means posts and media only.
  @JsonKey(name: 'account_ids')
  final List<String>? accountIds;

  /// `true` lifts the account limit. `false` is refused: send `account_ids` instead.
  /// Cannot be sent together with `account_ids`.
  @JsonKey(name: 'all_accounts')
  final bool? allAccounts;

  /// A new expiry in the future, or `null` to clear it. Absent leaves it unchanged, so a.
  /// client that always serializes nulls must send the current value to keep it.
  @JsonKey(name: 'expires_at')
  final DateTime? expiresAt;

  /// A new level.
  final LevelDto? level;

  Map<String, Object?> toJson() => _$ChangeDelegationRequestToJson(this);
}
