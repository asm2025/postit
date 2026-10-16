// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/page_user_dto.dart';
import '../models/patch_user_request.dart';
import '../models/user_dto.dart';

part 'users_api.g.dart';

@RestApi()
abstract class UsersApi {
  factory UsersApi(Dio dio, {String? baseUrl}) = _UsersApi;

  /// Lists users, filtered by status, role, and a name/email search, ordered by `sort`.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 422 for invalid filters or paging.
  ///
  /// [status] - `pending`, `active`, `disabled`, or `deleting`.
  ///
  /// [role] - `admin` or `member`.
  ///
  /// [sort] - `created_at` (oldest first) or `-created_at` (newest first, the default).
  ///
  /// [search] - Case-insensitive substring of display name or email.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/users')
  Future<PageUserDto> listUsers({
    @Query('status') String? status,
    @Query('role') String? role,
    @Query('sort') String? sort,
    @Query('search') String? search,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// One user by id.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 404 for an unknown id.
  @GET('/api/v1/users/{id}')
  Future<UserDto> getUser({
    @Path('id') required String id,
  });

  /// Starts deleting a user's account.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins or self-deletion, 404 for an unknown id, 409 for the last.
  /// admin or a user already being deleted.
  @DELETE('/api/v1/users/{id}')
  Future<void> deleteUser({
    @Path('id') required String id,
  });

  /// Changes one user's status or role (exactly one field per request).
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins, 404 for an unknown id, 409 for a user being deleted or the.
  /// last admin, and 422 for a malformed or disallowed change.
  @PATCH('/api/v1/users/{id}')
  Future<UserDto> updateUser({
    @Path('id') required String id,
    @Body() required PatchUserRequest body,
  });
}
