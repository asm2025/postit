// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'delete_me_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

DeleteMeRequest _$DeleteMeRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['display_name'],
    disallowNullValues: const ['display_name'],
  );
  return DeleteMeRequest(
    displayName: json['display_name'] as String,
    additionalProperties:
        DeleteMeRequest._extras(json, 'additionalProperties') == null
        ? const {}
        : DeleteMeRequest._readExtras(
            DeleteMeRequest._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$DeleteMeRequestToJson(DeleteMeRequest instance) =>
    <String, dynamic>{'display_name': instance.displayName};
