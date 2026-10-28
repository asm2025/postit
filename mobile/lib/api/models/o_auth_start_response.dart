// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'o_auth_start_response.g.dart';

/// A started OAuth connection.
@JsonSerializable()
class OAuthStartResponse {
  const OAuthStartResponse({
    required this.authorizeUrl,
    required this.expiresAt,
  });
  
  factory OAuthStartResponse.fromJson(Map<String, Object?> json) => _$OAuthStartResponseFromJson(json);
  
  /// Send the user's browser here to sign in to the platform. It carries a one-time `state`.
  @JsonKey(name: 'authorize_url')
  final String authorizeUrl;

  /// When the sign-in must be finished by.
  @JsonKey(name: 'expires_at')
  final DateTime expiresAt;

  Map<String, Object?> toJson() => _$OAuthStartResponseToJson(this);
}
