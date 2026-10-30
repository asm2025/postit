// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reconnect_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ReconnectNotice _$ReconnectNoticeFromJson(Map<String, dynamic> json) =>
    ReconnectNotice(
      account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
      grantId: json['grant_id'] as String,
      reconnectBy: DateTime.parse(json['reconnect_by'] as String),
      stage: json['stage'] == null
          ? null
          : ReminderStage.fromJson(json['stage'] as String),
    );

Map<String, dynamic> _$ReconnectNoticeToJson(ReconnectNotice instance) =>
    <String, dynamic>{
      'account': instance.account,
      'grant_id': instance.grantId,
      'reconnect_by': instance.reconnectBy.toIso8601String(),
      'stage': _$ReminderStageEnumMap[instance.stage],
    };

const _$ReminderStageEnumMap = {
  ReminderStage.sevenDays: 'seven_days',
  ReminderStage.oneDay: 'one_day',
  ReminderStage.$unknown: r'$unknown',
};
