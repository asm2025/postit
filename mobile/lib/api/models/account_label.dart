// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_id.dart';
import 'platform_id.dart';

part 'account_label.g.dart';

/// An account as a notification names it, so it still reads correctly after the account or.
/// the delegation is gone.
@JsonSerializable()
class AccountLabel {
  const AccountLabel({
    required this.accountId,
    required this.accountName,
    required this.platform,
  });
  
  factory AccountLabel.fromJson(Map<String, Object?> json) => _$AccountLabelFromJson(json);
  
  /// The account.
  @JsonKey(name: 'account_id')
  final AccountId accountId;

  /// Its display name when the notification was recorded.
  @JsonKey(name: 'account_name')
  final String accountName;

  /// Its platform.
  final PlatformId platform;

  Map<String, Object?> toJson() => _$AccountLabelToJson(this);
}
