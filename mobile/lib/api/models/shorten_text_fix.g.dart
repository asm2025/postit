// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'shorten_text_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ShortenTextFix _$ShortenTextFixFromJson(Map<String, dynamic> json) =>
    ShortenTextFix(
      max: (json['max'] as num).toInt(),
      type: ShortenTextFixType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$ShortenTextFixToJson(ShortenTextFix instance) =>
    <String, dynamic>{
      'max': instance.max,
      'type': _$ShortenTextFixTypeEnumMap[instance.type]!,
    };

const _$ShortenTextFixTypeEnumMap = {
  ShortenTextFixType.shortenText: 'shorten_text',
  ShortenTextFixType.$unknown: r'$unknown',
};
