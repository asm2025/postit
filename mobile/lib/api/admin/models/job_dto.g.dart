// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobDto _$JobDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'actions',
      'attempts',
      'created_at',
      'id',
      'job_type',
      'queue',
      'run_at',
      'state',
      'summary',
    ],
    disallowNullValues: const [
      'actions',
      'attempts',
      'created_at',
      'id',
      'job_type',
      'queue',
      'run_at',
      'state',
      'summary',
    ],
  );
  return JobDto(
    actions: JobActionsDto.fromJson(json['actions'] as Map<String, dynamic>),
    actionsDisabledReason: JobDto._read_actionsDisabledReason(
      json['actions_disabled_reason'],
    ),
    attempts: JobDto._read_attempts(json['attempts']),
    createdAt: JobDto._read_createdAt(json['created_at']),
    errorCode: json['error_code'] as String?,
    errorKind: json['error_kind'] as String?,
    finishedAt: JobDto._read_finishedAt(json['finished_at']),
    id: json['id'] as String,
    jobType: json['job_type'] as String,
    maxAttempts: JobDto._read_maxAttempts(json['max_attempts']),
    queue: json['queue'] as String,
    recurringName: json['recurring_name'] as String?,
    runAt: JobDto._read_runAt(json['run_at']),
    startedAt: JobDto._read_startedAt(json['started_at']),
    state: JobDto._read_state(json['state']),
    summary: Map<String, String>.from(json['summary'] as Map),
    additionalProperties: JobDto._extras(json, 'additionalProperties') == null
        ? const {}
        : JobDto._readExtras(JobDto._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$JobDtoToJson(JobDto instance) => <String, dynamic>{
  'actions': instance.actions.toJson(),
  'actions_disabled_reason': ?JobDto._write_actionsDisabledReason(
    instance.actionsDisabledReason,
  ),
  'attempts': ?JobDto._write_attempts(instance.attempts),
  'created_at': ?JobDto._write_createdAt(instance.createdAt),
  'error_code': ?instance.errorCode,
  'error_kind': ?instance.errorKind,
  'finished_at': ?JobDto._write_finishedAt(instance.finishedAt),
  'id': instance.id,
  'job_type': instance.jobType,
  'max_attempts': ?JobDto._write_maxAttempts(instance.maxAttempts),
  'queue': instance.queue,
  'recurring_name': ?instance.recurringName,
  'run_at': ?JobDto._write_runAt(instance.runAt),
  'started_at': ?JobDto._write_startedAt(instance.startedAt),
  'state': ?JobDto._write_state(instance.state),
  'summary': instance.summary,
};
