// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'o_auth_start_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

OAuthStartResponse _$OAuthStartResponseFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['authorize_url', 'expires_at'],
    disallowNullValues: const ['authorize_url', 'expires_at'],
  );
  return OAuthStartResponse(
    authorizeUrl: json['authorize_url'] as String,
    expiresAt: OAuthStartResponse._read_expiresAt(json['expires_at']),
    additionalProperties:
        OAuthStartResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : OAuthStartResponse._readExtras(
            OAuthStartResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$OAuthStartResponseToJson(OAuthStartResponse instance) =>
    <String, dynamic>{
      'authorize_url': instance.authorizeUrl,
      'expires_at': ?OAuthStartResponse._write_expiresAt(instance.expiresAt),
    };
