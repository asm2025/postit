// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'safe_zone.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

SafeZone _$SafeZoneFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['height', 'label', 'width', 'x', 'y'],
    disallowNullValues: const ['height', 'label', 'width', 'x', 'y'],
  );
  return SafeZone(
    height: json['height'] as num,
    label: Message.fromJson(json['label'] as Map<String, dynamic>),
    width: json['width'] as num,
    x: json['x'] as num,
    y: json['y'] as num,
    additionalProperties: SafeZone._extras(json, 'additionalProperties') == null
        ? const {}
        : SafeZone._readExtras(SafeZone._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$SafeZoneToJson(SafeZone instance) => <String, dynamic>{
  'height': instance.height,
  'label': instance.label.toJson(),
  'width': instance.width,
  'x': instance.x,
  'y': instance.y,
};
