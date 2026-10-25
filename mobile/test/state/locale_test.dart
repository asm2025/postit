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

import '../helpers/scripted_adapter.dart';

class _FakeMe implements MeRepository {
  _FakeMe([this.stored = 'en-US']);
  String stored;
  final puts = <String>[];
  Object? failWith;

  @override
  Future<PreferencesDto> getPreferences() async => PreferencesDto(locale: LocaleDto.fromJson(stored));

  @override
  Future<PreferencesDto> putPreferences(String locale) async {
    puts.add(locale);
    if (failWith case final error?) throw error;
    stored = locale;
    return PreferencesDto(locale: LocaleDto.fromJson(locale));
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
  test('fromTag maps ar-SA to ar and fr to en', () {
    expect(AppLocale.fromTag('ar-SA'), AppLocale.ar);
    expect(AppLocale.fromTag('ar'), AppLocale.ar);
    expect(AppLocale.fromTag('en-GB'), AppLocale.en);
    expect(AppLocale.fromTag('fr'), AppLocale.en);
    expect(AppLocale.fromTag(null), AppLocale.en);
    expect(AppLocale.ar.apiTag, 'ar');
    expect(AppLocale.en.apiTag, 'en-US');
    expect(AppLocale.ar.flutterLocale, const Locale('ar'));
  });

  test('starts from the device locale', () {
    expect(_container(device: AppLocale.ar).read(localeProvider), AppLocale.ar);
  });

  test('a stored choice wins over the device and survives a restart', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final c = _container(prefs: prefs);
    await c.read(localeProvider.notifier).set(AppLocale.ar);
    expect(c.read(localeProvider), AppLocale.ar);
    final again = _container(prefs: await PreferencesRepository.open());
    expect(again.read(localeProvider), AppLocale.ar);
  });

  test('set persists to the API when asked', () async {
    final me = _FakeMe();
    final c = _container(me: me);
    await c.read(localeProvider.notifier).set(AppLocale.ar);
    expect(me.puts, isEmpty);
    await c.read(localeProvider.notifier).set(AppLocale.ar, persist: true);
    expect(me.puts, ['ar']);
  });

  test('a failed save restores the previous language and rethrows', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await PreferencesRepository.open();
    final me = _FakeMe()..failWith = const ApiError(500, 'internal');
    final c = _container(me: me, prefs: prefs);
    await expectLater(c.read(localeProvider.notifier).set(AppLocale.ar, persist: true), throwsA(isA<ApiError>()));
    expect(c.read(localeProvider), AppLocale.en);
    expect(prefs.localeTag, 'en-US');
  });

  test('syncFromServer applies the stored preference', () async {
    final c = _container(me: _FakeMe('ar'));
    await c.read(localeProvider.notifier).syncFromServer();
    expect(c.read(localeProvider), AppLocale.ar);
  });

  test('a language picked while signed out is saved to the account at the next sync', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US');
    final c = _container(me: me, prefs: await PreferencesRepository.open());
    await c.read(localeProvider.notifier).set(AppLocale.ar);
    await c.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['ar']);
    expect(c.read(localeProvider), AppLocale.ar);
    // Saved now: the sync after that reads the account again.
    me.stored = 'en-US';
    await c.read(localeProvider.notifier).syncFromServer();
    expect(me.puts, ['ar']);
    expect(c.read(localeProvider), AppLocale.en);
  });

  test('an unsaved choice survives a restart and a failed save', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US')..failWith = const ApiError(503, 'unavailable');
    await _container(me: me, prefs: await PreferencesRepository.open()).read(localeProvider.notifier).set(AppLocale.ar);
    final again = _container(me: me, prefs: await PreferencesRepository.open());
    await again.read(localeProvider.notifier).syncFromServer();
    expect(again.read(localeProvider), AppLocale.ar);
    me.failWith = null;
    await again.read(localeProvider.notifier).syncFromServer();
    expect(me.stored, 'ar');
  });

  test('a choice saved to the account leaves nothing to sync', () async {
    SharedPreferences.setMockInitialValues({});
    final me = _FakeMe('en-US');
    final c = _container(me: me, prefs: await PreferencesRepository.open());
    await c.read(localeProvider.notifier).set(AppLocale.ar);
    await c.read(localeProvider.notifier).set(AppLocale.ar, persist: true);
    me.stored = 'en-US';
    await c.read(localeProvider.notifier).syncFromServer();
    expect(c.read(localeProvider), AppLocale.en);
  });

  test('dio sends the on-screen language', () async {
    final c = _container();
    final adapter = ScriptedAdapter((_) async => ScriptedAdapter.json(200, {}));
    final dio = createDio(Uri.parse('https://api.test'), acceptLanguage: () => c.read(localeProvider).apiTag)
      ..httpClientAdapter = adapter;
    await dio.get<Object?>('/x');
    expect(adapter.requests.last.headers['Accept-Language'], 'en-US');
    await c.read(localeProvider.notifier).set(AppLocale.ar);
    await dio.get<Object?>('/x');
    expect(adapter.requests.last.headers['Accept-Language'], 'ar');
  });
}
