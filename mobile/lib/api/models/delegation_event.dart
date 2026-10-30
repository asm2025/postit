// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// What happened to a delegation.
@JsonEnum()
enum DelegationEvent {
  @JsonValue('granted')
  granted('granted'),
  @JsonValue('changed')
  changed('changed'),
  @JsonValue('revoked')
  revoked('revoked'),
  @JsonValue('left')
  left('left'),
  @JsonValue('expired')
  expired('expired'),
  @JsonValue('limit_emptied')
  limitEmptied('limit_emptied'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const DelegationEvent(this.json);

  factory DelegationEvent.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<DelegationEvent> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
