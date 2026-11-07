// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/brand_profile_dto.dart';
import '../models/campaign_dto.dart';
import '../models/create_campaign_request.dart';
import '../models/page_campaign_dto.dart';
import '../models/page_target_group_dto.dart';
import '../models/patch_campaign_request.dart';
import '../models/put_brand_profile_request.dart';
import '../models/put_writing_style_request.dart';
import '../models/target_group_dto.dart';
import '../models/target_group_request.dart';
import '../models/writing_style_dto.dart';

part 'workspace_api.g.dart';

@RestApi()
abstract class WorkspaceApi {
  factory WorkspaceApi(Dio dio, {String? baseUrl}) = _WorkspaceApi;

  /// An account's writing style. An account in view that has none reads an empty style with.
  /// `id: null`. Needs `view`.
  ///
  /// # Errors.
  ///
  /// 404 for an account that is missing, not the owner's, or outside a delegate's account limit.
  ///
  /// [id] - Account ID.
  @GET('/api/v1/accounts/{id}/writing-style')
  Future<WritingStyleDto> getWritingStyle({
    @Path('id') required String id,
  });

  /// Stores an account's writing style, replacing the earlier one. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for an account that is missing, not the owner's, or outside a delegate's account limit.
  ///
  /// [id] - Account ID.
  @PUT('/api/v1/accounts/{id}/writing-style')
  Future<WritingStyleDto> putWritingStyle({
    @Path('id') required String id,
    @Body() required PutWritingStyleRequest body,
  });

  /// The brand profile. An owner who wrote none reads an empty profile with `id: null`, so a.
  /// client renders one form either way. Needs `view`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  @GET('/api/v1/brand-profile')
  Future<BrandProfileDto> getBrandProfile();

  /// Stores the brand profile, replacing the earlier one. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` with `field: "examples"` for more than five examples.
  @PUT('/api/v1/brand-profile')
  Future<BrandProfileDto> putBrandProfile({
    @Body() required PutBrandProfileRequest body,
  });

  /// The owner's campaigns, newest first. Archived ones appear only with `include_archived`.
  /// Needs `view`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  ///
  /// [includeArchived] - Include archived campaigns (default false).
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/campaigns')
  Future<PageCampaignDto> listCampaigns({
    @Query('include_archived') bool? includeArchived,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Creates a campaign. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` with `field: "ends_on"` when it ends before it starts.
  @POST('/api/v1/campaigns')
  Future<CampaignDto> createCampaign({
    @Body() required CreateCampaignRequest body,
  });

  /// Deletes a campaign. Its posts stay and lose the campaign. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for a missing campaign.
  ///
  /// [id] - Campaign ID.
  @DELETE('/api/v1/campaigns/{id}')
  Future<void> deleteCampaign({
    @Path('id') required String id,
  });

  /// Changes a campaign: only the fields sent, and `null` clears a date or the color.
  /// `archived` archives or restores it. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for a missing campaign; 422 `validation_failed` with `field: "ends_on"` when the result.
  /// ends before it starts.
  ///
  /// [id] - Campaign ID.
  @PATCH('/api/v1/campaigns/{id}')
  Future<CampaignDto> patchCampaign({
    @Path('id') required String id,
    @Body() required PatchCampaignRequest body,
  });

  /// The owner's target groups, by name, with the members the caller can see. Needs `view`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/target-groups')
  Future<PageTargetGroupDto> listTargetGroups({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Creates a target group. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope` for a member outside a delegate's account limit; 404 for an.
  /// account that is not the owner's.
  @POST('/api/v1/target-groups')
  Future<TargetGroupDto> createTargetGroup({
    @Body() required TargetGroupRequest body,
  });

  /// Renames a target group and replaces its members. Members a limited delegate cannot see.
  /// stay. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for a missing group; 403 `account_not_in_scope` for a member outside the caller's limit.
  ///
  /// [id] - Target group ID.
  @PUT('/api/v1/target-groups/{id}')
  Future<TargetGroupDto> replaceTargetGroup({
    @Path('id') required String id,
    @Body() required TargetGroupRequest body,
  });

  /// Deletes a target group. Its accounts and the posts sent through it stay. Needs `edit`.
  ///
  /// # Errors.
  ///
  /// 404 for a missing group.
  ///
  /// [id] - Target group ID.
  @DELETE('/api/v1/target-groups/{id}')
  Future<void> deleteTargetGroup({
    @Path('id') required String id,
  });
}
