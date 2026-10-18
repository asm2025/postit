import 'package:flutter/widgets.dart';
import 'package:intl/intl.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

String _tag(BuildContext context) => Localizations.localeOf(context).languageCode;

/// Arabic UI shows Arabic-Indic digits for every formatted quantity. intl's `ar` symbols already do
/// for numbers, but not for every date pattern, so map any Western digit left over.
String _digits(String languageCode, String s) {
  if (languageCode != 'ar') return s;
  return s.replaceAllMapped(RegExp('[0-9]'), (m) => String.fromCharCode(0x0660 + m[0]!.codeUnitAt(0) - 0x30));
}

String formatNumber(BuildContext context, num value) {
  final tag = _tag(context);
  final text = NumberFormat.decimalPattern(tag).format(value);
  // intl's `ar` data has Arabic-Indic digits but ASCII separators; use the Arabic ones (U+066C, U+066B).
  return _digits(tag, tag == 'ar' ? text.replaceAll(',', '\u066C').replaceAll('.', '\u066B') : text);
}

/// [pattern] is an intl skeleton or pattern; the default is a medium date. Shown in the device time zone.
String formatDate(BuildContext context, DateTime value, {String? pattern}) {
  final tag = _tag(context);
  final format = pattern == null ? DateFormat.yMMMd(tag) : DateFormat(pattern, tag);
  return _digits(tag, format.format(value.toLocal()));
}

/// "just now", "5 minutes ago", "3 days ago"; older than a week falls back to the date.
String formatRelative(BuildContext context, DateTime value, {DateTime? now}) {
  final l = AppLocalizations.of(context);
  final diff = (now ?? DateTime.now()).difference(value);
  final seconds = diff.inSeconds;
  if (seconds < 60) return l.relative_just_now;
  final text = switch (seconds) {
    < 3600 => l.relative_minutes_ago((seconds / 60).round()),
    < 86400 => l.relative_hours_ago((seconds / 3600).round()),
    < 7 * 86400 => l.relative_days_ago((seconds / 86400).round()),
    _ => formatDate(context, value),
  };
  return _digits(_tag(context), text);
}
