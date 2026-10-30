// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delivery_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeliveryNotice _$DeliveryNoticeFromJson(Map<String, dynamic> json) =>
    DeliveryNotice(
      account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
      deliveryId: json['delivery_id'] as String,
      jobId: json['job_id'] as String,
      postId: json['post_id'] as String,
      parkReason: json['park_reason'] == null
          ? null
          : ParkReason.fromJson(json['park_reason'] as String),
    );

Map<String, dynamic> _$DeliveryNoticeToJson(DeliveryNotice instance) =>
    <String, dynamic>{
      'account': instance.account,
      'delivery_id': instance.deliveryId,
      'job_id': instance.jobId,
      'park_reason': _$ParkReasonEnumMap[instance.parkReason],
      'post_id': instance.postId,
    };

const _$ParkReasonEnumMap = {
  ParkReason.ownerDisabled: 'owner_disabled',
  ParkReason.accountNeedsReconnect: 'account_needs_reconnect',
  ParkReason.platformUnavailable: 'platform_unavailable',
  ParkReason.platformDisabled: 'platform_disabled',
  ParkReason.livePublishDisabled: 'live_publish_disabled',
  ParkReason.quotaExceeded: 'quota_exceeded',
  ParkReason.outcomeUnresolvable: 'outcome_unresolvable',
  ParkReason.$unknown: r'$unknown',
};
