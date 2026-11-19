// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publish_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishJobDto _$PublishJobDtoFromJson(Map<String, dynamic> json) {
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
      'deliveries',
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
      'deliveries',
    ],
  );
  return PublishJobDto(
    cancelledAt: PublishJobDto._read_cancelledAt(json['cancelled_at']),
    createdAt: PublishJobDto._read_createdAt(json['created_at']),
    createdBy: json['created_by'] as String?,
    dryRun: json['dry_run'] as bool,
    id: json['id'] as String,
    nativeSchedule: json['native_schedule'] as bool,
    postId: json['post_id'] as String,
    postVersion: PublishJobDto._read_postVersion(json['post_version']),
    scheduledAt: PublishJobDto._read_scheduledAt(json['scheduled_at']),
    status: PublishJobDto._read_status(json['status']),
    version: PublishJobDto._read_version(json['version']),
    deliveries: (json['deliveries'] as List<dynamic>)
        .map((e) => DeliveryDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        PublishJobDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PublishJobDto._readExtras(
            PublishJobDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PublishJobDtoToJson(PublishJobDto instance) =>
    <String, dynamic>{
      'cancelled_at': ?PublishJobDto._write_cancelledAt(instance.cancelledAt),
      'created_at': ?PublishJobDto._write_createdAt(instance.createdAt),
      'created_by': ?instance.createdBy,
      'dry_run': instance.dryRun,
      'id': instance.id,
      'native_schedule': instance.nativeSchedule,
      'post_id': instance.postId,
      'post_version': ?PublishJobDto._write_postVersion(instance.postVersion),
      'scheduled_at': ?PublishJobDto._write_scheduledAt(instance.scheduledAt),
      'status': ?PublishJobDto._write_status(instance.status),
      'version': ?PublishJobDto._write_version(instance.version),
      'deliveries': instance.deliveries.map((e) => e.toJson()).toList(),
    };
