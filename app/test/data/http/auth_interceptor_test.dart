import 'dart:async';

import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/auth/auth_client.dart';
import 'package:postit/data/auth/not_signed_in.dart';
import 'package:postit/data/auth/session_manager.dart';
import 'package:postit/data/http/auth_interceptor.dart';
import 'package:postit/data/http/dio_factory.dart';

import '../../helpers/fakes.dart';
import '../../helpers/scripted_adapter.dart';

void main() {
  late FakeAuthClient client;
  late SessionManager session;

  Dio dioWith(ScriptedAdapter adapter) {
    final dio = createDio(Uri.parse('https://api.test'))..httpClientAdapter = adapter;
    dio.interceptors.add(AuthInterceptor(session, dio));
    return dio;
  }

  setUp(() async {
    client = FakeAuthClient();
    session = SessionManager(client: client, store: MemoryStore(), revoke: (_) async {});
    await session.signIn();
  });

  test('attaches the bearer token', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {}));
    await dioWith(adapter).get<Object>('/api/v1/me');
    expect(adapter.requests.single.headers['Authorization'], 'Bearer access-signin');
  });

  test('a 401 refreshes once and retries with the new token', () async {
    final adapter = ScriptedAdapter(
      (o) async => o.headers['Authorization'] == 'Bearer access-signin'
          ? ScriptedAdapter.json(401, {'code': 'unauthenticated'})
          : ScriptedAdapter.json(200, {'ok': true}),
    );
    final res = await dioWith(adapter).get<Map<String, dynamic>>('/api/v1/me');
    expect(res.data, {'ok': true});
    expect(client.refreshCalls, 1);
    expect(adapter.requests.last.headers['Authorization'], 'Bearer access-refreshed');
  });

  test('concurrent 401s share one refresh', () async {
    final gate = Completer<TokenSet>();
    client.onRefresh = (_) => gate.future;
    final adapter = ScriptedAdapter(
      (o) async => o.headers['Authorization'] == 'Bearer access-signin'
          ? ScriptedAdapter.json(401, {'code': 'unauthenticated'})
          : ScriptedAdapter.json(200, {}),
    );
    final dio = dioWith(adapter);
    final calls = Future.wait([dio.get<Object>('/a'), dio.get<Object>('/b'), dio.get<Object>('/c')]);
    await Future<void>.delayed(Duration.zero);
    gate.complete(tokens('new'));
    final results = await calls;
    expect(results.map((r) => r.statusCode), [200, 200, 200]);
    expect(client.refreshCalls, 1);
  });

  test('a 401 that arrives after another refresh retries with the current token', () async {
    // The request left with the old token; a sibling refreshed before its 401 came back.
    final adapter = ScriptedAdapter((o) async {
      if (o.headers['Authorization'] != 'Bearer access-signin') return ScriptedAdapter.json(200, {});
      await session.refresh();
      return ScriptedAdapter.json(401, {'code': 'unauthenticated'});
    });
    final res = await dioWith(adapter).get<Object>('/api/v1/me');
    expect(res.statusCode, 200);
    expect(client.refreshCalls, 1);
    expect(adapter.requests.last.headers['Authorization'], 'Bearer access-refreshed');
  });

  test('a second 401 expires the session', () async {
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(isA<DioException>().having((e) => e.response?.statusCode, 'status', 401)),
    );
    expect(session.state.value, const SessionState.signedOut(sessionExpiredNotice));
  });

  test('an unreachable provider fails the call as a network error and keeps the session', () async {
    client.onRefresh = (_) async => throw const AuthUnavailable();
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(
        isA<DioException>()
            .having((e) => e.type, 'type', DioExceptionType.connectionError)
            .having((e) => e.error, 'error', isA<AuthUnavailable>()),
      ),
    );
    expect(session.state.value, const SessionState.signedIn());
  });

  test('a failed refresh expires the session and fails the call', () async {
    client.onRefresh = (_) async => throw const AuthFailed('invalid_grant');
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(401, {'code': 'unauthenticated'}));
    await expectLater(dioWith(adapter).get<Object>('/api/v1/me'), throwsA(isA<DioException>()));
    expect(session.state.value.status, SessionStatus.signedOut);
  });

  test('without a session the request is never sent', () async {
    await session.signOut();
    final adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(200, {}));
    await expectLater(
      dioWith(adapter).get<Object>('/api/v1/me'),
      throwsA(isA<DioException>().having((e) => e.error, 'error', isA<NotSignedIn>())),
    );
    expect(adapter.requests, isEmpty);
  });
}
