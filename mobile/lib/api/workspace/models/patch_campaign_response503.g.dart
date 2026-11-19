// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'patch_campaign_response503.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PatchCampaignResponse503 _$PatchCampaignResponse503FromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['code', 'status', 'title', 'type'],
    disallowNullValues: const ['code', 'status', 'title', 'type'],
  );
  return PatchCampaignResponse503(
    code: PatchCampaignResponse503._read_code(json['code']),
    detail: json['detail'] as String?,
    field: json['field'] as String?,
    missingScopes: (json['missing_scopes'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    requestId: json['request_id'] as String?,
    status: PatchCampaignResponse503._read_status(json['status']),
    title: json['title'] as String,
    type: json['type'] as String,
    violations: (json['violations'] as List<dynamic>?)
        ?.map(
          (e) => PatchCampaignResponse503ViolationsItem.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    additionalProperties:
        PatchCampaignResponse503._extras(json, 'additionalProperties') == null
        ? const {}
        : PatchCampaignResponse503._readExtras(
            PatchCampaignResponse503._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PatchCampaignResponse503ToJson(
  PatchCampaignResponse503 instance,
) => <String, dynamic>{
  'code': ?PatchCampaignResponse503._write_code(instance.code),
  'detail': ?instance.detail,
  'field': ?instance.field,
  'missing_scopes': ?instance.missingScopes,
  'request_id': ?instance.requestId,
  'status': ?PatchCampaignResponse503._write_status(instance.status),
  'title': instance.title,
  'type': instance.type,
  'violations': ?instance.violations?.map((e) => e.toJson()).toList(),
};
