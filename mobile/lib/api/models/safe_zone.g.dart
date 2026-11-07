// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'safe_zone.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SafeZone _$SafeZoneFromJson(Map<String, dynamic> json) => SafeZone(
  height: (json['height'] as num).toDouble(),
  label: Message.fromJson(json['label'] as Map<String, dynamic>),
  width: (json['width'] as num).toDouble(),
  x: (json['x'] as num).toDouble(),
  y: (json['y'] as num).toDouble(),
);

Map<String, dynamic> _$SafeZoneToJson(SafeZone instance) => <String, dynamic>{
  'height': instance.height,
  'label': instance.label,
  'width': instance.width,
  'x': instance.x,
  'y': instance.y,
};
