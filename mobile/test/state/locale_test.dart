import 'dart:ui' show Locale;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/data/http/dio_factory.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/locale.dart';
import 'package:postit/state/providers.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../helpers/fake_users.dart';
import '../helpers/scripted_adapter.dart';

class _FakeMe implements MeRepository {
  _FakeMe([this.stored = 'en-US']);
  String stored;
  final puts = <String>[];
  Object? failWith;

  @override
  Future<PreferencesDto> getPreferences() async => fakePreferences(stored);

  @override
  Future<PreferencesDto> putPreferences(String locale) async {
    puts.add(locale);
    if (failWith case final error?) throw error;
    stored = locale;
    return fakePreferences(locale);
  }

  @override
  Future<MeDto> get() => throw UnimplementedError();

  @override
  Future<void> delete(String displayName) => throw UnimplementedError();
}

ProviderContainer _container({_FakeMe? me, PreferencesRepository? prefs, AppLocale device = AppLocale.en}) {
  final c = ProviderContainer(
    retry: (retryCount, error) => null,
    overrides: [
      preferencesProvider.overrideWithValue(prefs ?? PreferencesRepository(null)),
      deviceLocaleProvider.overrideWithValue(device),
      if (me != null) meRepositoryProvider.overrideWithValue(me),
    ],
  );
  addTearDown(c.dispose);
  return c;
}

void main() {
  test('fromTag maps a tag by its primary subtag, anything else to en', () {
    expect(AppLocale.fromTag('en-GB'), AppLocale.en);
    expect(AppLocale.fromTag('EN'), AppLocale.en);
    expect(AppLocale.fromTag('fr'), AppLocale.en);
    expect(AppLocale.fromTag(null), AppLocale.en);
    expect(AppLocale.en.apiTag, 'en-US');
    expect(AppLocale.en.flutterLocale, const Locale('en'));
    expect(AppLocale.multilingual, AppLocale.values.length > 1);
  });

  test('starts from the device locale', () {
    expect(_container().read(localeProvider), AppLocale.en);
  });

  test('a stored choice survives a restart', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final c = _container(prefs: prefs);
    await c.read(localeProvider.notifier).set(AppLocale.en);
    expect(prefs.localeTag, 'en-US');
    final again = _container(prefs: await PreferencesRepository.open());
    expect(again.read(localeProvider), AppLocale.en);
  });

  test('set persists to the API when asked', () async {
    final me = _FakeMe();
    final c = _container(me: me);
    await c.read(localeProvider.notifier).set(AppLocale.en);
    expect(me.puts, isEmpty);
    await c.read(localeProvider.notifier).set(AppLocale.en, persist: true);
    expect(me.puts, ['en-US']);
  });

  test('a failed save restores the previous language and rethrows', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final me = _FakeMe()..failWith = const ApiError(500, 'internal');
    final c = _container(me: me, prefs: prefs);
    await expectLater(c.read(localeProvider.notifier).set(AppLocale.en, persist: true), throwsA(isA<ApiError>()));
    expect(c.read(localeProvider), AppLocale.en);
    expect(prefs.localeTag, 'en-US');
  });

  test('syncFromServer applies the stored preference', () async {
    final c = _container(me: _FakeMe('en-US'));
    await c.read(localeProvider.notifier).syncFromServer();
    expect(c.read(localeProvider), AppLocale.en);
  });

  test('a language picked while signed out is saved to the account at the next sync', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US');
    final c = _container(me: me, prefs: await PreferencesRepository.open());
    await c.read(localeProvider.notifier).set(AppLocale.en);
    await c.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['en-US']);
    // Saved now: the sync after that reads the account again.
    await c.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['en-US']);
  });

  test('an unsaved choice survives a restart and a failed save', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US')..failWith = const ApiError(503, 'unavailable');
    await _container(me: me, prefs: await PreferencesRepository.open()).read(localeProvider.notifier).set(AppLocale.en);
    final again = _container(me: me, prefs: await PreferencesRepository.open());
    await again.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['en-US']);
    me.failWith = null;
    await again.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['en-US', 'en-US']);
  });

  test('a choice saved to the account leaves nothing to sync', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US');
    final c = _container(me: me, prefs: await PreferencesRepository.open());
    await c.read(localeProvider.notifier).set(AppLocale.en);
    await c.read(localeProvider.notifier).set(AppLocale.en, persist: true);
    await c.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['en-US']);
  });

  test('dio sends the on-screen language', () async {
    final c = _container();
    final adapter = ScriptedAdapter((_) async => ScriptedAdapter.json(200, {}));
    final dio = createDio(Uri.parse('https://api.test'), acceptLanguage: () => c.read(localeProvider).apiTag)
      ..httpClientAdapter = adapter;
    await dio.get<Object?>('/x');
    expect(adapter.requests.last.headers['Accept-Language'], 'en-US');
  });
}
