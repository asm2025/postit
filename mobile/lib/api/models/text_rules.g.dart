// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'text_rules.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TextRules _$TextRulesFromJson(Map<String, dynamic> json) => TextRules(
  max: (json['max'] as num).toInt(),
  measure: TextMeasure.fromJson(json['measure'] as String),
  requiredValue: json['required'] as bool,
  includesLinkAndTags: json['includes_link_and_tags'] as bool?,
);

Map<String, dynamic> _$TextRulesToJson(TextRules instance) => <String, dynamic>{
  'includes_link_and_tags': instance.includesLinkAndTags,
  'max': instance.max,
  'measure': _$TextMeasureEnumMap[instance.measure]!,
  'required': instance.requiredValue,
};

const _$TextMeasureEnumMap = {
  TextMeasure.graphemes: 'graphemes',
  TextMeasure.xWeighted: 'x_weighted',
  TextMeasure.utf16Units: 'utf16_units',
  TextMeasure.bytes: 'bytes',
  TextMeasure.$unknown: r'$unknown',
};
