// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'credential_check_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CredentialCheckResponse _$CredentialCheckResponseFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['discovered'],
    disallowNullValues: const ['discovered'],
  );
  return CredentialCheckResponse(
    connectId: json['connect_id'] as String?,
    discovered: (json['discovered'] as List<dynamic>)
        .map((e) => DiscoveredAccountDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    expiresAt: CredentialCheckResponse._read_expiresAt(json['expires_at']),
    additionalProperties:
        CredentialCheckResponse._extras(json, 'additionalProperties') == null
        ? const {}
        : CredentialCheckResponse._readExtras(
            CredentialCheckResponse._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$CredentialCheckResponseToJson(
  CredentialCheckResponse instance,
) => <String, dynamic>{
  'connect_id': ?instance.connectId,
  'discovered': instance.discovered.map((e) => e.toJson()).toList(),
  'expires_at': ?CredentialCheckResponse._write_expiresAt(instance.expiresAt),
};
