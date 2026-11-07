// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// The status of a publish job, derived from its deliveries.
@JsonEnum()
enum JobStatus {
  @JsonValue('scheduled')
  scheduled('scheduled'),
  @JsonValue('publishing')
  publishing('publishing'),
  @JsonValue('published')
  published('published'),
  @JsonValue('cancelled')
  cancelled('cancelled'),
  @JsonValue('failed')
  failed('failed'),
  @JsonValue('partial')
  partial('partial'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const JobStatus(this.json);

  factory JobStatus.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<JobStatus> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
