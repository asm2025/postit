// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'auth_config_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AuthConfigDto _$AuthConfigDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['client_id', 'issuer', 'scopes'],
    disallowNullValues: const ['client_id', 'issuer', 'scopes'],
  );
  return AuthConfigDto(
    androidClientId: json['android_client_id'] as String?,
    clientId: json['client_id'] as String,
    iosClientId: json['ios_client_id'] as String?,
    issuer: json['issuer'] as String,
    scopes: (json['scopes'] as List<dynamic>).map((e) => e as String).toList(),
    additionalProperties:
        AuthConfigDto._extras(json, 'additionalProperties') == null
        ? const {}
        : AuthConfigDto._readExtras(
            AuthConfigDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AuthConfigDtoToJson(AuthConfigDto instance) =>
    <String, dynamic>{
      'android_client_id': ?instance.androidClientId,
      'client_id': instance.clientId,
      'ios_client_id': ?instance.iosClientId,
      'issuer': instance.issuer,
      'scopes': instance.scopes,
    };
