// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'attempt_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AttemptDto _$AttemptDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['attempt', 'started_at', 'worker'],
    disallowNullValues: const ['attempt', 'started_at', 'worker'],
  );
  return AttemptDto(
    attempt: AttemptDto._read_attempt(json['attempt']),
    errorCode: json['error_code'] as String?,
    errorKind: json['error_kind'] as String?,
    errorMessage: json['error_message'] as String?,
    finishedAt: AttemptDto._read_finishedAt(json['finished_at']),
    outcome: json['outcome'] as String?,
    startedAt: AttemptDto._read_startedAt(json['started_at']),
    worker: json['worker'] as String,
    additionalProperties:
        AttemptDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AttemptDto._readExtras(
            AttemptDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AttemptDtoToJson(AttemptDto instance) =>
    <String, dynamic>{
      'attempt': ?AttemptDto._write_attempt(instance.attempt),
      'error_code': ?instance.errorCode,
      'error_kind': ?instance.errorKind,
      'error_message': ?instance.errorMessage,
      'finished_at': ?AttemptDto._write_finishedAt(instance.finishedAt),
      'outcome': ?instance.outcome,
      'started_at': ?AttemptDto._write_startedAt(instance.startedAt),
      'worker': instance.worker,
    };
