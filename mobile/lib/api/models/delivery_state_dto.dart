// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Where a delivery is in its life. A parked delivery names why in `parked_reason`.
@JsonEnum()
enum DeliveryStateDto {
  @JsonValue('pending')
  pending('pending'),
  @JsonValue('claimed')
  claimed('claimed'),
  @JsonValue('in_flight')
  inFlight('in_flight'),
  @JsonValue('processing')
  processing('processing'),
  @JsonValue('outcome_unknown')
  outcomeUnknown('outcome_unknown'),
  @JsonValue('parked')
  parked('parked'),
  @JsonValue('published')
  published('published'),
  @JsonValue('failed')
  failed('failed'),
  @JsonValue('dead_letter')
  deadLetter('dead_letter'),
  @JsonValue('cancelled')
  cancelled('cancelled'),
  @JsonValue('simulated')
  simulated('simulated'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const DeliveryStateDto(this.json);

  factory DeliveryStateDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<DeliveryStateDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
