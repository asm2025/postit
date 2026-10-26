// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'media_usage_dto.g.dart';

/// Storage used against the quota.
@JsonSerializable()
class MediaUsageDto {
  const MediaUsageDto({
    required this.quotaBytes,
    required this.usedBytes,
  });
  
  factory MediaUsageDto.fromJson(Map<String, Object?> json) => _$MediaUsageDtoFromJson(json);
  
  /// The quota in bytes; 0 means unlimited.
  @JsonKey(name: 'quota_bytes')
  final int quotaBytes;

  /// Bytes stored or reserved by open uploads.
  @JsonKey(name: 'used_bytes')
  final int usedBytes;

  Map<String, Object?> toJson() => _$MediaUsageDtoToJson(this);
}
