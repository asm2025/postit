// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/change_delegation_request.dart';
import '../models/delegation_dto.dart';
import '../models/grant_delegation_request.dart';
import '../models/page_delegation_dto.dart';

part 'delegations_api.g.dart';

@RestApi()
abstract class DelegationsApi {
  factory DelegationsApi(Dio dio, {String? baseUrl}) = _DelegationsApi;

  /// The caller's grants, newest first: live ones, or every one with `include_ended`.
  /// Owner-only: `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
  ///
  /// # Errors.
  ///
  /// 403 `owner_only` through `X-Postit-Act-As`; 422 for invalid paging.
  ///
  /// [includeEnded] - Include ended grants (default false).
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/delegations')
  Future<PageDelegationDto> listDelegations({
    @Query('include_ended') bool? includeEnded,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Lets another user act in the caller's workspace. Without `account_ids` the grant reaches.
  /// every account. Owner-only: `X-Postit-Act-As` is accepted but every delegate is refused.
  /// with `owner_only`.
  ///
  /// # Errors.
  ///
  /// 409 `delegation_exists` when the user already holds a live grant (change it instead); 422.
  /// `validation_failed` with `field` (`delegate_id`, `account_ids`, or `expires_at`).
  @POST('/api/v1/delegations')
  Future<DelegationDto> grantDelegation({
    @Body() required GrantDelegationRequest body,
  });

  /// The grants the caller holds in other users' workspaces that have not ended, newest first.
  /// Does not accept `X-Postit-Act-As` (`owner_only`).
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/delegations/received')
  Future<PageDelegationDto> listReceivedDelegations({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Leaves a grant the caller holds. Leaving an ended grant does nothing. Does not accept.
  /// `X-Postit-Act-As` (`owner_only`).
  ///
  /// # Errors.
  ///
  /// 404 when the caller holds no such delegation.
  ///
  /// [id] - Delegation ID.
  @DELETE('/api/v1/delegations/received/{id}')
  Future<void> leaveDelegation({
    @Path('id') required String id,
  });

  /// Revokes one of the caller's grants. Revoking an ended grant does nothing. Owner-only:.
  /// `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
  ///
  /// # Errors.
  ///
  /// 404 when the caller granted no such delegation.
  ///
  /// [id] - Delegation ID.
  @DELETE('/api/v1/delegations/{id}')
  Future<void> revokeDelegation({
    @Path('id') required String id,
  });

  /// Changes the level, account limit, or expiry of one of the caller's live grants. Absent.
  /// fields stay as they are; `expires_at: null` clears the expiry. Owner-only:.
  /// `X-Postit-Act-As` is accepted but every delegate is refused with `owner_only`.
  ///
  /// # Errors.
  ///
  /// 404 for an unknown, ended, or expired grant; 422 `validation_failed` with `field`.
  /// (`all_accounts` when false, `account_ids` when sent with `all_accounts` or not the caller's.
  /// connected account, `expires_at` when not in the future).
  ///
  /// [id] - Delegation ID.
  @PATCH('/api/v1/delegations/{id}')
  Future<DelegationDto> changeDelegation({
    @Path('id') required String id,
    @Body() required ChangeDelegationRequest body,
  });
}
