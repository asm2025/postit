import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/app.dart';

import 'fakes.dart';

final testConfig = AppConfig.parse(apiBaseUrl: 'https://api.test', env: 'development');

Future<SessionManager> signedInManager([FakeAuthClient? client]) async {
  final m = SessionManager(client: client ?? FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  await m.signIn();
  return m;
}

Future<SessionManager> signedOutManager([FakeAuthClient? client]) async {
  final m = SessionManager(client: client ?? FakeAuthClient(), store: MemoryStore(), revoke: (_) async {});
  await m.restore();
  return m;
}

Future<void> pumpApp(WidgetTester tester, {required List<Override> overrides, Size size = const Size(400, 800)}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      retry: (retryCount, error) => null,
      overrides: [
        appConfigProvider.overrideWithValue(testConfig),
        preferencesProvider.overrideWithValue(PreferencesRepository(null)),
        clockProvider.overrideWithValue(() => DateTime(2026, 10, 16, 9)),
        ...overrides,
      ],
      child: const PostitApp(),
    ),
  );
  await tester.pumpAndSettle();
}
