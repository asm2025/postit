// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'recurring_job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RecurringJobDto _$RecurringJobDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['name', 'schedule'],
    disallowNullValues: const ['name', 'schedule'],
  );
  return RecurringJobDto(
    lastOutcome: json['last_outcome'] as String?,
    lastRunAt: RecurringJobDto._read_lastRunAt(json['last_run_at']),
    name: json['name'] as String,
    nextRunAt: RecurringJobDto._read_nextRunAt(json['next_run_at']),
    schedule: json['schedule'] as String,
    additionalProperties:
        RecurringJobDto._extras(json, 'additionalProperties') == null
        ? const {}
        : RecurringJobDto._readExtras(
            RecurringJobDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$RecurringJobDtoToJson(RecurringJobDto instance) =>
    <String, dynamic>{
      'last_outcome': ?instance.lastOutcome,
      'last_run_at': ?RecurringJobDto._write_lastRunAt(instance.lastRunAt),
      'name': instance.name,
      'next_run_at': ?RecurringJobDto._write_nextRunAt(instance.nextRunAt),
      'schedule': instance.schedule,
    };
