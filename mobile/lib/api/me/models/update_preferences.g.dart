// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'update_preferences.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

UpdatePreferences _$UpdatePreferencesFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    allowedKeys: const [
      'comment_digest',
      'email_notifications',
      'locale',
      'store_ai_prompts',
      'switches',
      'timezone',
    ],
  );
  return UpdatePreferences(
    commentDigest: json['comment_digest'] as bool?,
    emailNotifications: json['email_notifications'] as bool?,
    locale: UpdatePreferences._read_locale(json['locale']),
    storeAiPrompts: json['store_ai_prompts'] as bool?,
    switches: (json['switches'] as List<dynamic>?)
        ?.map((e) => KindSwitchUpdate.fromJson(e as Map<String, dynamic>))
        .toList(),
    timezone: json['timezone'] as String?,
  );
}

Map<String, dynamic> _$UpdatePreferencesToJson(UpdatePreferences instance) =>
    <String, dynamic>{
      'comment_digest': ?instance.commentDigest,
      'email_notifications': ?instance.emailNotifications,
      'locale': ?UpdatePreferences._write_locale(instance.locale),
      'store_ai_prompts': ?instance.storeAiPrompts,
      'switches': ?instance.switches?.map((e) => e.toJson()).toList(),
      'timezone': ?instance.timezone,
    };
