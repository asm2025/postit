// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'locale_dto.dart';

part 'preferences_dto.g.dart';

/// The caller's preferences.
@JsonSerializable()
class PreferencesDto {
  const PreferencesDto({
    required this.locale,
  });
  
  factory PreferencesDto.fromJson(Map<String, Object?> json) => _$PreferencesDtoFromJson(json);
  
  /// The language of the interface and of emails sent to the caller.
  final LocaleDto locale;

  Map<String, Object?> toJson() => _$PreferencesDtoToJson(this);
}
