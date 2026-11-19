// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'media_item.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

MediaItem _$MediaItemFromJson(Map<String, dynamic> json) {
  $checkKeys(json, requiredKeys: const ['asset']);
  return MediaItem(
    altText: json['alt_text'] as String?,
    asset: MediaItem._read_asset(json['asset']),
    additionalProperties:
        MediaItem._extras(json, 'additionalProperties') == null
        ? const {}
        : MediaItem._readExtras(
            MediaItem._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$MediaItemToJson(MediaItem instance) => <String, dynamic>{
  'alt_text': ?instance.altText,
  'asset': MediaItem._write_asset(instance.asset),
};
