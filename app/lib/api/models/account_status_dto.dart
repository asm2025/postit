// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Whether an account can be used.
@JsonEnum()
enum AccountStatusDto {
  @JsonValue('active')
  active('active'),
  @JsonValue('needs_reconnect')
  needsReconnect('needs_reconnect'),
  @JsonValue('revoked')
  revoked('revoked'),
  @JsonValue('disconnected')
  disconnected('disconnected'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const AccountStatusDto(this.json);

  factory AccountStatusDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<AccountStatusDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
