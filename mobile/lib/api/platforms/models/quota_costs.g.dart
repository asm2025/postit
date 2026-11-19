// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'quota_costs.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

QuotaCosts _$QuotaCostsFromJson(Map<String, dynamic> json) {
  $checkKeys(
    json,
    requiredKeys: const ['comment', 'inbox_poll', 'metrics', 'publish'],
    disallowNullValues: const ['comment', 'inbox_poll', 'metrics', 'publish'],
  );
  return QuotaCosts(
    comment: QuotaCosts._read_comment(json['comment']),
    inboxPoll: QuotaCosts._read_inboxPoll(json['inbox_poll']),
    metrics: QuotaCosts._read_metrics(json['metrics']),
    publish: QuotaCosts._read_publish(json['publish']),
    additionalProperties:
        QuotaCosts._extras(json, 'additionalProperties') == null
        ? const {}
        : QuotaCosts._readExtras(
            QuotaCosts._extras(json, 'additionalProperties'),
          ),
  );
}

Map<String, dynamic> _$QuotaCostsToJson(QuotaCosts instance) =>
    <String, dynamic>{
      'comment': ?QuotaCosts._write_comment(instance.comment),
      'inbox_poll': ?QuotaCosts._write_inboxPoll(instance.inboxPoll),
      'metrics': ?QuotaCosts._write_metrics(instance.metrics),
      'publish': ?QuotaCosts._write_publish(instance.publish),
    };
