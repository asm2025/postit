// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'text_rules.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TextRules _$TextRulesFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['max', 'measure', 'required'],
    disallowNullValues: const [
      'includes_link_and_tags',
      'max',
      'measure',
      'required',
    ],
  );
  return TextRules(
    includesLinkAndTags: json['includes_link_and_tags'] as bool?,
    max: TextRules._read_max(json['max']),
    measure: TextRules._read_measure(json['measure']),
    required2: json['required'] as bool,
    additionalProperties:
        TextRules._extras(json, 'additionalProperties') == null
        ? const {}
        : TextRules._readExtras(
            TextRules._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$TextRulesToJson(TextRules instance) => <String, dynamic>{
  'includes_link_and_tags': ?instance.includesLinkAndTags,
  'max': ?TextRules._write_max(instance.max),
  'measure': ?TextRules._write_measure(instance.measure),
  'required': instance.required2,
};
