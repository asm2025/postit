// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'reply_to.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

ReplyTo _$ReplyToFromJson(Map<String, dynamic> json) => ReplyTo(
  remotePostId: json['remote_post_id'] as String,
  remoteCommentId: json['remote_comment_id'] as String?,
  url: json['url'] as String?,
);

Map<String, dynamic> _$ReplyToToJson(ReplyTo instance) => <String, dynamic>{
  'remote_comment_id': instance.remoteCommentId,
  'remote_post_id': instance.remotePostId,
  'url': instance.url,
};
