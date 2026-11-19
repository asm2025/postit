// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'format_selector.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FormatSelector _$FormatSelectorFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['option', 'value'],
    disallowNullValues: const ['option', 'value'],
  );
  return FormatSelector(
    option: json['option'] as String,
    value: json['value'] as String,
    additionalProperties:
        FormatSelector._extras(json, 'additionalProperties') == null
        ? const {}
        : FormatSelector._readExtras(
            FormatSelector._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$FormatSelectorToJson(FormatSelector instance) =>
    <String, dynamic>{'option': instance.option, 'value': instance.value};
