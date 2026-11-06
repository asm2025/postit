// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'suggested_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Map<String, dynamic> _$SuggestedFixToJson(SuggestedFix instance) =>
    <String, dynamic>{};

SuggestedFixFitFix _$SuggestedFixFitFixFromJson(Map<String, dynamic> json) =>
    SuggestedFixFitFix(
      aspect: json['aspect'] as String,
      type: FitFixType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$SuggestedFixFitFixToJson(SuggestedFixFitFix instance) =>
    <String, dynamic>{
      'aspect': instance.aspect,
      'type': _$FitFixTypeEnumMap[instance.type]!,
    };

const _$FitFixTypeEnumMap = {
  FitFixType.fit: 'fit',
  FitFixType.$unknown: r'$unknown',
};

SuggestedFixRemoveMediaFix _$SuggestedFixRemoveMediaFixFromJson(
  Map<String, dynamic> json,
) => SuggestedFixRemoveMediaFix(
  index: (json['index'] as num?)?.toInt(),
  type: RemoveMediaFixType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$SuggestedFixRemoveMediaFixToJson(
  SuggestedFixRemoveMediaFix instance,
) => <String, dynamic>{
  'index': instance.index,
  'type': _$RemoveMediaFixTypeEnumMap[instance.type]!,
};

const _$RemoveMediaFixTypeEnumMap = {
  RemoveMediaFixType.removeMedia: 'remove_media',
  RemoveMediaFixType.$unknown: r'$unknown',
};

SuggestedFixShortenTextFix _$SuggestedFixShortenTextFixFromJson(
  Map<String, dynamic> json,
) => SuggestedFixShortenTextFix(
  max: (json['max'] as num).toInt(),
  type: ShortenTextFixType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$SuggestedFixShortenTextFixToJson(
  SuggestedFixShortenTextFix instance,
) => <String, dynamic>{
  'max': instance.max,
  'type': _$ShortenTextFixTypeEnumMap[instance.type]!,
};

const _$ShortenTextFixTypeEnumMap = {
  ShortenTextFixType.shortenText: 'shorten_text',
  ShortenTextFixType.$unknown: r'$unknown',
};

SuggestedFixTrimFix _$SuggestedFixTrimFixFromJson(Map<String, dynamic> json) =>
    SuggestedFixTrimFix(
      maxMs: (json['maxMs'] as num).toInt(),
      type: TrimFixType.fromJson(json['type'] as String),
    );

Map<String, dynamic> _$SuggestedFixTrimFixToJson(
  SuggestedFixTrimFix instance,
) => <String, dynamic>{
  'maxMs': instance.maxMs,
  'type': _$TrimFixTypeEnumMap[instance.type]!,
};

const _$TrimFixTypeEnumMap = {
  TrimFixType.trim: 'trim',
  TrimFixType.$unknown: r'$unknown',
};
