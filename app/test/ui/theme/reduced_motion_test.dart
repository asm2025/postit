import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/ui/theme/app_theme.dart';

void main() {
  // MaterialApp reads MediaQuery from the view, so the OS setting is faked on the dispatcher.
  Future<void> pushSecond(WidgetTester tester, {required bool reduce}) async {
    tester.platformDispatcher.accessibilityFeaturesTestValue = FakeAccessibilityFeatures(disableAnimations: reduce);
    addTearDown(tester.platformDispatcher.clearAccessibilityFeaturesTestValue);
    final nav = GlobalKey<NavigatorState>();
    await tester.pumpWidget(
      MaterialApp(navigatorKey: nav, theme: buildTheme(Brightness.light), home: const Text('first')),
    );
    nav.currentState!.push(MaterialPageRoute<void>(builder: (_) => const Text('second')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));
  }

  testWidgets('reduced motion shows the next page without a transition', (tester) async {
    await pushSecond(tester, reduce: true);
    // The route's controller still ticks; what matters is that nothing on screen moves.
    final second = tester.getTopLeft(find.text('second'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(tester.getTopLeft(find.text('second')), second);
  });

  testWidgets('otherwise the platform transition runs', (tester) async {
    await pushSecond(tester, reduce: false);
    expect(tester.hasRunningAnimations, isTrue);
  });
}
