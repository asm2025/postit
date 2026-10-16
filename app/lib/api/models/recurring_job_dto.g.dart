// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'recurring_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RecurringJobDto _$RecurringJobDtoFromJson(Map<String, dynamic> json) =>
    RecurringJobDto(
      name: json['name'] as String,
      schedule: json['schedule'] as String,
      lastOutcome: json['last_outcome'] as String?,
      lastRunAt: json['last_run_at'] == null
          ? null
          : DateTime.parse(json['last_run_at'] as String),
      nextRunAt: json['next_run_at'] == null
          ? null
          : DateTime.parse(json['next_run_at'] as String),
    );

Map<String, dynamic> _$RecurringJobDtoToJson(RecurringJobDto instance) =>
    <String, dynamic>{
      'last_outcome': instance.lastOutcome,
      'last_run_at': instance.lastRunAt?.toIso8601String(),
      'name': instance.name,
      'next_run_at': instance.nextRunAt?.toIso8601String(),
      'schedule': instance.schedule,
    };
