// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why a recurrence skipped an occurrence.
@JsonEnum()
enum SkipReason {
  @JsonValue('ai_provider_missing')
  aiProviderMissing('ai_provider_missing'),
  @JsonValue('rewrite_invalid')
  rewriteInvalid('rewrite_invalid'),
  @JsonValue('job_active')
  jobActive('job_active'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const SkipReason(this.json);

  factory SkipReason.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<SkipReason> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
