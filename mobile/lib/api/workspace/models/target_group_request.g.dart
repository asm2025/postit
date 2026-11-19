// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_group_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetGroupRequest _$TargetGroupRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['account_ids', 'name'],
    requiredKeys: const ['name'],
    disallowNullValues: const ['account_ids', 'name'],
  );
  return TargetGroupRequest(
    accountIds: (json['account_ids'] as List<dynamic>?)
        ?.map((e) => e as String)
        .toList(),
    name: json['name'] as String,
  );
}

Map<String, dynamic> _$TargetGroupRequestToJson(TargetGroupRequest instance) =>
    <String, dynamic>{
      'account_ids': ?instance.accountIds,
      'name': instance.name,
    };
