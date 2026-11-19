// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'connect_method.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConnectMethod _$ConnectMethodFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['method'],
    disallowNullValues: const ['method', 'schema'],
  );
  return ConnectMethod(
    method: ConnectMethod._read_method(json['method']),
    schema: json['schema'] == null
        ? null
        : ConnectMethodSchema.fromJson(json['schema'] as Map<String, dynamic>),
    additionalProperties:
        ConnectMethod._extras(json, 'additionalProperties') == null
        ? const {}
        : ConnectMethod._readExtras(
            ConnectMethod._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ConnectMethodToJson(ConnectMethod instance) =>
    <String, dynamic>{
      'method': ?ConnectMethod._write_method(instance.method),
      'schema': ?instance.schema?.toJson(),
    };
