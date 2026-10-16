import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/dev_trust.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/state/providers.dart';
import 'package:postit/ui/app.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final config = AppConfig.fromEnvironment();
  final preferences = await PreferencesRepository.open();
  final trust = await devTrust(config);
  runApp(
    ProviderScope(
      // Failures surface at once; the error screens own retrying.
      retry: (retryCount, error) => null,
      overrides: [
        appConfigProvider.overrideWithValue(config),
        preferencesProvider.overrideWithValue(preferences),
        trustProvider.overrideWithValue(trust),
      ],
      child: const PostitApp(),
    ),
  );
}
