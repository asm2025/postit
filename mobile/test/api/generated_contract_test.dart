import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:postit/api/export.dart';

void main() {
  test('the generated client has the names the repositories use', () {
    final dio = Dio();
    final auth = AuthApi(dio);
    final me = MeApi(dio);
    final users = UsersApi(dio);
    final Future<AuthConfigDto> Function() authConfig = auth.authConfig;
    final Future<MeDto> Function() getMe = me.getMe;
    final Future<void> Function({required DeleteMeRequest body}) deleteMe = me.deleteMe;
    final Future<UserDto> Function({required String id}) getUser = users.getUser;
    final Future<void> Function({required String id}) deleteUser = users.deleteUser;
    expect([authConfig, getMe, deleteMe, getUser, deleteUser], hasLength(5));
    expect(users.listUsers, isNotNull);
    expect(users.updateUser, isNotNull);
  });

  test('models decode the server JSON', () {
    final user = <String, Object?>{
      'id': '0192f0c4-0000-7000-8000-000000000001',
      'display_name': 'Ada',
      'email': 'ada@postit.test',
      'email_verified': true,
      'role': 'admin',
      'status': 'active',
      'created_at': '2026-10-16T10:00:00Z',
      'updated_at': '2026-10-16T10:00:00Z',
      'approved_at': null,
      'approved_by': null,
      'last_seen_at': null,
    };
    final me = MeDto.fromJson({...user, 'account_url': 'https://idp.test/account'});
    expect(me.displayName, 'Ada');
    expect(me.role, RoleDto.admin);
    expect(me.status, StatusDto.active);
    expect(me.accountUrl, 'https://idp.test/account');

    final page = PageUserDto.fromJson({
      'data': [user],
      'total': 1,
      'page': 1,
      'page_size': 25,
    });
    expect(page.data.single.emailVerified, isTrue);
    expect(page.pageSize, 25);

    final config = AuthConfigDto.fromJson({
      'issuer': 'https://idp.test',
      'client_id': 'web',
      'android_client_id': 'android',
      'ios_client_id': null,
      'scopes': ['openid'],
    });
    expect(config.androidClientId, 'android');
    expect(config.iosClientId, isNull);

    expect(const PatchUserRequest(status: StatusDto.active).toJson()['status'], 'active');
    expect(const DeleteMeRequest(displayName: 'Ada').toJson(), {'display_name': 'Ada'});
  });
}
