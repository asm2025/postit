// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'calendar_item_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CalendarItemDto _$CalendarItemDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'account_ids',
      'job_id',
      'kind',
      'post_id',
      'scheduled_at',
      'status',
      'text_head',
    ],
    disallowNullValues: const [
      'account_ids',
      'job_id',
      'kind',
      'post_id',
      'scheduled_at',
      'status',
      'text_head',
    ],
  );
  return CalendarItemDto(
    accountIds: (json['account_ids'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    campaignColor: json['campaign_color'] as String?,
    campaignId: json['campaign_id'] as String?,
    jobId: json['job_id'] as String,
    kind: CalendarItemDto._read_kind(json['kind']),
    postId: json['post_id'] as String,
    scheduledAt: CalendarItemDto._read_scheduledAt(json['scheduled_at']),
    status: CalendarItemDto._read_status(json['status']),
    textHead: json['text_head'] as String,
    additionalProperties:
        CalendarItemDto._extras(json, 'additionalProperties') == null
        ? const {}
        : CalendarItemDto._readExtras(
            CalendarItemDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CalendarItemDtoToJson(CalendarItemDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'campaign_color': ?instance.campaignColor,
      'campaign_id': ?instance.campaignId,
      'job_id': instance.jobId,
      'kind': ?CalendarItemDto._write_kind(instance.kind),
      'post_id': instance.postId,
      'scheduled_at': ?CalendarItemDto._write_scheduledAt(instance.scheduledAt),
      'status': ?CalendarItemDto._write_status(instance.status),
      'text_head': instance.textHead,
    };
