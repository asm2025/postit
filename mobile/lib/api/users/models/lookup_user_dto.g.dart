// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'lookup_user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LookupUserDto _$LookupUserDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['display_name', 'id'],
    disallowNullValues: const ['display_name', 'id'],
  );
  return LookupUserDto(
    displayName: json['display_name'] as String,
    id: json['id'] as String,
    additionalProperties:
        LookupUserDto._extras(json, 'additionalProperties') == null
        ? const {}
        : LookupUserDto._readExtras(
            LookupUserDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$LookupUserDtoToJson(LookupUserDto instance) =>
    <String, dynamic>{'display_name': instance.displayName, 'id': instance.id};
