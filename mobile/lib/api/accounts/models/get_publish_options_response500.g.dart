// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'get_publish_options_response500.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

GetPublishOptionsResponse500 _$GetPublishOptionsResponse500FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return GetPublishOptionsResponse500(
    code: GetPublishOptionsResponse500._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: GetPublishOptionsResponse500._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => GetPublishOptionsResponse500ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        GetPublishOptionsResponse500._extras(json, 'additionalProperties') ==
            null
        ? const {}
        : GetPublishOptionsResponse500._readExtras(
            GetPublishOptionsResponse500._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$GetPublishOptionsResponse500ToJson(
  GetPublishOptionsResponse500 instance,
) => <String, dynamic>{
  'code': ?GetPublishOptionsResponse500._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?GetPublishOptionsResponse500._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
