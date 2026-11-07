// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_rules.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaRules _$MediaRulesFromJson(Map<String, dynamic> json) => MediaRules(
  aspectRatios: (json['aspect_ratios'] as List<dynamic>)
      .map((e) => AspectRange.fromJson(e as Map<String, dynamic>))
      .toList(),
  kinds: (json['kinds'] as List<dynamic>)
      .map((e) => MediaKind.fromJson(e as String))
      .toList(),
  maxCount: (json['max_count'] as num).toInt(),
  mimeTypes: (json['mime_types'] as List<dynamic>)
      .map((e) => e as String)
      .toList(),
  minCount: (json['min_count'] as num).toInt(),
  safeZones: (json['safe_zones'] as List<dynamic>)
      .map((e) => SafeZone.fromJson(e as Map<String, dynamic>))
      .toList(),
  maxBytes: (json['max_bytes'] as num?)?.toInt(),
  maxDurationMs: (json['max_duration_ms'] as num?)?.toInt(),
  maxHeight: (json['max_height'] as num?)?.toInt(),
  maxWidth: (json['max_width'] as num?)?.toInt(),
  minDurationMs: (json['min_duration_ms'] as num?)?.toInt(),
  minHeight: (json['min_height'] as num?)?.toInt(),
  minWidth: (json['min_width'] as num?)?.toInt(),
);

Map<String, dynamic> _$MediaRulesToJson(MediaRules instance) =>
    <String, dynamic>{
      'aspect_ratios': instance.aspectRatios,
      'kinds': instance.kinds.map((e) => _$MediaKindEnumMap[e]!).toList(),
      'max_bytes': instance.maxBytes,
      'max_count': instance.maxCount,
      'max_duration_ms': instance.maxDurationMs,
      'max_height': instance.maxHeight,
      'max_width': instance.maxWidth,
      'mime_types': instance.mimeTypes,
      'min_count': instance.minCount,
      'min_duration_ms': instance.minDurationMs,
      'min_height': instance.minHeight,
      'min_width': instance.minWidth,
      'safe_zones': instance.safeZones,
    };

const _$MediaKindEnumMap = {
  MediaKind.image: 'image',
  MediaKind.video: 'video',
  MediaKind.$unknown: r'$unknown',
};
