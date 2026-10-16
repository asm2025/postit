// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_detail_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobDetailDto _$JobDetailDtoFromJson(Map<String, dynamic> json) => JobDetailDto(
  attempts: (json['attempts'] as List<dynamic>)
      .map((e) => AttemptDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  job: JobDto.fromJson(json['job'] as Map<String, dynamic>),
  retriedBy: json['retried_by'] as String?,
  retriedFrom: json['retried_from'] as String?,
);

Map<String, dynamic> _$JobDetailDtoToJson(JobDetailDto instance) =>
    <String, dynamic>{
      'attempts': instance.attempts,
      'job': instance.job,
      'retried_by': instance.retriedBy,
      'retried_from': instance.retriedFrom,
    };
