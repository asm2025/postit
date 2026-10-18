// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'preferences_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

PreferencesDto _$PreferencesDtoFromJson(Map<String, dynamic> json) =>
    PreferencesDto(locale: LocaleDto.fromJson(json['locale'] as String));

Map<String, dynamic> _$PreferencesDtoToJson(PreferencesDto instance) =>
    <String, dynamic>{'locale': _$LocaleDtoEnumMap[instance.locale]!};

const _$LocaleDtoEnumMap = {
  LocaleDto.enUs: 'en-US',
  LocaleDto.ar: 'ar',
  LocaleDto.$unknown: r'$unknown',
};
