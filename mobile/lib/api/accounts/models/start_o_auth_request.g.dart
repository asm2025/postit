// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'start_o_auth_request.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

StartOAuthRequest _$StartOAuthRequestFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const ['return_to'],
    requiredKeys: const ['return_to'],
    disallowNullValues: const ['return_to'],
  );
  return StartOAuthRequest(returnTo: json['return_to'] as String);
}

Map<String, dynamic> _$StartOAuthRequestToJson(StartOAuthRequest instance) =>
    <String, dynamic>{'return_to': instance.returnTo};
