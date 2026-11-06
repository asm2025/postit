// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// The language an email campaign is written in. It sets the body's direction, the footer's.
/// language, and the language of the unsubscribe page, whatever each contact's own locale.
@JsonEnum()
enum EmailLanguage {
  @JsonValue('en-US')
  enUs('en-US'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const EmailLanguage(this.json);

  factory EmailLanguage.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<EmailLanguage> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
