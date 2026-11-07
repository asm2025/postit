// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'violation.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

Violation _$ViolationFromJson(Map<String, dynamic> json) => Violation(
  code: ViolationCode.fromJson(json['code'] as String),
  fix: json['fix'] == null
      ? null
      : SuggestedFix.fromJson(json['fix'] as Map<String, dynamic>),
  params: json['params'],
  path: json['path'] as String?,
);

Map<String, dynamic> _$ViolationToJson(Violation instance) => <String, dynamic>{
  'code': _$ViolationCodeEnumMap[instance.code]!,
  'fix': instance.fix,
  'params': instance.params,
  'path': instance.path,
};

const _$ViolationCodeEnumMap = {
  ViolationCode.scheduleInPast: 'schedule_in_past',
  ViolationCode.scheduleTimeNonexistent: 'schedule_time_nonexistent',
  ViolationCode.scheduleTimeAmbiguous: 'schedule_time_ambiguous',
  ViolationCode.scheduleZoneUnknown: 'schedule_zone_unknown',
  ViolationCode.nativeScheduleOutsideWindow: 'native_schedule_outside_window',
  ViolationCode.formatUnsupported: 'format_unsupported',
  ViolationCode.textRequired: 'text_required',
  ViolationCode.textTooLong: 'text_too_long',
  ViolationCode.mediaRequired: 'media_required',
  ViolationCode.mediaCount: 'media_count',
  ViolationCode.mediaMissing: 'media_missing',
  ViolationCode.mediaKindNotAllowed: 'media_kind_not_allowed',
  ViolationCode.mediaTypeNotAllowed: 'media_type_not_allowed',
  ViolationCode.mediaTooLarge: 'media_too_large',
  ViolationCode.mediaDimensions: 'media_dimensions',
  ViolationCode.mediaAspectRatio: 'media_aspect_ratio',
  ViolationCode.mediaDuration: 'media_duration',
  ViolationCode.mediaNotReady: 'media_not_ready',
  ViolationCode.replyToRequired: 'reply_to_required',
  ViolationCode.replyToNotAllowed: 'reply_to_not_allowed',
  ViolationCode.overrideMediaNotInPost: 'override_media_not_in_post',
  ViolationCode.overrideMediaDuplicate: 'override_media_duplicate',
  ViolationCode.emailDocumentRequired: 'email_document_required',
  ViolationCode.emailSubjectRequired: 'email_subject_required',
  ViolationCode.emailTooLarge: 'email_too_large',
  ViolationCode.emailPostalAddressRequired: 'email_postal_address_required',
  ViolationCode.emailMergeTagInvalid: 'email_merge_tag_invalid',
  ViolationCode.optionUnknown: 'option_unknown',
  ViolationCode.optionRequired: 'option_required',
  ViolationCode.optionType: 'option_type',
  ViolationCode.optionNotAllowed: 'option_not_allowed',
  ViolationCode.optionTooShort: 'option_too_short',
  ViolationCode.optionTooLong: 'option_too_long',
  ViolationCode.optionOutOfRange: 'option_out_of_range',
  ViolationCode.optionConsentRequired: 'option_consent_required',
  ViolationCode.agentRule: 'agent_rule',
  ViolationCode.$unknown: r'$unknown',
};
