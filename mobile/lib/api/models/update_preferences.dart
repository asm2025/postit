// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'update_preferences.g.dart';

/// Changes the caller's preferences.
@JsonSerializable()
class UpdatePreferences {
  const UpdatePreferences({
    required this.locale,
  });
  
  factory UpdatePreferences.fromJson(Map<String, Object?> json) => _$UpdatePreferencesFromJson(json);
  
  /// `en-US` or `ar`; anything else is rejected with `validation_failed`.
  final String locale;

  Map<String, Object?> toJson() => _$UpdatePreferencesToJson(this);
}
