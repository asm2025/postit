import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/l10n/gen/app_localizations.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/state/theme.dart';
import 'package:postit/ui/widgets/theme_control.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  Future<ProviderContainer> pump(WidgetTester tester, PreferencesRepository prefs) async {
    final container = ProviderContainer(overrides: [preferencesProvider.overrideWithValue(prefs)]);
    addTearDown(container.dispose);
    await tester.pumpWidget(
      UncontrolledProviderScope(
        container: container,
        child: const MaterialApp(
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: ThemeControl()),
        ),
      ),
    );
    return container;
  }

  testWidgets('choosing dark applies and persists it', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final container = await pump(tester, prefs);
    expect(container.read(themeModeProvider), ThemeMode.system);
    await tester.tap(find.byTooltip('Dark'));
    await tester.pump();
    expect(container.read(themeModeProvider), ThemeMode.dark);
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.dark);
  });

  testWidgets('without storage the control still switches', (tester) async {
    final container = await pump(tester, PreferencesRepository(null));
    await tester.tap(find.byTooltip('Light'));
    await tester.pump();
    expect(container.read(themeModeProvider), ThemeMode.light);
  });
}
