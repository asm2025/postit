// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'kind_switch_dto.dart';
import 'locale_dto.dart';

part 'preferences_dto.g.dart';

/// The caller's preferences.
@JsonSerializable()
class PreferencesDto {
  const PreferencesDto({
    required this.commentDigest,
    required this.emailNotifications,
    required this.locale,
    required this.switches,
    required this.timezone,
  });
  
  factory PreferencesDto.fromJson(Map<String, Object?> json) => _$PreferencesDtoFromJson(json);
  
  /// Whether comment notifications are emailed as a daily digest.
  @JsonKey(name: 'comment_digest')
  final bool commentDigest;

  /// The master switch for emails about activity.
  @JsonKey(name: 'email_notifications')
  final bool emailNotifications;

  /// The language of the interface and of emails sent to the caller.
  final LocaleDto locale;

  /// One entry per notification kind, critical kinds first.
  final List<KindSwitchDto> switches;

  /// IANA time zone name (for example `Asia/Riyadh`); times are shown and digests are.
  /// scheduled in it.
  final String timezone;

  Map<String, Object?> toJson() => _$PreferencesDtoToJson(this);
}
