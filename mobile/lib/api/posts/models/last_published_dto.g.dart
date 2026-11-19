// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'last_published_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LastPublishedDto _$LastPublishedDtoFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['account_id', 'post_version', 'published_at'],
    disallowNullValues: const ['account_id', 'post_version', 'published_at'],
  );
  return LastPublishedDto(
    accountId: json['account_id'] as String,
    postVersion: LastPublishedDto._read_postVersion(json['post_version']),
    publishedAt: LastPublishedDto._read_publishedAt(json['published_at']),
    additionalProperties:
        LastPublishedDto._extras(json, 'additionalProperties') == null
        ? const {}
        : LastPublishedDto._readExtras(
            LastPublishedDto._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$LastPublishedDtoToJson(
  LastPublishedDto instance,
) => <String, dynamic>{
  'account_id': instance.accountId,
  'post_version': ?LastPublishedDto._write_postVersion(instance.postVersion),
  'published_at': ?LastPublishedDto._write_publishedAt(instance.publishedAt),
};
