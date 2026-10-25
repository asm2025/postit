import 'dart:io' show Platform, SecurityContext;

import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/auth/app_auth_client.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/auth/session_store.dart';
import 'package:postit/data/auth/token_revoker.dart';
import 'package:postit/data/config/app_config.dart';
import 'package:postit/data/http/auth_interceptor.dart';
import 'package:postit/data/http/dio_factory.dart';
import 'package:postit/data/http/recheck_interceptor.dart';
import 'package:postit/data/repositories/auth_config_repository.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/data/repositories/preferences_repository.dart';
import 'package:postit/data/repositories/users_repository.dart';
import 'package:postit/state/locale.dart';

enum MobilePlatform { android, ios }

/// Overridden in `main()` with the dart-define values.
final appConfigProvider = Provider<AppConfig>((ref) => throw StateError('appConfigProvider is set in main()'));

/// Overridden in `main()` once shared preferences are open.
final preferencesProvider = Provider<PreferencesRepository>(
  (ref) => throw StateError('preferencesProvider is set in main()'),
);

/// Extra TLS trust for dio: the dev CA in debug development builds (`devTrust`), else null.
final trustProvider = Provider<SecurityContext?>((ref) => null);

final platformProvider = Provider<MobilePlatform>(
  (ref) => Platform.isIOS ? MobilePlatform.ios : MobilePlatform.android,
);
final authClientFactoryProvider = Provider<AuthClient Function(OidcClientConfig)>(
  (ref) =>
      (config) => AppAuthClient(config, uiLocale: () => ref.read(localeProvider).apiTag),
);
final sessionStoreProvider = Provider<SessionStore>((ref) => SecureSessionStore());
final clockProvider = Provider<DateTime Function()>((ref) => DateTime.now);

/// This platform's client ID; the web `client_id` is never used by the app.
String? clientIdFor(AuthConfigDto config, MobilePlatform platform) => switch (platform) {
  MobilePlatform.android => config.androidClientId,
  MobilePlatform.ios => config.iosClientId,
};

final _publicDioProvider = Provider<Dio>(
  (ref) => createDio(
    ref.watch(appConfigProvider).apiBaseUrl,
    trust: ref.watch(trustProvider),
    acceptLanguage: () => ref.read(localeProvider).apiTag,
  ),
);

/// The session for this platform's client, restored from storage; null when the deployment
/// has no client for this platform.
final sessionManagerProvider = FutureProvider<SessionManager?>((ref) async {
  final config = ref.watch(appConfigProvider);
  final publicDio = ref.watch(_publicDioProvider);
  final auth = await AuthConfigRepository(AuthApi(publicDio)).load();
  final clientId = clientIdFor(auth, ref.watch(platformProvider));
  if (clientId == null) return null;
  final oidc = OidcClientConfig(
    issuer: auth.issuer,
    clientId: clientId,
    scopes: auth.scopes,
    redirectUrl: config.redirectUrl,
    postLogoutRedirectUrl: config.postLogoutRedirectUrl,
  );
  final manager = SessionManager(
    client: ref.watch(authClientFactoryProvider)(oidc),
    store: ref.watch(sessionStoreProvider),
    revoke: TokenRevoker(publicDio, oidc).revoke,
    now: ref.watch(clockProvider),
  );
  ref.onDispose(manager.dispose);
  await manager.restore();
  return manager;
});

final sessionStateProvider = Provider<SessionState>((ref) {
  final manager = ref.watch(sessionManagerProvider).value;
  if (manager == null) return const SessionState.loading();
  void changed() => ref.invalidateSelf();
  manager.state.addListener(changed);
  ref.onDispose(() => manager.state.removeListener(changed));
  return manager.state.value;
});

/// Only read while signed in, when the session exists.
final Provider<Dio> apiDioProvider = Provider<Dio>((ref) {
  final manager = ref.watch(sessionManagerProvider).requireValue;
  if (manager == null) throw const NotSignedIn();
  final dio = createDio(
    ref.watch(appConfigProvider).apiBaseUrl,
    trust: ref.watch(trustProvider),
    acceptLanguage: () => ref.read(localeProvider).apiTag,
  );
  dio.interceptors
    ..add(AuthInterceptor(manager, dio))
    ..add(RecheckInterceptor(() => ref.invalidate(meProvider)));
  return dio;
});

final Provider<MeRepository> meRepositoryProvider = Provider<MeRepository>(
  (ref) => MeRepository(MeApi(ref.watch(apiDioProvider))),
);
final Provider<UsersRepository> usersRepositoryProvider = Provider<UsersRepository>(
  (ref) => UsersRepository(UsersApi(ref.watch(apiDioProvider))),
);

final FutureProvider<MeDto> meProvider = FutureProvider<MeDto>((ref) async {
  final status = ref.watch(sessionStateProvider.select((s) => s.status));
  if (status != SessionStatus.signedIn) throw const NotSignedIn();
  return ref.watch(meRepositoryProvider).get();
});

final pendingCountProvider = FutureProvider.autoDispose<int>((ref) async {
  final page = await ref.watch(usersRepositoryProvider).list(status: StatusDto.pending, page: 1, pageSize: 1);
  return page.total;
});

final activeAdminCountProvider = FutureProvider.autoDispose<int>((ref) async {
  final page = await ref
      .watch(usersRepositoryProvider)
      .list(status: StatusDto.active, role: RoleDto.admin, page: 1, pageSize: 1);
  return page.total;
});

final userProvider = FutureProvider.autoDispose.family<UserDto, String>(
  (ref, id) => ref.watch(usersRepositoryProvider).get(id),
);
