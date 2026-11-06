import 'dart:ui' show Locale, PlatformDispatcher;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/state/providers.dart';

/// The languages the interface ships in, named by their language code. A right-to-left language
/// needs no code of its own: Flutter's localizations set the text direction from the locale, and
/// layouts use directional insets and alignments.
enum AppLocale {
  en;

  Locale get flutterLocale => Locale(name);

  /// The language's name in itself, shown whatever the screen language is.
  String get nativeName => switch (this) {
    AppLocale.en => 'English',
  };

  /// The tag the API and the identity provider use.
  String get apiTag => switch (this) {
    AppLocale.en => 'en-US',
  };

  /// Whether there is more than one language to choose from; the language switch shows only then.
  static bool get multilingual => values.length > 1;

  /// Maps a tag to a supported language by its primary subtag; anything else (an unsupported
  /// language included) is English.
  static AppLocale fromTag(String? tag) {
    final language = (tag ?? '').toLowerCase().split(RegExp('[-_]')).first;
    return values.firstWhere((l) => l.name == language, orElse: () => AppLocale.en);
  }
}

/// The device language, mapped to a supported one. Overridden in tests.
final deviceLocaleProvider = Provider<AppLocale>(
  (ref) => AppLocale.fromTag(PlatformDispatcher.instance.locale.languageCode),
);

class LocaleController extends Notifier<AppLocale> {
  /// A language picked without saving it to the account (on the sign-in screen, say); the next sync
  /// saves it instead of reading the account's.
  var _unsaved = false;

  @override
  AppLocale build() {
    final prefs = ref.read(preferencesProvider);
    _unsaved = prefs.localeUnsaved;
    final stored = prefs.localeTag;
    return stored == null ? ref.read(deviceLocaleProvider) : AppLocale.fromTag(stored);
  }

  /// Switches the language now and remembers it on this device; with [persist], also saves it to the
  /// signed-in user's preferences, otherwise the next [syncFromServer] does. When saving fails the
  /// previous language comes back, so the screen never disagrees with the account, and the API error
  /// is rethrown.
  Future<void> set(AppLocale locale, {bool persist = false}) async {
    final previous = state;
    final prefs = ref.read(preferencesProvider);
    state = locale;
    await prefs.setLocaleTag(locale.apiTag);
    if (!persist) return _markUnsaved(true);
    try {
      await ref.read(meRepositoryProvider).putPreferences(locale.apiTag);
      await _markUnsaved(false);
    } on Object {
      state = previous;
      await prefs.setLocaleTag(previous.apiTag);
      rethrow;
    }
  }

  /// After sign-in: a language picked while signed out is saved to the account; otherwise the
  /// account's saved language wins over the device. A failure keeps the current language.
  Future<void> syncFromServer() async {
    try {
      final me = ref.read(meRepositoryProvider);
      if (_unsaved) {
        await me.putPreferences(state.apiTag);
        await _markUnsaved(false);
        return;
      }
      final saved = await me.getPreferences();
      final locale = AppLocale.fromTag(saved.locale.json);
      if (locale == state) return;
      state = locale;
      await ref.read(preferencesProvider).setLocaleTag(locale.apiTag);
    } on Object {
      // Offline or signed out: keep what is on screen (and anything unsaved stays unsaved).
    }
  }

  Future<void> _markUnsaved(bool unsaved) async {
    _unsaved = unsaved;
    await ref.read(preferencesProvider).setLocaleUnsaved(unsaved);
  }
}

final localeProvider = NotifierProvider<LocaleController, AppLocale>(LocaleController.new);
