// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'calendar_kind_dto.dart';
import 'job_status.dart';

part 'calendar_item_dto.g.dart';

/// One scheduled job on the calendar.
@JsonSerializable()
class CalendarItemDto {
  const CalendarItemDto({
    required this.accountIds,
    required this.jobId,
    required this.kind,
    required this.postId,
    required this.scheduledAt,
    required this.status,
    required this.textHead,
    this.campaignColor,
    this.campaignId,
  });
  
  factory CalendarItemDto.fromJson(Map<String, Object?> json) => _$CalendarItemDtoFromJson(json);
  
  /// The accounts it goes to that the caller can see.
  @JsonKey(name: 'account_ids')
  final List<String> accountIds;

  /// The campaign's display color.
  @JsonKey(name: 'campaign_color')
  final String? campaignColor;

  /// The post's campaign.
  @JsonKey(name: 'campaign_id')
  final String? campaignId;

  /// The job.
  @JsonKey(name: 'job_id')
  final String jobId;

  /// What the entry is.
  final CalendarKindDto kind;

  /// The post it publishes.
  @JsonKey(name: 'post_id')
  final String postId;

  /// When it is due.
  @JsonKey(name: 'scheduled_at')
  final DateTime scheduledAt;

  /// Where it is.
  final JobStatus status;

  /// The first line of the post's text, cut to 80 graphemes.
  @JsonKey(name: 'text_head')
  final String textHead;

  Map<String, Object?> toJson() => _$CalendarItemDtoToJson(this);
}
