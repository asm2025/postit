// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reschedule_publish_job_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ReschedulePublishJobResponse503 _$ReschedulePublishJobResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return ReschedulePublishJobResponse503(
    code: ReschedulePublishJobResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: ReschedulePublishJobResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => ReschedulePublishJobResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        ReschedulePublishJobResponse503._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : ReschedulePublishJobResponse503._readExtras(
            ReschedulePublishJobResponse503._extras(
              json,
              'additionalProperties',
            ),
          ),
  );
}

Map<String, dynamic> _$ReschedulePublishJobResponse503ToJson(
  ReschedulePublishJobResponse503 instance,
) => <String, dynamic>{
  'code': ?ReschedulePublishJobResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?ReschedulePublishJobResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
