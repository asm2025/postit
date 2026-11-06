import 'package:flutter/material.dart';

/// A small all-caps label. Right-to-left scripts often have no capitals, and letter spacing breaks
/// joined letters, so right to left it is plain text with no spacing.
class LabelText extends StatelessWidget {
  const LabelText(this.text, {super.key, this.color});

  final String text;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final rtl = Directionality.of(context) == TextDirection.rtl;
    final base = Theme.of(context).textTheme.labelSmall;
    return Text(
      rtl ? text : text.toUpperCase(),
      style: base?.copyWith(color: color, letterSpacing: rtl ? 0 : null),
    );
  }
}
