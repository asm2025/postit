// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'direct_counts_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DirectCountsDto _$DirectCountsDtoFromJson(Map<String, dynamic> json) =>
    DirectCountsDto(
      audience: (json['audience'] as num).toInt(),
      bounced: (json['bounced'] as num).toInt(),
      failed: (json['failed'] as num).toInt(),
      queued: (json['queued'] as num).toInt(),
      sent: (json['sent'] as num).toInt(),
      skipped: (json['skipped'] as num).toInt(),
    );

Map<String, dynamic> _$DirectCountsDtoToJson(DirectCountsDto instance) =>
    <String, dynamic>{
      'audience': instance.audience,
      'bounced': instance.bounced,
      'failed': instance.failed,
      'queued': instance.queued,
      'sent': instance.sent,
      'skipped': instance.skipped,
    };
