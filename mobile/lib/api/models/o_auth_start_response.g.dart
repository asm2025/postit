// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'o_auth_start_response.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

OAuthStartResponse _$OAuthStartResponseFromJson(Map<String, dynamic> json) =>
    OAuthStartResponse(
      authorizeUrl: json['authorize_url'] as String,
      expiresAt: DateTime.parse(json['expires_at'] as String),
    );

Map<String, dynamic> _$OAuthStartResponseToJson(OAuthStartResponse instance) =>
    <String, dynamic>{
      'authorize_url': instance.authorizeUrl,
      'expires_at': instance.expiresAt.toIso8601String(),
    };
