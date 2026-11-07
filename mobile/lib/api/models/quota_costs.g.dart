// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'quota_costs.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

QuotaCosts _$QuotaCostsFromJson(Map<String, dynamic> json) => QuotaCosts(
  comment: (json['comment'] as num).toInt(),
  inboxPoll: (json['inbox_poll'] as num).toInt(),
  metrics: (json['metrics'] as num).toInt(),
  publish: (json['publish'] as num).toInt(),
);

Map<String, dynamic> _$QuotaCostsToJson(QuotaCosts instance) =>
    <String, dynamic>{
      'comment': instance.comment,
      'inbox_poll': instance.inboxPoll,
      'metrics': instance.metrics,
      'publish': instance.publish,
    };
