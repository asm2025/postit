// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'throughput_bucket_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ThroughputBucketDto _$ThroughputBucketDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['failed', 'start', 'succeeded'],
    disallowNullValues: const ['failed', 'start', 'succeeded'],
  );
  return ThroughputBucketDto(
    failed: ThroughputBucketDto._read_failed(json['failed']),
    start: ThroughputBucketDto._read_start(json['start']),
    succeeded: ThroughputBucketDto._read_succeeded(json['succeeded']),
    additionalProperties:
        ThroughputBucketDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ThroughputBucketDto._readExtras(
            ThroughputBucketDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ThroughputBucketDtoToJson(
  ThroughputBucketDto instance,
) => <String, dynamic>{
  'failed': ?ThroughputBucketDto._write_failed(instance.failed),
  'start': ?ThroughputBucketDto._write_start(instance.start),
  'succeeded': ?ThroughputBucketDto._write_succeeded(instance.succeeded),
};
