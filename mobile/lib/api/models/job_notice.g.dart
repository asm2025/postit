// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobNotice _$JobNoticeFromJson(Map<String, dynamic> json) => JobNotice(
  failed: (json['failed'] as num).toInt(),
  jobId: json['job_id'] as String,
  postId: json['post_id'] as String,
  published: (json['published'] as num).toInt(),
);

Map<String, dynamic> _$JobNoticeToJson(JobNotice instance) => <String, dynamic>{
  'failed': instance.failed,
  'job_id': instance.jobId,
  'post_id': instance.postId,
  'published': instance.published,
};
