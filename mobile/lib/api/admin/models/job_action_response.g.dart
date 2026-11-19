// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'job_action_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

JobActionResponse _$JobActionResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['job_id'],
    disallowNullValues: const ['job_id'],
  );
  return JobActionResponse(
    jobId: json['job_id'] as String,
    additionalProperties:
        JobActionResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : JobActionResponse._readExtras(
            JobActionResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$JobActionResponseToJson(JobActionResponse instance) =>
    <String, dynamic>{'job_id': instance.jobId};
