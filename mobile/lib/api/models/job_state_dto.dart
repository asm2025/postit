// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Where a job is in its life. Clients must tolerate values added later.
@JsonEnum()
enum JobStateDto {
  @JsonValue('queued')
  queued('queued'),
  @JsonValue('scheduled')
  scheduled('scheduled'),
  @JsonValue('running')
  running('running'),
  @JsonValue('succeeded')
  succeeded('succeeded'),
  @JsonValue('failed')
  failed('failed'),
  @JsonValue('dead')
  dead('dead'),
  @JsonValue('killed')
  killed('killed'),
  @JsonValue('cancelled')
  cancelled('cancelled'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const JobStateDto(this.json);

  factory JobStateDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<JobStateDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
