// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_id.dart';
import 'publish_job_id.dart';

part 'job_notice.g.dart';

/// Payload of `job_partial`.
@JsonSerializable()
class JobNotice {
  const JobNotice({
    required this.failed,
    required this.jobId,
    required this.postId,
    required this.published,
  });
  
  factory JobNotice.fromJson(Map<String, Object?> json) => _$JobNoticeFromJson(json);
  
  /// Deliveries failed so far.
  final int failed;

  /// The job.
  @JsonKey(name: 'job_id')
  final PublishJobId jobId;

  /// Its post.
  @JsonKey(name: 'post_id')
  final PostId postId;

  /// Deliveries published so far.
  final int published;

  Map<String, Object?> toJson() => _$JobNoticeToJson(this);
}
