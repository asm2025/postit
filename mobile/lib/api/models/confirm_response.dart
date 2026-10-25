// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'connected_account_dto.dart';

part 'confirm_response.g.dart';

/// The result of confirming a connection.
@JsonSerializable()
class ConfirmResponse {
  const ConfirmResponse({
    required this.connected,
    required this.skippedElsewhere,
    required this.unknown,
  });
  
  factory ConfirmResponse.fromJson(Map<String, Object?> json) => _$ConfirmResponseFromJson(json);
  
  /// The accounts now connected.
  final List<ConnectedAccountDto> connected;

  /// How many chosen accounts someone else already has connected.
  @JsonKey(name: 'skipped_elsewhere')
  final int skippedElsewhere;

  /// Chosen IDs the credential did not reach.
  final List<String> unknown;

  Map<String, Object?> toJson() => _$ConfirmResponseToJson(this);
}
