// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'contact_list_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ContactListRequest _$ContactListRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['name'],
    requiredKeys: const ['name'],
    disallowNullValues: const ['name'],
  );
  return ContactListRequest(name: json['name'] as String);
}

Map<String, dynamic> _$ContactListRequestToJson(ContactListRequest instance) =>
    <String, dynamic>{'name': instance.name};
