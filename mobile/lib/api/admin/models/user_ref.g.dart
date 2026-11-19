// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'user_ref.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UserRef _$UserRefFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['deleted', 'id'],
    disallowNullValues: const ['deleted', 'id'],
  );
  return UserRef(
    deleted: json['deleted'] as bool,
    displayName: json['display_name'] as String?,
    id: json['id'] as String,
    additionalProperties: UserRef._extras(json, 'additionalProperties') == null
        ? const {}
        : UserRef._readExtras(UserRef._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$UserRefToJson(UserRef instance) => <String, dynamic>{
  'deleted': instance.deleted,
  'display_name': ?instance.displayName,
  'id': instance.id,
};
