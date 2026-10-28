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
import '../models/o_auth_start_response.dart';
import '../models/page_account_dto.dart';
import '../models/pending_connection_dto.dart';
import '../models/start_o_auth_request.dart';
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

  /// Connects the accounts the user picked from a checked credential or an OAuth sign-in. The.
  /// `connect_id` works once, for the user who started the connection.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown, expired, used, or foreign `connect_id`; 409 `platform_disabled` or.
  /// `sending_method_exists`.
  ///
  /// [connectId] - The ID returned by the credential check or the OAuth callback's redirect.
  @POST('/api/v1/accounts/connect/{connect_id}/confirm')
  Future<ConfirmResponse> confirm({
    @Path('connect_id') required String connectId,
    @Body() required ConfirmRequest body,
  });

  /// Reads a pending connection, from an OAuth sign-in or pasted credentials, without.
  /// consuming it: the accounts to offer before confirming. Only the user who started it may.
  /// read it; anyone else's read ends it and answers 404.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown, expired, used, or foreign `connect_id`.
  ///
  /// [id] - The `connect_id` from the OAuth callback's redirect or the credential check.
  @GET('/api/v1/accounts/connect/{id}')
  Future<PendingConnectionDto> getPendingConnection({
    @Path('id') required String id,
  });

  /// Starts connecting accounts of an OAuth platform. Send the user's browser to.
  /// `authorize_url`; the platform redirects it to the callback, which ends on `return_to` with.
  /// `result=ok&connect_id=…` (read the accounts with `GET /accounts/connect/{id}`, then confirm).
  /// or `result=error&code=…`. Owner only.
  ///
  /// # Errors.
  ///
  /// 422 `return_to_invalid` unless `return_to` is exactly an allowed return URL, 409.
  /// `capability_unsupported` for a credentials platform, 404 for an unknown platform, 503 when.
  /// the platform is not configured here.
  ///
  /// [id] - Platform ID, such as `mock_oauth`.
  @POST('/api/v1/accounts/connect/{id}')
  Future<OAuthStartResponse> startOauthConnection({
    @Path('id') required String id,
    @Body() required StartOAuthRequest body,
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

  /// An event the platform sends, such as a deauthorization. Not called by clients. The agent.
  /// verifies it (a signature over the raw body, typically) and its reply is returned as is.
  /// A verified deauthorization revokes every grant of that remote user on the platform.
  ///
  /// # Errors.
  ///
  /// 401 with no detail when the agent cannot verify the event, 404 for an unknown or unavailable.
  /// platform or an event kind it does not handle, 413 past 64 KiB, 422 for a verified event the.
  /// agent cannot read.
  ///
  /// [id] - Platform ID.
  ///
  /// [kind] - The agent's name for the event, such as `deauthorize`.
  @POST('/api/v1/platforms/{id}/events/{kind}')
  Future<void> platformEvent({
    @Path('id') required String id,
    @Path('kind') required String kind,
    @Body() required String body,
  });

  /// The provider's redirect after the user signed in (or refused). Not called by clients: the.
  /// browser arrives here from the platform, without a bearer token, and `X-Postit-Act-As` is.
  /// ignored; the connection belongs to the user who started it.
  ///
  /// Always answers 303. The `Location` is the `return_to` the connection was started with,.
  /// carrying `result=ok&connect_id=…` on success (read the accounts with.
  /// `GET /accounts/connect/{id}`, then confirm) or `result=error&code=…`, where `code` is one of.
  /// `access_denied` (the user refused), `provider_error` (the provider reported another error or.
  /// sent no code), `exchange_failed` (the code could not be exchanged for tokens),.
  /// `scope_missing` (the user did not grant every permission the platform needs),.
  /// `discovery_failed` (the accounts could not be read), `no_accounts` (the sign-in reaches no.
  /// account), or `platform_unavailable` (the platform was switched off or is misconfigured). An.
  /// unknown, used, expired, or foreign `state` (`state_invalid`) goes to the web app's.
  /// `/oauth-done`, since no trustworthy `return_to` exists. Accounts another owner already.
  /// holds are skipped at confirm and counted in its `skipped_elsewhere`.
  ///
  /// [id] - Platform ID.
  ///
  /// [code] - The authorization code.
  ///
  /// [state] - The `state` postit sent with the authorization request.
  ///
  /// [error] - The provider's OAuth error, such as `access_denied`.
  ///
  /// [errorDescription] - The provider's description of the error. Ignored.
  @GET('/api/v1/platforms/{id}/oauth/callback')
  Future<void> oauthCallback({
    @Path('id') required String id,
    @Query('code') String? code,
    @Query('state') String? state,
    @Query('error') String? error,
    @Query('error_description') String? errorDescription,
  });
}
