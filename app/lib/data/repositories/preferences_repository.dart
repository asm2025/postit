import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Device-local preferences. Every read and write tolerates storage being unavailable.
class PreferencesRepository {
  PreferencesRepository(this._prefs);

  final SharedPreferences? _prefs;
  static const _themeKey = 'postit.theme';

  static Future<PreferencesRepository> open() async {
    try {
      return PreferencesRepository(await SharedPreferences.getInstance());
    } on Object {
      return PreferencesRepository(null);
    }
  }

  ThemeMode get themeMode {
    try {
      return switch (_prefs?.getString(_themeKey)) {
        'light' => ThemeMode.light,
        'dark' => ThemeMode.dark,
        _ => ThemeMode.system,
      };
    } on Object {
      return ThemeMode.system;
    }
  }

  Future<void> setThemeMode(ThemeMode mode) async {
    try {
      await _prefs?.setString(_themeKey, mode.name);
    } on Object {
      // Storage unavailable: the choice lasts until the app closes.
    }
  }
}
