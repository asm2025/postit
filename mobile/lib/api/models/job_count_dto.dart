// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'job_count_dto.g.dart';

/// A count under a key (state, type or queue).
@JsonSerializable()
class JobCountDto {
  const JobCountDto({
    required this.count,
    required this.key,
  });
  
  factory JobCountDto.fromJson(Map<String, Object?> json) => _$JobCountDtoFromJson(json);
  
  /// How many jobs.
  final int count;

  /// The state, type or queue name.
  final String key;

  Map<String, Object?> toJson() => _$JobCountDtoToJson(this);
}
