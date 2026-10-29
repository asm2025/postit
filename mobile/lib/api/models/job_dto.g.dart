// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobDto _$JobDtoFromJson(Map<String, dynamic> json) => JobDto(
  actions: JobActionsDto.fromJson(json['actions'] as Map<String, dynamic>),
  attempts: (json['attempts'] as num).toInt(),
  createdAt: DateTime.parse(json['created_at'] as String),
  id: json['id'] as String,
  jobType: json['job_type'] as String,
  queue: json['queue'] as String,
  runAt: DateTime.parse(json['run_at'] as String),
  state: JobStateDto.fromJson(json['state'] as String),
  summary: Map<String, String>.from(json['summary'] as Map),
  actionsDisabledReason: json['actions_disabled_reason'] == null
      ? null
      : DisabledReasonDto.fromJson(json['actions_disabled_reason'] as String),
  errorCode: json['error_code'] as String?,
  errorKind: json['error_kind'] as String?,
  finishedAt: json['finished_at'] == null
      ? null
      : DateTime.parse(json['finished_at'] as String),
  maxAttempts: (json['max_attempts'] as num?)?.toInt(),
  recurringName: json['recurring_name'] as String?,
  startedAt: json['started_at'] == null
      ? null
      : DateTime.parse(json['started_at'] as String),
);

Map<String, dynamic> _$JobDtoToJson(JobDto instance) => <String, dynamic>{
  'actions': instance.actions,
  'actions_disabled_reason':
      _$DisabledReasonDtoEnumMap[instance.actionsDisabledReason],
  'attempts': instance.attempts,
  'created_at': instance.createdAt.toIso8601String(),
  'error_code': instance.errorCode,
  'error_kind': instance.errorKind,
  'finished_at': instance.finishedAt?.toIso8601String(),
  'id': instance.id,
  'job_type': instance.jobType,
  'max_attempts': instance.maxAttempts,
  'queue': instance.queue,
  'recurring_name': instance.recurringName,
  'run_at': instance.runAt.toIso8601String(),
  'started_at': instance.startedAt?.toIso8601String(),
  'state': _$JobStateDtoEnumMap[instance.state]!,
  'summary': instance.summary,
};

const _$DisabledReasonDtoEnumMap = {
  DisabledReasonDto.alreadyRetried: 'already_retried',
  DisabledReasonDto.typeNotRegistered: 'type_not_registered',
  DisabledReasonDto.mustFinish: 'must_finish',
  DisabledReasonDto.publishingManaged: 'publishing_managed',
  DisabledReasonDto.$unknown: r'$unknown',
};

const _$JobStateDtoEnumMap = {
  JobStateDto.queued: 'queued',
  JobStateDto.scheduled: 'scheduled',
  JobStateDto.running: 'running',
  JobStateDto.succeeded: 'succeeded',
  JobStateDto.failed: 'failed',
  JobStateDto.dead: 'dead',
  JobStateDto.killed: 'killed',
  JobStateDto.cancelled: 'cancelled',
  JobStateDto.$unknown: r'$unknown',
};
