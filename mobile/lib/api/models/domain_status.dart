// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// How a sender-domain record looks.
@JsonEnum()
enum DomainStatus {
  @JsonValue('ok')
  ok('ok'),
  @JsonValue('missing')
  missing('missing'),
  @JsonValue('weak')
  weak('weak'),
  @JsonValue('unknown')
  unknown('unknown'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const DomainStatus(this.json);

  factory DomainStatus.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<DomainStatus> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
