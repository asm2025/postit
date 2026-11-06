// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'remove_media_fix.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RemoveMediaFix _$RemoveMediaFixFromJson(Map<String, dynamic> json) =>
    RemoveMediaFix(
      type: RemoveMediaFixType.fromJson(json['type'] as String),
      index: (json['index'] as num?)?.toInt(),
    );

Map<String, dynamic> _$RemoveMediaFixToJson(RemoveMediaFix instance) =>
    <String, dynamic>{
      'index': instance.index,
      'type': _$RemoveMediaFixTypeEnumMap[instance.type]!,
    };

const _$RemoveMediaFixTypeEnumMap = {
  RemoveMediaFixType.removeMedia: 'remove_media',
  RemoveMediaFixType.$unknown: r'$unknown',
};
