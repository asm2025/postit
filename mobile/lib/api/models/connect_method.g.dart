// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'connect_method.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ConnectMethod _$ConnectMethodFromJson(Map<String, dynamic> json) =>
    ConnectMethod(
      method: ConnectMethodMethod.fromJson(json['method'] as String),
      schema: json['schema'],
    );

Map<String, dynamic> _$ConnectMethodToJson(ConnectMethod instance) =>
    <String, dynamic>{
      'method': _$ConnectMethodMethodEnumMap[instance.method]!,
      'schema': instance.schema,
    };

const _$ConnectMethodMethodEnumMap = {
  ConnectMethodMethod.oauth: 'oauth',
  ConnectMethodMethod.credentials: 'credentials',
  ConnectMethodMethod.$unknown: r'$unknown',
};
