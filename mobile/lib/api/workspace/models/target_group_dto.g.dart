// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'target_group_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TargetGroupDto _$TargetGroupDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_ids', 'id', 'name', 'updated_at'],
    disallowNullValues: const ['account_ids', 'id', 'name', 'updated_at'],
  );
  return TargetGroupDto(
    accountIds: (json['account_ids'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    id: json['id'] as String,
    name: json['name'] as String,
    updatedAt: TargetGroupDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        TargetGroupDto._extras(json, 'additionalProperties') == null
        ? const {}
        : TargetGroupDto._readExtras(
            TargetGroupDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$TargetGroupDtoToJson(TargetGroupDto instance) =>
    <String, dynamic>{
      'account_ids': instance.accountIds,
      'id': instance.id,
      'name': instance.name,
      'updated_at': ?TargetGroupDto._write_updatedAt(instance.updatedAt),
    };
