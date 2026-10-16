// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'auth_config_dto.g.dart';

/// What a client needs to start the OIDC sign-in.
@JsonSerializable()
class AuthConfigDto {
  const AuthConfigDto({
    required this.clientId,
    required this.issuer,
    required this.scopes,
    this.androidClientId,
    this.iosClientId,
  });
  
  factory AuthConfigDto.fromJson(Map<String, Object?> json) => _$AuthConfigDtoFromJson(json);
  
  /// OAuth client ID of the Android app, or null when the deployment has none.
  @JsonKey(name: 'android_client_id')
  final String? androidClientId;

  /// OAuth client ID of the web client.
  @JsonKey(name: 'client_id')
  final String clientId;

  /// OAuth client ID of the iOS app, or null when the deployment has none.
  @JsonKey(name: 'ios_client_id')
  final String? iosClientId;

  /// Issuer URL of the identity provider.
  final String issuer;

  /// Scopes to request at sign-in.
  final List<String> scopes;

  Map<String, Object?> toJson() => _$AuthConfigDtoToJson(this);
}
