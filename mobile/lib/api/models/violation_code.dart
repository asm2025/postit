// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// What rule a post broke. The parameters each code carries are listed on the variant;.
/// clients translate by code and fill the text from those parameters.
@JsonEnum()
enum ViolationCode {
  @JsonValue('targets_required')
  targetsRequired('targets_required'),
  @JsonValue('target_duplicate')
  targetDuplicate('target_duplicate'),
  @JsonValue('schedule_in_past')
  scheduleInPast('schedule_in_past'),
  @JsonValue('schedule_time_nonexistent')
  scheduleTimeNonexistent('schedule_time_nonexistent'),
  @JsonValue('schedule_time_ambiguous')
  scheduleTimeAmbiguous('schedule_time_ambiguous'),
  @JsonValue('schedule_zone_unknown')
  scheduleZoneUnknown('schedule_zone_unknown'),
  @JsonValue('native_schedule_outside_window')
  nativeScheduleOutsideWindow('native_schedule_outside_window'),
  @JsonValue('format_unsupported')
  formatUnsupported('format_unsupported'),
  @JsonValue('text_required')
  textRequired('text_required'),
  @JsonValue('text_too_long')
  textTooLong('text_too_long'),
  @JsonValue('media_required')
  mediaRequired('media_required'),
  @JsonValue('media_count')
  mediaCount('media_count'),
  @JsonValue('media_missing')
  mediaMissing('media_missing'),
  @JsonValue('media_kind_not_allowed')
  mediaKindNotAllowed('media_kind_not_allowed'),
  @JsonValue('media_type_not_allowed')
  mediaTypeNotAllowed('media_type_not_allowed'),
  @JsonValue('media_too_large')
  mediaTooLarge('media_too_large'),
  @JsonValue('media_dimensions')
  mediaDimensions('media_dimensions'),
  @JsonValue('media_aspect_ratio')
  mediaAspectRatio('media_aspect_ratio'),
  @JsonValue('media_duration')
  mediaDuration('media_duration'),
  @JsonValue('media_not_ready')
  mediaNotReady('media_not_ready'),
  @JsonValue('reply_to_required')
  replyToRequired('reply_to_required'),
  @JsonValue('reply_to_not_allowed')
  replyToNotAllowed('reply_to_not_allowed'),
  @JsonValue('override_media_not_in_post')
  overrideMediaNotInPost('override_media_not_in_post'),
  @JsonValue('override_media_duplicate')
  overrideMediaDuplicate('override_media_duplicate'),
  @JsonValue('email_document_required')
  emailDocumentRequired('email_document_required'),
  @JsonValue('email_subject_required')
  emailSubjectRequired('email_subject_required'),
  @JsonValue('email_too_large')
  emailTooLarge('email_too_large'),
  @JsonValue('email_postal_address_required')
  emailPostalAddressRequired('email_postal_address_required'),
  @JsonValue('email_merge_tag_invalid')
  emailMergeTagInvalid('email_merge_tag_invalid'),
  @JsonValue('option_unknown')
  optionUnknown('option_unknown'),
  @JsonValue('option_required')
  optionRequired('option_required'),
  @JsonValue('option_type')
  optionType('option_type'),
  @JsonValue('option_not_allowed')
  optionNotAllowed('option_not_allowed'),
  @JsonValue('option_too_short')
  optionTooShort('option_too_short'),
  @JsonValue('option_too_long')
  optionTooLong('option_too_long'),
  @JsonValue('option_out_of_range')
  optionOutOfRange('option_out_of_range'),
  @JsonValue('option_consent_required')
  optionConsentRequired('option_consent_required'),
  @JsonValue('agent_rule')
  agentRule('agent_rule'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const ViolationCode(this.json);

  factory ViolationCode.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<ViolationCode> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
