// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'preferences_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PreferencesDto _$PreferencesDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'comment_digest',
      'email_notifications',
      'locale',
      'store_ai_prompts',
      'switches',
      'timezone',
    ],
    disallowNullValues: const [
      'comment_digest',
      'email_notifications',
      'locale',
      'store_ai_prompts',
      'switches',
      'timezone',
    ],
  );
  return PreferencesDto(
    commentDigest: json['comment_digest'] as bool,
    emailNotifications: json['email_notifications'] as bool,
    locale: PreferencesDto._read_locale(json['locale']),
    storeAiPrompts: json['store_ai_prompts'] as bool,
    switches: (json['switches'] as List<dynamic>)
        .map((e) => KindSwitchDto.fromJson(e as Map<String, dynamic>))
        .toList(),
    timezone: json['timezone'] as String,
    additionalProperties:
        PreferencesDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PreferencesDto._readExtras(
            PreferencesDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PreferencesDtoToJson(PreferencesDto instance) =>
    <String, dynamic>{
      'comment_digest': instance.commentDigest,
      'email_notifications': instance.emailNotifications,
      'locale': ?PreferencesDto._write_locale(instance.locale),
      'store_ai_prompts': instance.storeAiPrompts,
      'switches': instance.switches.map((e) => e.toJson()).toList(),
      'timezone': instance.timezone,
    };
