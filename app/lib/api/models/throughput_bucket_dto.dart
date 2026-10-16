// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'throughput_bucket_dto.g.dart';

/// Attempt outcomes within one time bucket.
@JsonSerializable()
class ThroughputBucketDto {
  const ThroughputBucketDto({
    required this.failed,
    required this.start,
    required this.succeeded,
  });
  
  factory ThroughputBucketDto.fromJson(Map<String, Object?> json) => _$ThroughputBucketDtoFromJson(json);
  
  /// Attempts that failed.
  final int failed;

  /// Start of the bucket.
  final DateTime start;

  /// Attempts that succeeded.
  final int succeeded;

  Map<String, Object?> toJson() => _$ThroughputBucketDtoToJson(this);
}
