// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'suggested_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FitFix _$FitFixFromJson(Map<String, dynamic> json) {
  $checkKeys(json, requiredKeys: const ['aspect']);
  return FitFix(
    aspect: FitFix._read_aspect(json['aspect']),
    additionalProperties: FitFix._extras(json, 'additionalProperties') == null
        ? const {}
        : FitFix._readExtras(FitFix._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$FitFixToJson(FitFix instance) => <String, dynamic>{
  'aspect': FitFix._write_aspect(instance.aspect),
};

TrimFix _$TrimFixFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['max_ms'],
    disallowNullValues: const ['max_ms'],
  );
  return TrimFix(
    maxMs: TrimFix._read_maxMs(json['max_ms']),
    additionalProperties: TrimFix._extras(json, 'additionalProperties') == null
        ? const {}
        : TrimFix._readExtras(TrimFix._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$TrimFixToJson(TrimFix instance) => <String, dynamic>{
  'max_ms': ?TrimFix._write_maxMs(instance.maxMs),
};

RemoveMediaFix _$RemoveMediaFixFromJson(Map<String, dynamic> json) =>
    RemoveMediaFix(
      index: RemoveMediaFix._read_index(json['index']),
      additionalProperties:
          RemoveMediaFix._extras(json, 'additionalProperties') == null
          ? const {}
          : RemoveMediaFix._readExtras(
              RemoveMediaFix._extras(json, 'additionalProperties'),
            ),
    );

Map<String, dynamic> _$RemoveMediaFixToJson(RemoveMediaFix instance) =>
    <String, dynamic>{'index': ?RemoveMediaFix._write_index(instance.index)};

ShortenTextFix _$ShortenTextFixFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['max'],
    disallowNullValues: const ['max'],
  );
  return ShortenTextFix(
    max: ShortenTextFix._read_max(json['max']),
    additionalProperties:
        ShortenTextFix._extras(json, 'additionalProperties') == null
        ? const {}
        : ShortenTextFix._readExtras(
            ShortenTextFix._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$ShortenTextFixToJson(ShortenTextFix instance) =>
    <String, dynamic>{'max': ?ShortenTextFix._write_max(instance.max)};
