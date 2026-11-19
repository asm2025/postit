// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publish_job_summary_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishJobSummaryDto _$PublishJobSummaryDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'created_at',
      'dry_run',
      'id',
      'native_schedule',
      'post_id',
      'post_version',
      'scheduled_at',
      'status',
      'version',
    ],
    disallowNullValues: const [
      'created_at',
      'dry_run',
      'id',
      'native_schedule',
      'post_id',
      'post_version',
      'scheduled_at',
      'status',
      'version',
    ],
  );
  return PublishJobSummaryDto(
    cancelledAt: PublishJobSummaryDto._read_cancelledAt(json['cancelled_at']),
    createdAt: PublishJobSummaryDto._read_createdAt(json['created_at']),
    createdBy: json['created_by'] as String?,
    dryRun: json['dry_run'] as bool,
    id: json['id'] as String,
    nativeSchedule: json['native_schedule'] as bool,
    postId: json['post_id'] as String,
    postVersion: PublishJobSummaryDto._read_postVersion(json['post_version']),
    scheduledAt: PublishJobSummaryDto._read_scheduledAt(json['scheduled_at']),
    status: PublishJobSummaryDto._read_status(json['status']),
    version: PublishJobSummaryDto._read_version(json['version']),
    additionalProperties:
        PublishJobSummaryDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PublishJobSummaryDto._readExtras(
            PublishJobSummaryDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PublishJobSummaryDtoToJson(
  PublishJobSummaryDto instance,
) => <String, dynamic>{
  'cancelled_at': ?PublishJobSummaryDto._write_cancelledAt(
    instance.cancelledAt,
  ),
  'created_at': ?PublishJobSummaryDto._write_createdAt(instance.createdAt),
  'created_by': ?instance.createdBy,
  'dry_run': instance.dryRun,
  'id': instance.id,
  'native_schedule': instance.nativeSchedule,
  'post_id': instance.postId,
  'post_version': ?PublishJobSummaryDto._write_postVersion(
    instance.postVersion,
  ),
  'scheduled_at': ?PublishJobSummaryDto._write_scheduledAt(
    instance.scheduledAt,
  ),
  'status': ?PublishJobSummaryDto._write_status(instance.status),
  'version': ?PublishJobSummaryDto._write_version(instance.version),
};
