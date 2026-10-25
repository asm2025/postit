import 'package:flutter/material.dart';
import 'package:intl/intl.dart' show Bidi;

/// The reading direction of user content (a name, an email), taken from the text itself rather than
/// from the screen, or `null` for empty text.
TextDirection? contentDirection(String text) {
  if (text.isEmpty) return null;
  return Bidi.detectRtlDirectionality(text) ? TextDirection.rtl : TextDirection.ltr;
}

/// Wraps user-supplied text (a name) so it keeps its own direction inside a translated sentence: a
/// name in the other script otherwise reorders the punctuation around it. Display only; compare raw text.
String isolate(String text) => '\u2068$text\u2069';

/// Text from a person or an identity provider. It reads in its own direction, and when that differs
/// from the screen's it still sits at the screen's start edge.
class AutoDirectionText extends StatelessWidget {
  const AutoDirectionText(this.text, {super.key, this.style});

  final String text;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final direction = contentDirection(text);
    final screen = Directionality.of(context);
    return Text(
      text,
      style: style,
      textDirection: direction,
      textAlign: direction == null || direction == screen
          ? null
          : screen == TextDirection.rtl
          ? TextAlign.right
          : TextAlign.left,
    );
  }
}
