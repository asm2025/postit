// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'push_endpoint_created.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PushEndpointCreated _$PushEndpointCreatedFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['id'],
    disallowNullValues: const ['id'],
  );
  return PushEndpointCreated(
    id: json['id'] as String,
    additionalProperties:
        PushEndpointCreated._extras(json, 'additionalProperties') == null
        ? const {}
        : PushEndpointCreated._readExtras(
            PushEndpointCreated._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PushEndpointCreatedToJson(
  PushEndpointCreated instance,
) => <String, dynamic>{'id': instance.id};
