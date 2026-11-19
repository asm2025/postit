import 'package:postit/api/exports.dart';
import 'package:postit/data/http/api_call.dart';

/// Admin user management (`/api/v1/users`).
class UsersRepository {
  UsersRepository(this._api);
  final UsersClient _api;

  Future<PageUserDto> list({
    StatusDto? status,
    RoleDto? role,
    String? search,
    required int page,
    required int pageSize,
  }) => apiCall(
    () => _api.listUsers(
      status: status?.toJson(),
      role: role?.toJson(),
      search: (search == null || search.isEmpty) ? null : search,
      page: page,
      pageSize: pageSize,
    ),
  );

  Future<UserDto> get(String id) => apiCall(() => _api.getUser(id: id));

  /// A pending user becomes active.
  Future<UserDto> approve(String id) => _patch(id, const PatchUserRequest(status: StatusDto.active));
  Future<UserDto> enable(String id) => _patch(id, const PatchUserRequest(status: StatusDto.active));
  Future<UserDto> disable(String id) => _patch(id, const PatchUserRequest(status: StatusDto.disabled));
  Future<UserDto> setRole(String id, RoleDto role) => _patch(id, PatchUserRequest(role: role));

  /// Deletes a user, or rejects a pending one.
  Future<void> delete(String id) => apiCall(() => _api.deleteUser(id: id));

  Future<UserDto> _patch(String id, PatchUserRequest body) => apiCall(() => _api.updateUser(id: id, body: body));
}
