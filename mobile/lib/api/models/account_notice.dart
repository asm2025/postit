// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_label.dart';

part 'account_notice.g.dart';

/// Payload of the account kinds.
@JsonSerializable()
class AccountNotice {
  const AccountNotice({
    required this.account,
  });
  
  factory AccountNotice.fromJson(Map<String, Object?> json) => _$AccountNoticeFromJson(json);
  
  /// The account.
  final AccountLabel account;

  Map<String, Object?> toJson() => _$AccountNoticeToJson(this);
}
