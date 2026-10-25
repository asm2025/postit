// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'auth_config_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AuthConfigDto _$AuthConfigDtoFromJson(Map<String, dynamic> json) =>
    AuthConfigDto(
      clientId: json['client_id'] as String,
      issuer: json['issuer'] as String,
      scopes: (json['scopes'] as List<dynamic>)
          .map((e) => e as String)
          .toList(),
      androidClientId: json['android_client_id'] as String?,
      iosClientId: json['ios_client_id'] as String?,
    );

Map<String, dynamic> _$AuthConfigDtoToJson(AuthConfigDto instance) =>
    <String, dynamic>{
      'android_client_id': instance.androidClientId,
      'client_id': instance.clientId,
      'ios_client_id': instance.iosClientId,
      'issuer': instance.issuer,
      'scopes': instance.scopes,
    };
