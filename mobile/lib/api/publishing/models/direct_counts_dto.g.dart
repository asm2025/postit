// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'direct_counts_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DirectCountsDto _$DirectCountsDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'audience',
      'bounced',
      'failed',
      'queued',
      'sent',
      'skipped',
    ],
    disallowNullValues: const [
      'audience',
      'bounced',
      'failed',
      'queued',
      'sent',
      'skipped',
    ],
  );
  return DirectCountsDto(
    audience: DirectCountsDto._read_audience(json['audience']),
    bounced: DirectCountsDto._read_bounced(json['bounced']),
    failed: DirectCountsDto._read_failed(json['failed']),
    queued: DirectCountsDto._read_queued(json['queued']),
    sent: DirectCountsDto._read_sent(json['sent']),
    skipped: DirectCountsDto._read_skipped(json['skipped']),
    additionalProperties:
        DirectCountsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : DirectCountsDto._readExtras(
            DirectCountsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DirectCountsDtoToJson(DirectCountsDto instance) =>
    <String, dynamic>{
      'audience': ?DirectCountsDto._write_audience(instance.audience),
      'bounced': ?DirectCountsDto._write_bounced(instance.bounced),
      'failed': ?DirectCountsDto._write_failed(instance.failed),
      'queued': ?DirectCountsDto._write_queued(instance.queued),
      'sent': ?DirectCountsDto._write_sent(instance.sent),
      'skipped': ?DirectCountsDto._write_skipped(instance.skipped),
    };
