// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_detail_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobDetailDto _$JobDetailDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['attempts', 'job'],
    disallowNullValues: const ['attempts', 'job'],
  );
  return JobDetailDto(
    attempts: (json['attempts'] as List<dynamic>)
        .map((e) => AttemptDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    job: JobDto.fromJson(json['job'] as Map<String, dynamic>),
    retriedBy: json['retried_by'] as String?,
    retriedFrom: json['retried_from'] as String?,
    additionalProperties:
        JobDetailDto._extras(json, 'additionalProperties') == null
        ? const {}
        : JobDetailDto._readExtras(
            JobDetailDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobDetailDtoToJson(JobDetailDto instance) =>
    <String, dynamic>{
      'attempts': instance.attempts.map((e) => e.toJson()).toList(),
      'job': instance.job.toJson(),
      'retried_by': ?instance.retriedBy,
      'retried_from': ?instance.retriedFrom,
    };
