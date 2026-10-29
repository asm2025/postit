// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'kind_switch_update.dart';
import 'locale_dto.dart';

part 'update_preferences.g.dart';

/// Changes the caller's preferences. Every field is optional; null or absent leaves the.
/// stored value.
@JsonSerializable()
class UpdatePreferences {
  const UpdatePreferences({
    this.commentDigest,
    this.emailNotifications,
    this.locale,
    this.switches,
    this.timezone,
  });
  
  factory UpdatePreferences.fromJson(Map<String, Object?> json) => _$UpdatePreferencesFromJson(json);
  
  /// Whether comment notifications are emailed as a daily digest.
  @JsonKey(name: 'comment_digest')
  final bool? commentDigest;

  /// The master switch for emails about activity.
  @JsonKey(name: 'email_notifications')
  final bool? emailNotifications;

  /// `en-US` or `ar`.
  final LocaleDto? locale;

  /// Per-kind changes, merged per kind: kinds not listed keep their choice.
  final List<KindSwitchUpdate>? switches;

  /// An IANA time zone name; anything else is rejected with `validation_failed`.
  final String? timezone;

  Map<String, Object?> toJson() => _$UpdatePreferencesToJson(this);
}
