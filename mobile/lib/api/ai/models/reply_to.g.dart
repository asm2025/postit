// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reply_to.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ReplyTo _$ReplyToFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['remote_post_id'],
    disallowNullValues: const ['remote_post_id'],
  );
  return ReplyTo(
    remoteCommentId: json['remote_comment_id'] as String?,
    remotePostId: json['remote_post_id'] as String,
    url: json['url'] as String?,
    additionalProperties: ReplyTo._extras(json, 'additionalProperties') == null
        ? const {}
        : ReplyTo._readExtras(ReplyTo._extras(json, 'additionalProperties')),
  );
}

Map<String, dynamic> _$ReplyToToJson(ReplyTo instance) => <String, dynamic>{
  'remote_comment_id': ?instance.remoteCommentId,
  'remote_post_id': instance.remotePostId,
  'url': ?instance.url,
};
