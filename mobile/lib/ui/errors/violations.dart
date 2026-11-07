import 'package:flutter/widgets.dart';
import 'package:postit/api/export.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

/// The user-facing text of a post rule violation, in the language on screen. The server sends
/// the rule's code and parameters; the text is the app's own, so it changes with the language.
String violationMessage(BuildContext context, Violation violation) {
  final l = AppLocalizations.of(context);
  final params = violation.params;
  final p = params is Map ? Map<String, Object?>.from(params) : <String, Object?>{};
  return switch (violation.code) {
    ViolationCode.scheduleInPast => l.violation_schedule_in_past,
    ViolationCode.scheduleTimeNonexistent => l.violation_schedule_time_nonexistent(_text(p['local']), _text(p['zone'])),
    ViolationCode.scheduleTimeAmbiguous => l.violation_schedule_time_ambiguous(
      _text(p['local']),
      _text(p['zone']),
      _text(p['earliest_offset']),
      _text(p['latest_offset']),
    ),
    ViolationCode.scheduleZoneUnknown => l.violation_schedule_zone_unknown(_text(p['zone'])),
    ViolationCode.nativeScheduleOutsideWindow => l.violation_native_schedule_outside_window(
      _text(p['min_ms']),
      _text(p['max_ms']),
    ),
    ViolationCode.formatUnsupported => l.violation_format_unsupported(_text(p['inferred'])),
    ViolationCode.textRequired => l.violation_text_required,
    ViolationCode.textTooLong => l.violation_text_too_long(_text(p['actual']), _text(p['max'])),
    ViolationCode.mediaRequired => l.violation_media_required,
    ViolationCode.mediaCount => l.violation_media_count(_text(p['min']), _text(p['max']), _text(p['actual'])),
    ViolationCode.mediaMissing => l.violation_media_missing,
    ViolationCode.mediaKindNotAllowed => l.violation_media_kind_not_allowed(_text(p['kind']), _text(p['allowed'])),
    ViolationCode.mediaTypeNotAllowed => l.violation_media_type_not_allowed(
      _text(p['content_type']),
      _text(p['allowed']),
    ),
    ViolationCode.mediaTooLarge => l.violation_media_too_large(_text(p['actual_bytes']), _text(p['max_bytes'])),
    ViolationCode.mediaDimensions => l.violation_media_dimensions(_text(p['width']), _text(p['height'])),
    ViolationCode.mediaAspectRatio => l.violation_media_aspect_ratio(
      _text(p['actual']),
      _text(p['min']),
      _text(p['max']),
    ),
    ViolationCode.mediaDuration => l.violation_media_duration(
      _text(p['actual_ms']),
      _text(p['min_ms']),
      _text(p['max_ms']),
    ),
    ViolationCode.mediaNotReady => l.violation_media_not_ready,
    ViolationCode.replyToRequired => l.violation_reply_to_required,
    ViolationCode.replyToNotAllowed => l.violation_reply_to_not_allowed,
    ViolationCode.overrideMediaNotInPost => l.violation_override_media_not_in_post,
    ViolationCode.overrideMediaDuplicate => l.violation_override_media_duplicate,
    ViolationCode.emailDocumentRequired => l.violation_email_document_required,
    ViolationCode.emailSubjectRequired => l.violation_email_subject_required,
    ViolationCode.emailTooLarge => l.violation_email_too_large(_text(p['actual_bytes']), _text(p['max_bytes'])),
    ViolationCode.emailPostalAddressRequired => l.violation_email_postal_address_required,
    ViolationCode.emailMergeTagInvalid => l.violation_email_merge_tag_invalid,
    ViolationCode.optionUnknown => l.violation_option_unknown(_text(p['name'])),
    ViolationCode.optionRequired => l.violation_option_required(_text(p['name'])),
    ViolationCode.optionType => l.violation_option_type(_text(p['name']), _text(p['expected'])),
    ViolationCode.optionNotAllowed => l.violation_option_not_allowed(_text(p['name']), _text(p['allowed'])),
    ViolationCode.optionTooShort => l.violation_option_too_short(_text(p['name']), _text(p['min'])),
    ViolationCode.optionTooLong => l.violation_option_too_long(_text(p['name']), _text(p['max'])),
    ViolationCode.optionOutOfRange => l.violation_option_out_of_range(_text(p['name'])),
    ViolationCode.optionConsentRequired => l.violation_option_consent_required(_text(p['name'])),
    ViolationCode.agentRule => l.violation_agent_rule(_text(p['message'])),
    // A rule this app was built before: the generic message, never the server's English.
    ViolationCode.$unknown => l.error_generic,
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
