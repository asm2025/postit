// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/delete_me_request.dart';
import '../models/me_dto.dart';
import '../models/preferences_dto.dart';
import '../models/update_preferences.dart';

part 'me_api.g.dart';

@RestApi()
abstract class MeApi {
  factory MeApi(Dio dio, {String? baseUrl}) = _MeApi;

  /// The caller's own profile. Reachable while the account is still pending approval.
  ///
  /// # Errors.
  ///
  /// Returns 401 without a valid token, 403 for a disabled account.
  @GET('/api/v1/me')
  Future<MeDto> getMe();

  /// Starts deleting the caller's own account.
  ///
  /// # Errors.
  ///
  /// Returns 403 while pending (or for a disabled or already-deleting account, which `Auth`.
  /// rejects first), 422 when the display name does not match, 409 for the last active admin.
  @DELETE('/api/v1/me')
  Future<void> deleteMe({
    @Body() required DeleteMeRequest body,
  });

  /// The caller's preferences. Reachable while the account is still pending approval.
  ///
  /// # Errors.
  ///
  /// Returns 401 without a valid token, 403 for a disabled account.
  @GET('/api/v1/me/preferences')
  Future<PreferencesDto> getPreferences();

  /// Changes the caller's preferences. Reachable while the account is still pending approval.
  ///
  /// # Errors.
  ///
  /// Returns 401 without a valid token, 403 for a disabled account, 422 for an unsupported.
  /// locale.
  @PUT('/api/v1/me/preferences')
  Future<PreferencesDto> putPreferences({
    @Body() required UpdatePreferences body,
  });
}
