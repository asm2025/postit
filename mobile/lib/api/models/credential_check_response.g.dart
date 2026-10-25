// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'credential_check_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CredentialCheckResponse _$CredentialCheckResponseFromJson(
  Map<String, dynamic> json,
) => CredentialCheckResponse(
  discovered: (json['discovered'] as List<dynamic>)
      .map((e) => DiscoveredAccountDto.fromJson(e as Map<String, dynamic>))
      .toList(),
  connectId: json['connect_id'] as String?,
  expiresAt: json['expires_at'] == null
      ? null
      : DateTime.parse(json['expires_at'] as String),
);

Map<String, dynamic> _$CredentialCheckResponseToJson(
  CredentialCheckResponse instance,
) => <String, dynamic>{
  'connect_id': instance.connectId,
  'discovered': instance.discovered,
  'expires_at': instance.expiresAt?.toIso8601String(),
};
