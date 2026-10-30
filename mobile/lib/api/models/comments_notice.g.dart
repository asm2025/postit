// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'comments_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

CommentsNotice _$CommentsNoticeFromJson(Map<String, dynamic> json) =>
    CommentsNotice(
      account: AccountLabel.fromJson(json['account'] as Map<String, dynamic>),
      count: (json['count'] as num).toInt(),
    );

Map<String, dynamic> _$CommentsNoticeToJson(CommentsNotice instance) =>
    <String, dynamic>{'account': instance.account, 'count': instance.count};
