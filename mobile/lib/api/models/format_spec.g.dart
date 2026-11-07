// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'format_spec.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

FormatSpec _$FormatSpecFromJson(Map<String, dynamic> json) => FormatSpec(
  id: FormatId.fromJson(json['id'] as String),
  media: MediaRules.fromJson(json['media'] as Map<String, dynamic>),
  text: TextRules.fromJson(json['text'] as Map<String, dynamic>),
  selectedBy: json['selected_by'] == null
      ? null
      : FormatSelector.fromJson(json['selected_by'] as Map<String, dynamic>),
);

Map<String, dynamic> _$FormatSpecToJson(FormatSpec instance) =>
    <String, dynamic>{
      'id': _$FormatIdEnumMap[instance.id]!,
      'media': instance.media,
      'selected_by': instance.selectedBy,
      'text': instance.text,
    };

const _$FormatIdEnumMap = {
  FormatId.text: 'text',
  FormatId.link: 'link',
  FormatId.image: 'image',
  FormatId.carousel: 'carousel',
  FormatId.video: 'video',
  FormatId.shortVideo: 'short_video',
  FormatId.story: 'story',
  FormatId.reel: 'reel',
  FormatId.htmlEmail: 'html_email',
  FormatId.$unknown: r'$unknown',
};
