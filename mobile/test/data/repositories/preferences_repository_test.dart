import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:shared_preferences/shared_preferences.dart';

void main() {
  test('defaults to system', () async {
    SharedPreferences.setMockInitialValues({});
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.system);
  });

  test('round-trips the choice under postit.theme', () async {
    SharedPreferences.setMockInitialValues({});
    final repo = await PreferencesRepository.open();
    await repo.setThemeMode(ThemeMode.dark);
    expect((await SharedPreferences.getInstance()).getString('postit.theme'), 'dark');
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.dark);
  });

  test('an unknown stored value falls back to system', () async {
    SharedPreferences.setMockInitialValues({'postit.theme': 'sepia'});
    expect((await PreferencesRepository.open()).themeMode, ThemeMode.system);
  });

  test('without storage the choice is system and saving does nothing', () async {
    final repo = PreferencesRepository(null);
    expect(repo.themeMode, ThemeMode.system);
    await repo.setThemeMode(ThemeMode.light);
    expect(repo.themeMode, ThemeMode.system);
  });
}
