// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_list_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactListDto _$ContactListDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['created_at', 'id', 'members', 'name', 'updated_at'],
    disallowNullValues: const [
      'created_at',
      'id',
      'members',
      'name',
      'updated_at',
    ],
  );
  return ContactListDto(
    createdAt: ContactListDto._read_createdAt(json['created_at']),
    id: json['id'] as String,
    members: ContactListDto._read_members(json['members']),
    name: json['name'] as String,
    updatedAt: ContactListDto._read_updatedAt(json['updated_at']),
    additionalProperties:
        ContactListDto._extras(json, 'additionalProperties') == null
        ? const {}
        : ContactListDto._readExtras(
            ContactListDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ContactListDtoToJson(ContactListDto instance) =>
    <String, dynamic>{
      'created_at': ?ContactListDto._write_createdAt(instance.createdAt),
      'id': instance.id,
      'members': ?ContactListDto._write_members(instance.members),
      'name': instance.name,
      'updated_at': ?ContactListDto._write_updatedAt(instance.updatedAt),
    };
