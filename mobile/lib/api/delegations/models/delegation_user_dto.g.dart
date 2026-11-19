// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delegation_user_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DelegationUserDto _$DelegationUserDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['display_name', 'id', 'status'],
    disallowNullValues: const ['display_name', 'id', 'status'],
  );
  return DelegationUserDto(
    displayName: json['display_name'] as String,
    id: json['id'] as String,
    status: DelegationUserDto._read_status(json['status']),
    additionalProperties:
        DelegationUserDto._extras(json, 'additionalProperties') == null
        ? const {}
        : DelegationUserDto._readExtras(
            DelegationUserDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DelegationUserDtoToJson(DelegationUserDto instance) =>
    <String, dynamic>{
      'display_name': instance.displayName,
      'id': instance.id,
      'status': ?DelegationUserDto._write_status(instance.status),
    };
