// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'attempt_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AttemptDto _$AttemptDtoFromJson(Map<String, dynamic> json) => AttemptDto(
  attempt: (json['attempt'] as num).toInt(),
  startedAt: DateTime.parse(json['started_at'] as String),
  worker: json['worker'] as String,
  errorCode: json['error_code'] as String?,
  errorKind: json['error_kind'] as String?,
  errorMessage: json['error_message'] as String?,
  finishedAt: json['finished_at'] == null
      ? null
      : DateTime.parse(json['finished_at'] as String),
  outcome: json['outcome'] as String?,
);

Map<String, dynamic> _$AttemptDtoToJson(AttemptDto instance) =>
    <String, dynamic>{
      'attempt': instance.attempt,
      'error_code': instance.errorCode,
      'error_kind': instance.errorKind,
      'error_message': instance.errorMessage,
      'finished_at': instance.finishedAt?.toIso8601String(),
      'outcome': instance.outcome,
      'started_at': instance.startedAt.toIso8601String(),
      'worker': instance.worker,
    };
