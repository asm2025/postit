// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why an address is on the suppression list.
@JsonEnum()
enum SuppressionReasonDto {
  @JsonValue('unsubscribed')
  unsubscribed('unsubscribed'),
  @JsonValue('bounced')
  bounced('bounced'),
  @JsonValue('complained')
  complained('complained'),
  @JsonValue('manual')
  manual('manual'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const SuppressionReasonDto(this.json);

  factory SuppressionReasonDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<SuppressionReasonDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
