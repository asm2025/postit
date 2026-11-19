// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'aspect_range.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

AspectRange _$AspectRangeFromJson(Map<String, dynamic> json) {
  $checkKeys(json, requiredKeys: const ['max', 'min', 'preferred']);
  return AspectRange(
    max: AspectRange._read_max(json['max']),
    min: AspectRange._read_min(json['min']),
    preferred: AspectRange._read_preferred(json['preferred']),
    additionalProperties:
        AspectRange._extras(json, 'additionalProperties') == null
        ? const {}
        : AspectRange._readExtras(
            AspectRange._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$AspectRangeToJson(AspectRange instance) =>
    <String, dynamic>{
      'max': AspectRange._write_max(instance.max),
      'min': AspectRange._write_min(instance.min),
      'preferred': AspectRange._write_preferred(instance.preferred),
    };
