// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'recurrence_notice.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

RecurrenceNotice _$RecurrenceNoticeFromJson(Map<String, dynamic> json) =>
    RecurrenceNotice(
      postId: json['post_id'] as String,
      reason: SkipReason.fromJson(json['reason'] as String),
      recurrenceId: json['recurrence_id'] as String,
    );

Map<String, dynamic> _$RecurrenceNoticeToJson(RecurrenceNotice instance) =>
    <String, dynamic>{
      'post_id': instance.postId,
      'reason': _$SkipReasonEnumMap[instance.reason]!,
      'recurrence_id': instance.recurrenceId,
    };

const _$SkipReasonEnumMap = {
  SkipReason.aiProviderMissing: 'ai_provider_missing',
  SkipReason.rewriteInvalid: 'rewrite_invalid',
  SkipReason.jobActive: 'job_active',
  SkipReason.$unknown: r'$unknown',
};
