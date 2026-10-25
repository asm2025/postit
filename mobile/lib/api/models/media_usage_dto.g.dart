// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_usage_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaUsageDto _$MediaUsageDtoFromJson(Map<String, dynamic> json) =>
    MediaUsageDto(
      quotaBytes: (json['quota_bytes'] as num).toInt(),
      usedBytes: (json['used_bytes'] as num).toInt(),
    );

Map<String, dynamic> _$MediaUsageDtoToJson(MediaUsageDto instance) =>
    <String, dynamic>{
      'quota_bytes': instance.quotaBytes,
      'used_bytes': instance.usedBytes,
    };
