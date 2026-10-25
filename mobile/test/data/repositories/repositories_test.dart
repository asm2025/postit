import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/data/repositories/me_repository.dart';
import 'package:postit/data/repositories/users_repository.dart';

import '../../helpers/scripted_adapter.dart';

const _user = <String, Object?>{
  'id': 'u1',
  'display_name': 'Ada',
  'email': 'ada@postit.test',
  'email_verified': true,
  'role': 'member',
  'status': 'pending',
  'created_at': '2026-10-16T10:00:00Z',
  'updated_at': '2026-10-16T10:00:00Z',
};

void main() {
  late ScriptedAdapter adapter;
  Dio dio(Object? Function(RequestOptions o) body, {int status = 200}) {
    adapter = ScriptedAdapter((o) async => ScriptedAdapter.json(status, body(o)));
    return Dio(BaseOptions(baseUrl: 'https://api.test'))..httpClientAdapter = adapter;
  }

  test('list sends filters and paging as query parameters', () async {
    final repo = UsersRepository(
      UsersApi(
        dio(
          (_) => {
            'data': [_user],
            'total': 1,
            'page': 2,
            'page_size': 25,
          },
        ),
      ),
    );
    final page = await repo.list(status: StatusDto.pending, search: 'ada', page: 2, pageSize: 25);
    final q = adapter.requests.single.uri.queryParameters;
    expect(q['status'], 'pending');
    expect(q['search'], 'ada');
    expect(q['page'], '2');
    expect(q['page_size'], '25');
    expect(page.data.single.displayName, 'Ada');
  });

  test('an empty search is left out', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {'data': <Object>[], 'total': 0, 'page': 1, 'page_size': 25})));
    await repo.list(search: '', page: 1, pageSize: 25);
    expect(adapter.requests.single.uri.queryParameters.containsKey('search'), isFalse);
  });

  test('approve patches the status to active', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {..._user, 'status': 'active'})));
    final user = await repo.approve('u1');
    final req = adapter.requests.single;
    expect(req.method, 'PATCH');
    expect(req.uri.path, '/api/v1/users/u1');
    expect((req.data as Map)['status'], 'active');
    expect((req.data as Map)['role'], isNull);
    expect(user.status, StatusDto.active);
  });

  test('setRole patches only the role', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {..._user, 'role': 'admin'})));
    await repo.setRole('u1', RoleDto.admin);
    expect((adapter.requests.single.data as Map)['role'], 'admin');
    expect((adapter.requests.single.data as Map)['status'], isNull);
  });

  test('problems become ApiError', () async {
    final repo = UsersRepository(UsersApi(dio((_) => {'code': 'last_admin', 'status': 409}, status: 409)));
    await expectLater(repo.disable('u1'), throwsA(isA<ApiError>().having((e) => e.code, 'code', 'last_admin')));
  });

  test('delete me sends the typed display name', () async {
    final repo = MeRepository(MeApi(dio((_) => null, status: 202)));
    await repo.delete('Ada');
    final req = adapter.requests.single;
    expect(req.method, 'DELETE');
    expect(req.uri.path, '/api/v1/me');
    expect(req.data, isA<Map<String, dynamic>>().having((m) => m['display_name'], 'display_name', 'Ada'));
  });
}
