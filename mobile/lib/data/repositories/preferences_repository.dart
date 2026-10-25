import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Device-local preferences. Every read and write tolerates storage being unavailable.
class PreferencesRepository {
  PreferencesRepository(this._prefs);

  final SharedPreferences? _prefs;
  static const _themeKey = 'postit.theme';
  static const _localeKey = 'postit.locale';
  static const _localeUnsavedKey = 'postit.localeUnsaved';

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

  /// The language tag the user picked on this device (`en-US` or `ar`), if any.
  String? get localeTag {
    try {
      return _prefs?.getString(_localeKey);
    } on Object {
      return null;
    }
  }

  Future<void> setLocaleTag(String tag) async {
    try {
      await _prefs?.setString(_localeKey, tag);
    } on Object {
      // Storage unavailable: the choice lasts until the app closes.
    }
  }

  /// Whether [localeTag] was picked while signed out and still has to be saved to the account.
  bool get localeUnsaved {
    try {
      return _prefs?.getBool(_localeUnsavedKey) ?? false;
    } on Object {
      return false;
    }
  }

  Future<void> setLocaleUnsaved(bool unsaved) async {
    try {
      await _prefs?.setBool(_localeUnsavedKey, unsaved);
    } on Object {
      // Storage unavailable: the flag lasts until the app closes.
    }
  }
}
