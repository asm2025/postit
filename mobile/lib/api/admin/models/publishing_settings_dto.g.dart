// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publishing_settings_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishingSettingsDto _$PublishingSettingsDtoFromJson(
  Map<String, dynamic> json,
) {
  $checkKeys(
    json,
    requiredKeys: const ['live', 'native_scheduled_count'],
    disallowNullValues: const ['live', 'native_scheduled_count'],
  );
  return PublishingSettingsDto(
    live: json['live'] as bool,
    nativeScheduledCount: PublishingSettingsDto._read_nativeScheduledCount(
      json['native_scheduled_count'],
    ),
    additionalProperties:
        PublishingSettingsDto._extras(json, 'additionalProperties') == null
        ? const {}
        : PublishingSettingsDto._readExtras(
            PublishingSettingsDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$PublishingSettingsDtoToJson(
  PublishingSettingsDto instance,
) => <String, dynamic>{
  'live': instance.live,
  'native_scheduled_count': ?PublishingSettingsDto._write_nativeScheduledCount(
    instance.nativeScheduledCount,
  ),
};
