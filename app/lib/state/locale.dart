import 'dart:ui' show Locale, PlatformDispatcher;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';

/// The languages the interface ships in.
enum AppLocale {
  en,
  ar;

  Locale get flutterLocale => Locale(name);

  /// The tag the API and the identity provider use.
  String get apiTag => switch (this) {
    AppLocale.en => 'en-US',
    AppLocale.ar => 'ar',
  };

  /// `ar` and `ar-*` map to Arabic; everything else (including an unsupported language) to English.
  static AppLocale fromTag(String? tag) {
    final language = (tag ?? '').toLowerCase().split(RegExp('[-_]')).first;
    return language == 'ar' ? AppLocale.ar : AppLocale.en;
  }
}

/// The device language, mapped to a supported one. Overridden in tests.
final deviceLocaleProvider = Provider<AppLocale>(
  (ref) => AppLocale.fromTag(PlatformDispatcher.instance.locale.languageCode),
);

class LocaleController extends Notifier<AppLocale> {
  @override
  AppLocale build() {
    final stored = ref.read(preferencesProvider).localeTag;
    return stored == null ? ref.read(deviceLocaleProvider) : AppLocale.fromTag(stored);
  }

  /// Switches the language now and remembers it on this device; with [persist], also saves it to the
  /// signed-in user's preferences. When saving fails the previous language comes back, so the screen
  /// never disagrees with the account, and the API error is rethrown.
  Future<void> set(AppLocale locale, {bool persist = false}) async {
    final previous = state;
    state = locale;
    await ref.read(preferencesProvider).setLocaleTag(locale.apiTag);
    if (!persist) return;
    try {
      await ref.read(meRepositoryProvider).putPreferences(locale.apiTag);
    } on Object {
      state = previous;
      await ref.read(preferencesProvider).setLocaleTag(previous.apiTag);
      rethrow;
    }
  }

  /// After sign-in: the saved preference wins over the device. A failure keeps the current language.
  Future<void> syncFromServer() async {
    try {
      final saved = await ref.read(meRepositoryProvider).getPreferences();
      final locale = AppLocale.fromTag(saved.locale.json);
      if (locale == state) return;
      state = locale;
      await ref.read(preferencesProvider).setLocaleTag(locale.apiTag);
    } on Object {
      // Offline or signed out: keep what is on screen.
    }
  }
}

final localeProvider = NotifierProvider<LocaleController, AppLocale>(LocaleController.new);
