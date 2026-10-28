// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'delegation_user_dto.dart';
import 'level_dto.dart';

part 'workspace_dto.g.dart';

/// A workspace the caller can act in: their own, or one a live grant opens.
@JsonSerializable()
class WorkspaceDto {
  const WorkspaceDto({
    required this.own,
    required this.owner,
    this.accountIds,
    this.allAccounts,
    this.delegationId,
    this.level,
  });
  
  factory WorkspaceDto.fromJson(Map<String, Object?> json) => _$WorkspaceDtoFromJson(json);
  
  /// The accounts the caller reaches when `all_accounts` is false; null for the caller's.
  /// own.
  @JsonKey(name: 'account_ids')
  final List<String>? accountIds;

  /// Whether the caller reaches every account here; null for the caller's own.
  @JsonKey(name: 'all_accounts')
  final bool? allAccounts;

  /// The grant that opens it; null for the caller's own.
  @JsonKey(name: 'delegation_id')
  final String? delegationId;

  /// What the caller may do here; null for the caller's own (everything).
  final LevelDto? level;

  /// Whether this is the caller's own workspace.
  final bool own;

  /// The workspace's owner. Send their ID in `X-Postit-Act-As` to act here (not for the.
  /// caller's own workspace).
  final DelegationUserDto owner;

  Map<String, Object?> toJson() => _$WorkspaceDtoToJson(this);
}
