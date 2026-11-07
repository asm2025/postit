// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// An optional ability beyond publishing.
@JsonEnum()
enum Capability {
  @JsonValue('commenter')
  commenter('commenter'),
  @JsonValue('inbox_reader')
  inboxReader('inbox_reader'),
  @JsonValue('metrics_reader')
  metricsReader('metrics_reader'),
  @JsonValue('direct_sender')
  directSender('direct_sender'),
  @JsonValue('native_scheduler')
  nativeScheduler('native_scheduler'),
  @JsonValue('sender_domain_check')
  senderDomainCheck('sender_domain_check'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const Capability(this.json);

  factory Capability.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<Capability> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
