// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'connect_kind_dto.dart';
import 'discovered_account_dto.dart';

part 'pending_connection_dto.g.dart';

/// A connection waiting for the user to pick accounts, from pasted credentials or an OAuth.
/// sign-in.
@JsonSerializable()
class PendingConnectionDto {
  const PendingConnectionDto({
    required this.connect,
    required this.discovered,
    required this.expiresAt,
    required this.platform,
  });
  
  factory PendingConnectionDto.fromJson(Map<String, Object?> json) => _$PendingConnectionDtoFromJson(json);
  
  /// How the accounts would be connected.
  final ConnectKindDto connect;

  /// The accounts it reaches; confirm with the `remote_id`s to connect.
  final List<DiscoveredAccountDto> discovered;

  /// When the connection can no longer be confirmed.
  @JsonKey(name: 'expires_at')
  final DateTime expiresAt;

  /// The platform's ID.
  final String platform;

  Map<String, Object?> toJson() => _$PendingConnectionDtoToJson(this);
}
