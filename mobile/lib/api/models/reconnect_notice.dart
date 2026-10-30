// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_label.dart';
import 'grant_id.dart';
import 'reminder_stage.dart';

part 'reconnect_notice.g.dart';

/// Payload of the reconnect-by kinds; one per account.
@JsonSerializable()
class ReconnectNotice {
  const ReconnectNotice({
    required this.account,
    required this.grantId,
    required this.reconnectBy,
    this.stage,
  });
  
  factory ReconnectNotice.fromJson(Map<String, Object?> json) => _$ReconnectNoticeFromJson(json);
  
  /// The account.
  final AccountLabel account;

  /// The grant that needs reconnecting.
  @JsonKey(name: 'grant_id')
  final GrantId grantId;

  /// The date the platform stops honoring the connection.
  @JsonKey(name: 'reconnect_by')
  final DateTime reconnectBy;

  /// The reminder stage; `None` once the date has passed.
  final ReminderStage? stage;

  Map<String, Object?> toJson() => _$ReconnectNoticeToJson(this);
}
