// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'calendar_item_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CalendarItemDto _$CalendarItemDtoFromJson(Map<String, dynamic> json) =>
    CalendarItemDto(
      accountIds: (json['account_ids'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      jobId: json['job_id'] as String,
      kind: CalendarKindDto.fromJson(json['kind'] as String),
      postId: json['post_id'] as String,
      scheduledAt: DateTime.parse(json['scheduled_at'] as String),
      status: JobStatus.fromJson(json['status'] as String),
      textHead: json['text_head'] as String,
      campaignColor: json['campaign_color'] as String?,
      campaignId: json['campaign_id'] as String?,
    );

Map<String, dynamic> _$CalendarItemDtoToJson(CalendarItemDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'campaign_color': instance.campaignColor,
      'campaign_id': instance.campaignId,
      'job_id': instance.jobId,
      'kind': _$CalendarKindDtoEnumMap[instance.kind]!,
      'post_id': instance.postId,
      'scheduled_at': instance.scheduledAt.toIso8601String(),
      'status': _$JobStatusEnumMap[instance.status]!,
      'text_head': instance.textHead,
    };

const _$CalendarKindDtoEnumMap = {
  CalendarKindDto.job: 'job',
  CalendarKindDto.$unknown: r'$unknown',
};

const _$JobStatusEnumMap = {
  JobStatus.scheduled: 'scheduled',
  JobStatus.publishing: 'publishing',
  JobStatus.published: 'published',
  JobStatus.cancelled: 'cancelled',
  JobStatus.failed: 'failed',
  JobStatus.partial: 'partial',
  JobStatus.$unknown: r'$unknown',
};
