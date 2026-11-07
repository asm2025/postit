// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'last_published_dto.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

LastPublishedDto _$LastPublishedDtoFromJson(Map<String, dynamic> json) =>
    LastPublishedDto(
      accountId: json['account_id'] as String,
      postVersion: (json['post_version'] as num).toInt(),
      publishedAt: DateTime.parse(json['published_at'] as String),
    );

Map<String, dynamic> _$LastPublishedDtoToJson(LastPublishedDto instance) =>
    <String, dynamic>{
      'account_id': instance.accountId,
      'post_version': instance.postVersion,
      'published_at': instance.publishedAt.toIso8601String(),
    };
