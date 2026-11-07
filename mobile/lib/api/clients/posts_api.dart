// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/create_post_request.dart';
import '../models/page_post_summary_dto.dart';
import '../models/patch_post_request.dart';
import '../models/post_dto.dart';
import '../models/post_kind.dart';
import '../models/post_status_dto.dart';
import '../models/put_override_request.dart';
import '../models/validate_draft_request.dart';
import '../models/validate_post_request.dart';
import '../models/validation_dto.dart';

part 'posts_api.g.dart';

@RestApi()
abstract class PostsApi {
  factory PostsApi(Dio dio, {String? baseUrl}) = _PostsApi;

  /// The owner's posts, newest first, without media, targets, or overrides. Needs `view`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  ///
  /// [status] - Only posts in this status.
  ///
  /// [campaignId] - Only posts in this campaign.
  ///
  /// [kind] - Only posts of this kind.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/posts')
  Future<PagePostSummaryDto> listPosts({
    @Query('status') PostStatusDto? status,
    @Query('campaign_id') String? campaignId,
    @Query('kind') PostKind? kind,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Creates a draft with its media and targets. Needs `edit`. With `generation_id`, links the.
  /// AI generation it was saved from.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope` for a target outside a delegate's account limit; 404 for a.
  /// campaign, media asset, target account, or generation that is not the owner's (or a.
  /// generation already linked to a post); 422 `validation_failed` at `link`.
  @POST('/api/v1/posts')
  Future<PostDto> createPost({
    @Body() required CreatePostRequest body,
  });

  /// Checks an unsaved post for its targets with the same check a publish runs. Needs `edit`.
  /// It answers 200 with the verdict either way; violations are in `targets` and, for the.
  /// schedule, in the top-level `violations`. Charged to the publish bucket.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope` for a target outside a delegate's account limit; 404 for a target.
  /// account that is not the owner's; 422 `validation_failed` at `link`.
  @POST('/api/v1/posts/validate')
  Future<ValidationDto> validateDraft({
    @Body() required ValidateDraftRequest body,
  });

  /// One post with its media, targets, overrides, and the newest live delivery per account.
  /// Needs `view`. `If-None-Match` with the current `ETag` answers 304.
  ///
  /// # Errors.
  ///
  /// 404 for a missing post.
  ///
  /// [id] - Post ID.
  @GET('/api/v1/posts/{id}')
  Future<PostDto> getPost({
    @Path('id') required String id,
  });

  /// Deletes a post with its media links, targets, overrides, jobs, and deliveries. Needs `edit`,.
  /// and `publish` when the post has live history. Needs `If-Match`.
  ///
  /// # Errors.
  ///
  /// As `PATCH /posts/{id}`.
  ///
  /// [id] - Post ID.
  @DELETE('/api/v1/posts/{id}')
  Future<void> deletePost({
    @Path('id') required String id,
  });

  /// Changes a post. Needs `edit` and `If-Match`. Targets outside a limited delegate's account.
  /// limit stay. A post an unfinished or scheduled publish job holds is `post_locked`.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope` when an override or unfinished delivery belongs to an account.
  /// outside the delegate's limit; 404; 409 `post_locked`; 412 `version_conflict`; 428.
  /// `precondition_required`.
  ///
  /// [id] - Post ID.
  @PATCH('/api/v1/posts/{id}')
  Future<PostDto> patchPost({
    @Path('id') required String id,
    @Body() required PatchPostRequest body,
  });

  /// Sets the override for one account. Needs `edit` and `If-Match`; the post's version.
  /// changes, and the answer carries the new `ETag`.
  ///
  /// # Errors.
  ///
  /// As `PATCH /posts/{id}`; an account outside a delegate's limit, not the owner's, or.
  /// disconnected is 404.
  ///
  /// [id] - Post ID.
  ///
  /// [accountId] - Account ID.
  @PUT('/api/v1/posts/{id}/overrides/{account_id}')
  Future<PostDto> putPostOverride({
    @Path('id') required String id,
    @Path('account_id') required String accountId,
    @Body() required PutOverrideRequest body,
  });

  /// Removes the override for one account. Needs `edit` and `If-Match`.
  ///
  /// # Errors.
  ///
  /// As `PUT /posts/{id}/overrides/{account_id}`; an account with no override is 404.
  ///
  /// [id] - Post ID.
  ///
  /// [accountId] - Account ID.
  @DELETE('/api/v1/posts/{id}/overrides/{account_id}')
  Future<PostDto> deletePostOverride({
    @Path('id') required String id,
    @Path('account_id') required String accountId,
  });

  /// Checks a saved post, at its current version, for `targets` (its own by default) with the.
  /// same check a publish runs. Needs `edit`. Answers 200 with the verdict either way. Charged.
  /// to the publish bucket.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope` for a target outside a delegate's account limit; 404 for a.
  /// missing post or a target account that is not the owner's.
  ///
  /// [id] - Post ID.
  @POST('/api/v1/posts/{id}/validate')
  Future<ValidationDto> validatePost({
    @Path('id') required String id,
    @Body() required ValidatePostRequest body,
  });
}
