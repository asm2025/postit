import 'package:flutter/material.dart';

/// A small all-caps label. Arabic has no capitals, and letter spacing breaks its joined letters, so
/// there it is plain text with no spacing.
class LabelText extends StatelessWidget {
  const LabelText(this.text, {super.key, this.color});

  final String text;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final arabic = Localizations.localeOf(context).languageCode == 'ar';
    final base = Theme.of(context).textTheme.labelSmall;
    return Text(
      arabic ? text : text.toUpperCase(),
      style: base?.copyWith(color: color, letterSpacing: arabic ? 0 : null),
    );
  }
}
