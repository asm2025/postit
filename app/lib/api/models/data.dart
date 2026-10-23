// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'connect_kind_dto.dart';
import 'account_status_dto.dart';

part 'data.g.dart';

@JsonSerializable()
class Data {
  const Data({
    required this.createdAt,
    required this.displayName,
    required this.id,
    required this.platform,
    required this.remoteAccountId,
    required this.status,
    required this.testAccount,
    required this.updatedAt,
    this.avatarUrl,
    this.connect,
    this.lastCheckedAt,
  });
  
  factory Data.fromJson(Map<String, Object?> json) => _$DataFromJson(json);
  
  /// Profile picture, when the platform gives one.
  @JsonKey(name: 'avatar_url')
  final String? avatarUrl;

  /// How it is connected; absent when this build has no agent for the platform.
  final ConnectKindDto? connect;

  /// When it was connected.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// The name shown in postit.
  @JsonKey(name: 'display_name')
  final String displayName;

  /// Account ID.
  final String id;

  /// When the connection was last verified.
  @JsonKey(name: 'last_checked_at')
  final DateTime? lastCheckedAt;

  /// The platform's ID, such as `mastodon`.
  final String platform;

  /// The platform's ID for the account.
  @JsonKey(name: 'remote_account_id')
  final String remoteAccountId;

  /// Whether the account can be used.
  final AccountStatusDto status;

  /// Whether publishing to it needs no live confirmation.
  @JsonKey(name: 'test_account')
  final bool testAccount;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$DataToJson(this);
}
