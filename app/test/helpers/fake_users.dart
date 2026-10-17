import 'package:postit/api/export.dart';
import 'package:postit/data/http/api_error.dart';
import 'package:postit/data/repositories/users_repository.dart';

/// The signed-in user as `GET /me` returns it: Ada, member unless [role] says otherwise.
MeDto fakeMe(StatusDto status, {RoleDto role = RoleDto.member}) => MeDto.fromJson({
  'id': 'u1',
  'display_name': 'Ada',
  'email_verified': true,
  'role': role.json,
  'status': status.json,
  'created_at': '2026-10-16T10:00:00Z',
  'updated_at': '2026-10-16T10:00:00Z',
});

UserDto fakeUser(String id, {StatusDto status = StatusDto.active, RoleDto role = RoleDto.member, String? name}) =>
    UserDto.fromJson({
      'id': id,
      'display_name': name ?? 'User $id',
      'email': '$id@postit.test',
      'email_verified': true,
      'role': role.json,
      'status': status.json,
      'created_at': '2026-10-16T10:00:00Z',
      'updated_at': '2026-10-16T10:00:00Z',
    });

/// In-memory admin user list; records the calls the screens make.
class FakeUsersRepository implements UsersRepository {
  FakeUsersRepository(this.users);

  final List<UserDto> users;
  final pagesRequested = <int>[];
  final calls = <String>[];
  Object? failWith;

  /// Pages whose list request fails, for the load-more error path.
  final failPages = <int>{};

  @override
  Future<PageUserDto> list({
    StatusDto? status,
    RoleDto? role,
    String? search,
    required int page,
    required int pageSize,
  }) async {
    pagesRequested.add(page);
    if (failPages.contains(page)) throw const ApiError(0, 'network');
    final matching = users
        .where((u) => status == null || u.status == status)
        .where((u) => role == null || u.role == role)
        .where((u) => search == null || search.isEmpty || u.displayName.toLowerCase().contains(search.toLowerCase()))
        .toList();
    final start = (page - 1) * pageSize;
    final slice = matching.skip(start).take(pageSize).toList();
    return PageUserDto.fromJson({
      'data': [for (final u in slice) u.toJson()],
      'total': matching.length,
      'page': page,
      'page_size': pageSize,
    });
  }

  @override
  Future<UserDto> get(String id) async => users.firstWhere((u) => u.id == id);

  Future<UserDto> _record(String call, String id, UserDto Function(UserDto) change) async {
    calls.add('$call $id');
    final error = failWith;
    if (error != null) throw error;
    final i = users.indexWhere((u) => u.id == id);
    users[i] = change(users[i]);
    return users[i];
  }

  UserDto _with(UserDto u, {StatusDto? status, RoleDto? role}) =>
      UserDto.fromJson({...u.toJson(), if (status != null) 'status': status.json, if (role != null) 'role': role.json});

  @override
  Future<UserDto> approve(String id) => _record('approve', id, (u) => _with(u, status: StatusDto.active));
  @override
  Future<UserDto> enable(String id) => _record('enable', id, (u) => _with(u, status: StatusDto.active));
  @override
  Future<UserDto> disable(String id) => _record('disable', id, (u) => _with(u, status: StatusDto.disabled));
  @override
  Future<UserDto> setRole(String id, RoleDto role) => _record('role:${role.json}', id, (u) => _with(u, role: role));

  @override
  Future<void> delete(String id) async {
    await _record('delete', id, (u) => u);
    users.removeWhere((u) => u.id == id);
  }
}
