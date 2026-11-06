import 'package:flutter/widgets.dart';
import 'package:intl/intl.dart';
import 'package:postit/l10n/gen/app_localizations.dart';

String _tag(BuildContext context) => Localizations.localeOf(context).languageCode;

String formatNumber(BuildContext context, num value) => NumberFormat.decimalPattern(_tag(context)).format(value);

/// [pattern] is an intl skeleton or pattern; the default is a medium date. Shown in the device time zone.
String formatDate(BuildContext context, DateTime value, {String? pattern}) {
  final tag = _tag(context);
  final format = pattern == null ? DateFormat.yMMMd(tag) : DateFormat(pattern, tag);
  return format.format(value.toLocal());
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
  return text;
}
