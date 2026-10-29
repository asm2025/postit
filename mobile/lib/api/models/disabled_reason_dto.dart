// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why no action applies to a job: a stable code the clients translate.
@JsonEnum()
enum DisabledReasonDto {
  @JsonValue('already_retried')
  alreadyRetried('already_retried'),
  @JsonValue('type_not_registered')
  typeNotRegistered('type_not_registered'),
  @JsonValue('must_finish')
  mustFinish('must_finish'),
  @JsonValue('publishing_managed')
  publishingManaged('publishing_managed'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const DisabledReasonDto(this.json);

  factory DisabledReasonDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<DisabledReasonDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
