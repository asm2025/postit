// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'format_spec.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FormatSpec _$FormatSpecFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['id', 'media', 'text'],
    disallowNullValues: const ['id', 'media', 'text'],
  );
  return FormatSpec(
    id: FormatSpec._read_id(json['id']),
    media: MediaRules.fromJson(json['media'] as Map<String, dynamic>),
    selectedBy: json['selected_by'] == null
        ? null
        : FormatSelector.fromJson(json['selected_by'] as Map<String, dynamic>),
    text: TextRules.fromJson(json['text'] as Map<String, dynamic>),
    additionalProperties:
        FormatSpec._extras(json, 'additionalProperties') == null
        ? const {}
        : FormatSpec._readExtras(
            FormatSpec._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$FormatSpecToJson(FormatSpec instance) =>
    <String, dynamic>{
      'id': ?FormatSpec._write_id(instance.id),
      'media': instance.media.toJson(),
      'selected_by': ?instance.selectedBy?.toJson(),
      'text': instance.text.toJson(),
    };
