// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/account_dto.dart';
import '../models/confirm_request.dart';
import '../models/confirm_response.dart';
import '../models/credential_check_response.dart';
import '../models/disconnect_response.dart';
import '../models/import_report_dto.dart';
import '../models/page_account_dto.dart';
import '../models/update_account_request.dart';

part 'accounts_api.g.dart';

@RestApi()
abstract class AccountsApi {
  factory AccountsApi(Dio dio, {String? baseUrl}) = _AccountsApi;

  /// Lists the accounts the caller reaches, newest first.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [platform] - Only accounts on this platform.
  ///
  /// [includeDisconnected] - Include disconnected accounts (default false).
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/accounts')
  Future<PageAccountDto> listAccounts({
    @Query('platform') String? platform,
    @Query('include_disconnected') bool? includeDisconnected,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Connects the accounts the user picked from a checked credential. The `connect_id` works.
  /// once, for the user who checked the credential.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown, expired, used, or foreign `connect_id`; 409 `platform_disabled` or.
  /// `sending_method_exists`.
  ///
  /// [connectId] - The ID returned by the credential check.
  @POST('/api/v1/accounts/connect/{connect_id}/confirm')
  Future<ConfirmResponse> confirm({
    @Path('connect_id') required String connectId,
    @Body() required ConfirmRequest body,
  });

  /// Checks pasted credentials against the platform and lists the accounts they reach. The.
  /// body is a JSON object with the platform's credential fields. When the credential reaches.
  /// accounts, the response carries a one-time `connect_id` to confirm with.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` (every rule the values break, in `violations`) or.
  /// `credentials_invalid` (the field at fault, in `field`), 409 `capability_unsupported` for an.
  /// OAuth platform, 404 for an unknown platform, 429 when the credential-check limit is spent.
  ///
  /// [platform] - Platform ID, such as `telegram`.
  @POST('/api/v1/accounts/credentials/{platform}')
  Future<CredentialCheckResponse> credentials({
    @Path('platform') required String platform,
    @Body() required dynamic body,
  });

  /// Imports credential accounts from a CSV file (`Content-Type: text/csv`). The header row.
  /// lists the platform's credential fields and may add `display_name`. With `commit=false`.
  /// (the default) it only checks every row; with `commit=true` it connects the valid ones. The.
  /// report never contains a field value.
  ///
  /// # Errors.
  ///
  /// 413 past the configured size, 422 for a bad header. A file over the row limit is read up to.
  /// the limit and the report says `truncated`.
  ///
  /// [platform] - Platform ID.
  ///
  /// [commit] - `true` connects the valid rows; `false` (the default) only checks the file.
  ///
  /// The name has been replaced because it contains a keyword. Original name: `import`.
  @POST('/api/v1/accounts/import/{platform}')
  Future<ImportReportDto> postApiV1AccountsImportPlatform({
    @Path('platform') required String platform,
    @Body() required String body,
    @Query('commit') bool? commit,
  });

  /// One account.
  ///
  /// # Errors.
  ///
  /// 404 for a missing account or one outside a delegate's account limit.
  ///
  /// [id] - Account ID.
  @GET('/api/v1/accounts/{id}')
  Future<AccountDto> getAccount({
    @Path('id') required String id,
  });

  /// Disconnects an account: cancels its pending and parked deliveries and keeps the row for.
  /// history.
  ///
  /// # Errors.
  ///
  /// 409 `job_active` while a post is being published to the account.
  ///
  /// [id] - Account ID.
  @DELETE('/api/v1/accounts/{id}')
  Future<DisconnectResponse> disconnectAccount({
    @Path('id') required String id,
  });

  /// Changes the test flag or display name. Last writer wins: accounts carry no version.
  ///
  /// # Errors.
  ///
  /// 422 for a blank or over-long name.
  ///
  /// [id] - Account ID.
  @PATCH('/api/v1/accounts/{id}')
  Future<AccountDto> updateAccount({
    @Path('id') required String id,
    @Body() required UpdateAccountRequest body,
  });

  /// Asks the platform whether the account still works. A rejected credential is not an error:.
  /// the response shows the account as `needs_reconnect`.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` for a disconnected account; 503 when the platform cannot answer.
  ///
  /// [id] - Account ID.
  @POST('/api/v1/accounts/{id}/check')
  Future<AccountDto> check({
    @Path('id') required String id,
  });
}
