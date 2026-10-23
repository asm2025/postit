// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'discovered_account_dto.dart';

part 'credential_check_response.g.dart';

/// The result of checking a pasted credential.
@JsonSerializable()
class CredentialCheckResponse {
  const CredentialCheckResponse({
    required this.discovered,
    this.connectId,
    this.expiresAt,
  });
  
  factory CredentialCheckResponse.fromJson(Map<String, Object?> json) => _$CredentialCheckResponseFromJson(json);
  
  /// Confirm with this ID. Shown once; absent when the credential reaches no account.
  @JsonKey(name: 'connect_id')
  final String? connectId;

  /// The accounts the credential reaches.
  final List<DiscoveredAccountDto> discovered;

  /// When `connect_id` stops working.
  @JsonKey(name: 'expires_at')
  final DateTime? expiresAt;

  Map<String, Object?> toJson() => _$CredentialCheckResponseToJson(this);
}
