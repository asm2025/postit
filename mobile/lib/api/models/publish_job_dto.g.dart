// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publish_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishJobDto _$PublishJobDtoFromJson(Map<String, dynamic> json) =>
    PublishJobDto(
      createdAt: DateTime.parse(json['created_at'] as String),
      dryRun: json['dry_run'] as bool,
      id: json['id'] as String,
      nativeSchedule: json['native_schedule'] as bool,
      postId: json['post_id'] as String,
      postVersion: (json['post_version'] as num).toInt(),
      scheduledAt: DateTime.parse(json['scheduled_at'] as String),
      status: JobStatus.fromJson(json['status'] as String),
      version: (json['version'] as num).toInt(),
      deliveries: (json['deliveries'] as List<dynamic>)
          .map((e) => DeliveryDto.fromJson(e as Map<String, dynamic>))
          .toList(),
      cancelledAt: json['cancelled_at'] == null
          ? null
          : DateTime.parse(json['cancelled_at'] as String),
      createdBy: json['created_by'] as String?,
    );

Map<String, dynamic> _$PublishJobDtoToJson(PublishJobDto instance) =>
    <String, dynamic>{
      'cancelled_at': instance.cancelledAt?.toIso8601String(),
      'created_at': instance.createdAt.toIso8601String(),
      'created_by': instance.createdBy,
      'dry_run': instance.dryRun,
      'id': instance.id,
      'native_schedule': instance.nativeSchedule,
      'post_id': instance.postId,
      'post_version': instance.postVersion,
      'scheduled_at': instance.scheduledAt.toIso8601String(),
      'status': _$JobStatusEnumMap[instance.status]!,
      'version': instance.version,
      'deliveries': instance.deliveries,
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
