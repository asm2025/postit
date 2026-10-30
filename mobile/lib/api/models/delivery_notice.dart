// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'account_label.dart';
import 'delivery_id.dart';
import 'park_reason.dart';
import 'post_id.dart';
import 'publish_job_id.dart';

part 'delivery_notice.g.dart';

/// Payload of the delivery kinds.
@JsonSerializable()
class DeliveryNotice {
  const DeliveryNotice({
    required this.account,
    required this.deliveryId,
    required this.jobId,
    required this.postId,
    this.parkReason,
  });
  
  factory DeliveryNotice.fromJson(Map<String, Object?> json) => _$DeliveryNoticeFromJson(json);
  
  /// The target account.
  final AccountLabel account;

  /// The delivery.
  @JsonKey(name: 'delivery_id')
  final DeliveryId deliveryId;

  /// Its job.
  @JsonKey(name: 'job_id')
  final PublishJobId jobId;

  /// Why it is parked, for parked and stale notices.
  @JsonKey(name: 'park_reason')
  final ParkReason? parkReason;

  /// Its post.
  @JsonKey(name: 'post_id')
  final PostId postId;

  Map<String, Object?> toJson() => _$DeliveryNoticeToJson(this);
}
