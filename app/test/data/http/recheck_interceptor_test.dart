import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/data/http/recheck_interceptor.dart';

import '../../helpers/scripted_adapter.dart';

void main() {
  Future<int> rechecks(String path, Map<String, Object?> body, int status) async {
    var count = 0;
    final dio = Dio(BaseOptions(baseUrl: 'https://api.test'))
      ..httpClientAdapter = ScriptedAdapter((o) async => ScriptedAdapter.json(status, body))
      ..interceptors.add(RecheckInterceptor(() => count++));
    try {
      await dio.get<Object>(path);
    } on DioException {
      // expected
    }
    return count;
  }

  test('forbidden, pending or disabled on another route rechecks /me', () async {
    expect(await rechecks('/api/v1/users', {'code': 'forbidden'}, 403), 1);
    expect(await rechecks('/api/v1/users', {'code': 'account_pending'}, 403), 1);
    expect(await rechecks('/api/v1/users', {'code': 'account_disabled'}, 403), 1);
  });

  test('errors from /me itself and other codes do not', () async {
    expect(await rechecks('/api/v1/me', {'code': 'account_disabled'}, 403), 0);
    expect(await rechecks('/api/v1/users', {'code': 'last_admin'}, 409), 0);
  });
}
