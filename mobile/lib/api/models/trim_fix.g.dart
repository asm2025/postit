// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'trim_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

TrimFix _$TrimFixFromJson(Map<String, dynamic> json) => TrimFix(
  maxMs: (json['max_ms'] as num).toInt(),
  type: TrimFixType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$TrimFixToJson(TrimFix instance) => <String, dynamic>{
  'max_ms': instance.maxMs,
  'type': _$TrimFixTypeEnumMap[instance.type]!,
};

const _$TrimFixTypeEnumMap = {
  TrimFixType.trim: 'trim',
  TrimFixType.$unknown: r'$unknown',
};
