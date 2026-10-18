import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

/// Removes the bidi isolate and embedding marks (U+2066 to U+2069) that wrap names in sentences.
String stripIsolates(String s) => s.replaceAll(RegExp('[\u2066-\u2069]'), '');

/// Finds a [Text] whose content equals [expected] once the isolate marks are removed.
Finder findIsolatedText(String expected) => find.byWidgetPredicate(
  (w) => w is Text && stripIsolates(w.data ?? '') == expected,
  description: 'text "$expected"',
);
