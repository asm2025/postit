// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'quota_bucket.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

QuotaBucket _$QuotaBucketFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'costs',
      'default_limit',
      'id',
      'period',
      'reset_time_zone',
      'scope',
    ],
    disallowNullValues: const [
      'costs',
      'default_limit',
      'id',
      'period',
      'reset_time_zone',
      'scope',
    ],
  );
  return QuotaBucket(
    costs: QuotaCosts.fromJson(json['costs'] as Map<String, dynamic>),
    defaultLimit: QuotaBucket._read_defaultLimit(json['default_limit']),
    id: json['id'] as String,
    period: QuotaBucket._read_period(json['period']),
    resetTimeZone: json['reset_time_zone'] as String,
    scope: QuotaBucket._read_scope(json['scope']),
    additionalProperties:
        QuotaBucket._extras(json, 'additionalProperties') == null
        ? const {}
        : QuotaBucket._readExtras(
            QuotaBucket._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$QuotaBucketToJson(QuotaBucket instance) =>
    <String, dynamic>{
      'costs': instance.costs.toJson(),
      'default_limit': ?QuotaBucket._write_defaultLimit(instance.defaultLimit),
      'id': instance.id,
      'period': ?QuotaBucket._write_period(instance.period),
      'reset_time_zone': instance.resetTimeZone,
      'scope': ?QuotaBucket._write_scope(instance.scope),
    };
