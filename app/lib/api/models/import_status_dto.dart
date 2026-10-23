// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// How one CSV row ended.
@JsonEnum()
enum ImportStatusDto {
  @JsonValue('ok')
  ok('ok'),
  @JsonValue('invalid_fields')
  invalidFields('invalid_fields'),
  @JsonValue('invalid_credentials')
  invalidCredentials('invalid_credentials'),
  @JsonValue('already_connected')
  alreadyConnected('already_connected'),
  @JsonValue('connected_elsewhere')
  connectedElsewhere('connected_elsewhere'),
  @JsonValue('nothing_found')
  nothingFound('nothing_found'),
  @JsonValue('platform_error')
  platformError('platform_error'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const ImportStatusDto(this.json);

  factory ImportStatusDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<ImportStatusDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
