// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'quota_bucket.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

QuotaBucket _$QuotaBucketFromJson(Map<String, dynamic> json) => QuotaBucket(
  costs: QuotaCosts.fromJson(json['costs'] as Map<String, dynamic>),
  defaultLimit: (json['default_limit'] as num).toInt(),
  id: json['id'] as String,
  period: QuotaPeriod.fromJson(json['period'] as String),
  resetTimeZone: json['reset_time_zone'] as String,
  scope: QuotaScope.fromJson(json['scope'] as String),
);

Map<String, dynamic> _$QuotaBucketToJson(QuotaBucket instance) =>
    <String, dynamic>{
      'costs': instance.costs,
      'default_limit': instance.defaultLimit,
      'id': instance.id,
      'period': _$QuotaPeriodEnumMap[instance.period]!,
      'reset_time_zone': instance.resetTimeZone,
      'scope': _$QuotaScopeEnumMap[instance.scope]!,
    };

const _$QuotaPeriodEnumMap = {
  QuotaPeriod.day: 'day',
  QuotaPeriod.month: 'month',
  QuotaPeriod.$unknown: r'$unknown',
};

const _$QuotaScopeEnumMap = {
  QuotaScope.app: 'app',
  QuotaScope.account: 'account',
  QuotaScope.$unknown: r'$unknown',
};
