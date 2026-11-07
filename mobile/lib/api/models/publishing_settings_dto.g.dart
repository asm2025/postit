// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'publishing_settings_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PublishingSettingsDto _$PublishingSettingsDtoFromJson(
  Map<String, dynamic> json,
) => PublishingSettingsDto(
  live: json['live'] as bool,
  nativeScheduledCount: (json['native_scheduled_count'] as num).toInt(),
);

Map<String, dynamic> _$PublishingSettingsDtoToJson(
  PublishingSettingsDto instance,
) => <String, dynamic>{
  'live': instance.live,
  'native_scheduled_count': instance.nativeScheduledCount,
};
