// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_preferences.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdatePreferences _$UpdatePreferencesFromJson(Map<String, dynamic> json) =>
    UpdatePreferences(
      commentDigest: json['comment_digest'] as bool?,
      emailNotifications: json['email_notifications'] as bool?,
      locale: json['locale'] == null
          ? null
          : LocaleDto.fromJson(json['locale'] as String),
      switches: (json['switches'] as List<dynamic>?)
          ?.map((e) => KindSwitchUpdate.fromJson(e as Map<String, dynamic>))
          .toList(),
      timezone: json['timezone'] as String?,
    );

Map<String, dynamic> _$UpdatePreferencesToJson(UpdatePreferences instance) =>
    <String, dynamic>{
      'comment_digest': instance.commentDigest,
      'email_notifications': instance.emailNotifications,
      'locale': _$LocaleDtoEnumMap[instance.locale],
      'switches': instance.switches,
      'timezone': instance.timezone,
    };

const _$LocaleDtoEnumMap = {
  LocaleDto.enUs: 'en-US',
  LocaleDto.ar: 'ar',
  LocaleDto.$unknown: r'$unknown',
};
