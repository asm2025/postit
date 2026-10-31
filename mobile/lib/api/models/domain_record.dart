// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Which sender-domain DNS record a finding is about.
@JsonEnum()
enum DomainRecord {
  @JsonValue('spf')
  spf('spf'),
  @JsonValue('dkim')
  dkim('dkim'),
  @JsonValue('dmarc')
  dmarc('dmarc'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const DomainRecord(this.json);

  factory DomainRecord.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<DomainRecord> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
