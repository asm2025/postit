// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'fit_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FitFix _$FitFixFromJson(Map<String, dynamic> json) => FitFix(
  aspect: json['aspect'] as String,
  type: FitFixType.fromJson(json['type'] as String),
);

Map<String, dynamic> _$FitFixToJson(FitFix instance) => <String, dynamic>{
  'aspect': instance.aspect,
  'type': _$FitFixTypeEnumMap[instance.type]!,
};

const _$FitFixTypeEnumMap = {
  FitFixType.fit: 'fit',
  FitFixType.$unknown: r'$unknown',
};
