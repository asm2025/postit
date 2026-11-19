// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_usage_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaUsageDto _$MediaUsageDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['quota_bytes', 'used_bytes'],
    disallowNullValues: const ['quota_bytes', 'used_bytes'],
  );
  return MediaUsageDto(
    quotaBytes: MediaUsageDto._read_quotaBytes(json['quota_bytes']),
    usedBytes: MediaUsageDto._read_usedBytes(json['used_bytes']),
    additionalProperties:
        MediaUsageDto._extras(json, 'additionalProperties') == null
        ? const {}
        : MediaUsageDto._readExtras(
            MediaUsageDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$MediaUsageDtoToJson(MediaUsageDto instance) =>
    <String, dynamic>{
      'quota_bytes': ?MediaUsageDto._write_quotaBytes(instance.quotaBytes),
      'used_bytes': ?MediaUsageDto._write_usedBytes(instance.usedBytes),
    };
