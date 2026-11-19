import 'package:flutter/widgets.dart';
import 'package:postit/api/exports.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

/// The user-facing text of a post rule violation, in the language on screen. The server sends
/// the rule's code and parameters; the text is the app's own, so it changes with the language.
String violationMessage(BuildContext context, Violation violation) {
  final l = AppLocalizations.of(context);
  final p = violation.params?.additionalProperties ?? const <String, Object?>{};
  final code = switch (violation.code) {
    ViolationCodeKnown(:final value) => value,
    // A rule this app was built before: the generic message, never the server's English.
    ViolationCodeUnknown() => null,
  };
  return switch (code) {
    null => l.error_generic,
    ViolationCodeValue.targetsRequired => l.violation_targets_required,
    ViolationCodeValue.targetDuplicate => l.violation_target_duplicate,
    ViolationCodeValue.scheduleInPast => l.violation_schedule_in_past,
    ViolationCodeValue.scheduleTimeNonexistent => l.violation_schedule_time_nonexistent(
      _text(p['local']),
      _text(p['zone']),
    ),
    ViolationCodeValue.scheduleTimeAmbiguous => l.violation_schedule_time_ambiguous(
      _text(p['local']),
      _text(p['zone']),
      _text(p['earliest_offset']),
      _text(p['latest_offset']),
    ),
    ViolationCodeValue.scheduleZoneUnknown => l.violation_schedule_zone_unknown(_text(p['zone'])),
    ViolationCodeValue.nativeScheduleOutsideWindow => l.violation_native_schedule_outside_window(
      _text(p['min_ms']),
      _text(p['max_ms']),
    ),
    ViolationCodeValue.formatUnsupported => l.violation_format_unsupported(_text(p['inferred'])),
    ViolationCodeValue.textRequired => l.violation_text_required,
    ViolationCodeValue.textTooLong => l.violation_text_too_long(_text(p['actual']), _text(p['max'])),
    ViolationCodeValue.mediaRequired => l.violation_media_required,
    ViolationCodeValue.mediaCount => l.violation_media_count(_text(p['min']), _text(p['max']), _text(p['actual'])),
    ViolationCodeValue.mediaMissing => l.violation_media_missing,
    ViolationCodeValue.mediaKindNotAllowed => l.violation_media_kind_not_allowed(_text(p['kind']), _text(p['allowed'])),
    ViolationCodeValue.mediaTypeNotAllowed => l.violation_media_type_not_allowed(
      _text(p['content_type']),
      _text(p['allowed']),
    ),
    ViolationCodeValue.mediaTooLarge => l.violation_media_too_large(_text(p['actual_bytes']), _text(p['max_bytes'])),
    ViolationCodeValue.mediaDimensions => l.violation_media_dimensions(_text(p['width']), _text(p['height'])),
    ViolationCodeValue.mediaAspectRatio => l.violation_media_aspect_ratio(
      _text(p['actual']),
      _text(p['min']),
      _text(p['max']),
    ),
    ViolationCodeValue.mediaDuration => l.violation_media_duration(
      _text(p['actual_ms']),
      _text(p['min_ms']),
      _text(p['max_ms']),
    ),
    ViolationCodeValue.mediaNotReady => l.violation_media_not_ready,
    ViolationCodeValue.replyToRequired => l.violation_reply_to_required,
    ViolationCodeValue.replyToNotAllowed => l.violation_reply_to_not_allowed,
    ViolationCodeValue.overrideMediaNotInPost => l.violation_override_media_not_in_post,
    ViolationCodeValue.overrideMediaDuplicate => l.violation_override_media_duplicate,
    ViolationCodeValue.emailDocumentRequired => l.violation_email_document_required,
    ViolationCodeValue.emailSubjectRequired => l.violation_email_subject_required,
    ViolationCodeValue.emailTooLarge => l.violation_email_too_large(_text(p['actual_bytes']), _text(p['max_bytes'])),
    ViolationCodeValue.emailPostalAddressRequired => l.violation_email_postal_address_required,
    ViolationCodeValue.emailMergeTagInvalid => l.violation_email_merge_tag_invalid,
    ViolationCodeValue.optionUnknown => l.violation_option_unknown(_text(p['name'])),
    ViolationCodeValue.optionRequired => l.violation_option_required(_text(p['name'])),
    ViolationCodeValue.optionType => l.violation_option_type(_text(p['name']), _text(p['expected'])),
    ViolationCodeValue.optionNotAllowed => l.violation_option_not_allowed(_text(p['name']), _text(p['allowed'])),
    ViolationCodeValue.optionTooShort => l.violation_option_too_short(_text(p['name']), _text(p['min'])),
    ViolationCodeValue.optionTooLong => l.violation_option_too_long(_text(p['name']), _text(p['max'])),
    ViolationCodeValue.optionOutOfRange => l.violation_option_out_of_range(_text(p['name'])),
    ViolationCodeValue.optionConsentRequired => l.violation_option_consent_required(_text(p['name'])),
    ViolationCodeValue.agentRule => l.violation_agent_rule(_text(p['message'])),
  };
}

/// A parameter as text: a list reads as a comma separated list, and an agent's own message as
/// its English text.
String _text(Object? value) {
  if (value == null) return '';
  if (value is List) return value.map(_text).join(', ');
  if (value is Map) return _text(value['en']);
  return '$value';
}
