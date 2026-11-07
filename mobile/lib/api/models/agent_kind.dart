// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Whether an agent publishes to an audience or sends to individuals.
@JsonEnum()
enum AgentKind {
  @JsonValue('broadcast')
  broadcast('broadcast'),
  @JsonValue('direct')
  direct('direct'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const AgentKind(this.json);

  factory AgentKind.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<AgentKind> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
