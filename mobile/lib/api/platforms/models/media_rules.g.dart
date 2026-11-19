// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_rules.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaRules _$MediaRulesFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const [
      'aspect_ratios',
      'kinds',
      'max_count',
      'mime_types',
      'min_count',
      'safe_zones',
    ],
    disallowNullValues: const [
      'aspect_ratios',
      'kinds',
      'max_count',
      'mime_types',
      'min_count',
      'safe_zones',
    ],
  );
  return MediaRules(
    aspectRatios: (json['aspect_ratios'] as List<dynamic>)
        .map((e) => AspectRange.fromJson(e as Map<String, dynamic>))
        .toList(),
    kinds: MediaRules._read_kinds(json['kinds']),
    maxBytes: MediaRules._read_maxBytes(json['max_bytes']),
    maxCount: MediaRules._read_maxCount(json['max_count']),
    maxDurationMs: MediaRules._read_maxDurationMs(json['max_duration_ms']),
    maxHeight: MediaRules._read_maxHeight(json['max_height']),
    maxWidth: MediaRules._read_maxWidth(json['max_width']),
    mimeTypes: (json['mime_types'] as List<dynamic>)
        .map((e) => e as String)
        .toList(),
    minCount: MediaRules._read_minCount(json['min_count']),
    minDurationMs: MediaRules._read_minDurationMs(json['min_duration_ms']),
    minHeight: MediaRules._read_minHeight(json['min_height']),
    minWidth: MediaRules._read_minWidth(json['min_width']),
    safeZones: (json['safe_zones'] as List<dynamic>)
        .map((e) => SafeZone.fromJson(e as Map<String, dynamic>))
        .toList(),
    additionalProperties:
        MediaRules._extras(json, 'additionalProperties') == null
        ? const {}
        : MediaRules._readExtras(
            MediaRules._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$MediaRulesToJson(
  MediaRules instance,
) => <String, dynamic>{
  'aspect_ratios': instance.aspectRatios.map((e) => e.toJson()).toList(),
  'kinds': ?MediaRules._write_kinds(instance.kinds),
  'max_bytes': ?MediaRules._write_maxBytes(instance.maxBytes),
  'max_count': ?MediaRules._write_maxCount(instance.maxCount),
  'max_duration_ms': ?MediaRules._write_maxDurationMs(instance.maxDurationMs),
  'max_height': ?MediaRules._write_maxHeight(instance.maxHeight),
  'max_width': ?MediaRules._write_maxWidth(instance.maxWidth),
  'mime_types': instance.mimeTypes,
  'min_count': ?MediaRules._write_minCount(instance.minCount),
  'min_duration_ms': ?MediaRules._write_minDurationMs(instance.minDurationMs),
  'min_height': ?MediaRules._write_minHeight(instance.minHeight),
  'min_width': ?MediaRules._write_minWidth(instance.minWidth),
  'safe_zones': instance.safeZones.map((e) => e.toJson()).toList(),
};
