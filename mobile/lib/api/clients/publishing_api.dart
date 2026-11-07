// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/calendar_dto.dart';
import '../models/cancel_report_dto.dart';
import '../models/delivery_dto.dart';
import '../models/job_status.dart';
import '../models/page_publish_job_summary_dto.dart';
import '../models/publish_job_dto.dart';
import '../models/publish_request.dart';
import '../models/reschedule_outcome_dto.dart';
import '../models/reschedule_request.dart';
import '../models/resolve_request.dart';
import '../models/retry_request.dart';

part 'publishing_api.g.dart';

@RestApi()
abstract class PublishingApi {
  factory PublishingApi(Dio dio, {String? baseUrl}) = _PublishingApi;

  /// Live jobs due in `[from, to)`, soonest first, with their campaign color, the first 80.
  /// graphemes of the post's first line, and the accounts the caller can see. Dry runs are not.
  /// plans and are left out; so are jobs with no delivery inside a delegate's limit. Needs `view`.
  /// The `ETag` is a hash of the body; `If-None-Match` with it answers 304.
  ///
  /// # Errors.
  ///
  /// 422 `validation_failed` (`field: "to"`) when `to` is not after `from` or the range is.
  /// longer than 62 days.
  ///
  /// [from] - The first instant of the range, RFC 3339.
  ///
  /// [to] - The first instant after the range, RFC 3339; later than `from` by at most 62 days.
  @GET('/api/v1/calendar')
  Future<CalendarDto> getCalendar({
    @Query('from') required DateTime from,
    @Query('to') required DateTime to,
  });

  /// One delivery: its state, why it is parked, the platform's ID and URL, the last error's code.
  /// and class (never a platform's message), and the progress of a direct send. Needs `view`.
  ///
  /// # Errors.
  ///
  /// 404 for a missing delivery or one on an account outside a delegate's limit.
  ///
  /// [id] - Delivery ID.
  @GET('/api/v1/deliveries/{id}')
  Future<DeliveryDto> getDelivery({
    @Path('id') required String id,
  });

  /// Cancels one delivery. Needs `publish` and the delivery's account inside the caller's limit.
  ///
  /// # Errors.
  ///
  /// 404 for a missing delivery or one outside a delegate's limit.
  ///
  /// [id] - Delivery ID.
  @POST('/api/v1/deliveries/{id}/cancel')
  Future<CancelReportDto> cancelDelivery({
    @Path('id') required String id,
  });

  /// Settles a delivery whose outcome nobody can verify: `published` if the post is live, `failed`.
  /// if it is not and will not be retried. Needs `publish`. Answers the delivery.
  ///
  /// # Errors.
  ///
  /// 404; 409 `not_resolvable` for a delivery in a state the caller's word cannot settle.
  ///
  /// [id] - Delivery ID.
  @POST('/api/v1/deliveries/{id}/resolve')
  Future<DeliveryDto> resolveDelivery({
    @Path('id') required String id,
    @Body() required ResolveRequest body,
  });

  /// Sends a failed or stuck delivery again. Needs `publish`, `confirm: true`, and, for a.
  /// delivery whose outcome is unknown, `acknowledge_outcome_unknown: true`: the post may already.
  /// be live. Answers the delivery.
  ///
  /// # Errors.
  ///
  /// 400 `confirmation_required` or `acknowledgment_required`; 404; 409 `not_retryable`,.
  /// `job_active`.
  ///
  /// [id] - Delivery ID.
  @POST('/api/v1/deliveries/{id}/retry')
  Future<DeliveryDto> retryDelivery({
    @Path('id') required String id,
    @Body() required RetryRequest body,
  });

  /// Publishes a saved post to `targets`, now or at `at`. A dry run needs `edit` and only.
  /// simulates; a live publish needs `publish` and `confirm: true`. Needs `If-Match` with the.
  /// post's `ETag`, so what was reviewed is what goes out. Answers 201 with the job.
  ///
  /// # Errors.
  ///
  /// 400 `confirmation_required`; 403 `account_not_in_scope` for a target outside a delegate's.
  /// limit; 404; 409 `post_locked`, `job_active`, `platform_disabled`; 412 `version_conflict`;.
  /// 422 `validation_failed` with every violation; 428 `precondition_required`; 503.
  /// `platform_unavailable`.
  ///
  /// [id] - Post ID.
  @POST('/api/v1/posts/{id}/publish')
  Future<PublishJobDto> publishPost({
    @Path('id') required String id,
    @Body() required PublishRequest body,
  });

  /// The owner's publish jobs, newest first, without their deliveries. A delegate with a.
  /// limited account list sees a job when one of its deliveries is inside the limit. Needs `view`.
  ///
  /// # Errors.
  ///
  /// None of its own.
  ///
  /// [status] - Only jobs in this status.
  ///
  /// [postId] - Only jobs of this post.
  ///
  /// [campaignId] - Only jobs of posts in this campaign.
  ///
  /// [dryRun] - Only dry runs (`true`) or only live jobs (`false`).
  ///
  /// [from] - Only jobs scheduled at or after this time (RFC 3339).
  ///
  /// [to] - Only jobs scheduled before this time (RFC 3339).
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/publish-jobs')
  Future<PagePublishJobSummaryDto> listPublishJobs({
    @Query('status') JobStatus? status,
    @Query('post_id') String? postId,
    @Query('campaign_id') String? campaignId,
    @Query('dry_run') bool? dryRun,
    @Query('from') DateTime? from,
    @Query('to') DateTime? to,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// One job with the deliveries the caller can see and the progress of direct sends. Needs.
  /// `view`. `If-None-Match` with the current `ETag` answers 304.
  ///
  /// # Errors.
  ///
  /// 404 for a missing job, or one with no delivery inside a delegate's limit.
  ///
  /// [id] - Job ID.
  @GET('/api/v1/publish-jobs/{id}')
  Future<PublishJobDto> getPublishJob({
    @Path('id') required String id,
  });

  /// Moves a job that has not started to a new time. Needs `publish`, the whole job inside the.
  /// caller's limit, and `If-Match` with the job's `ETag`.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope`; 404; 409 `not_reschedulable`; 412; 422 (a schedule in the past.
  /// or one that does not resolve); 428.
  ///
  /// [id] - Job ID.
  @PATCH('/api/v1/publish-jobs/{id}')
  Future<RescheduleOutcomeDto> reschedulePublishJob({
    @Path('id') required String id,
    @Body() required RescheduleRequest body,
  });

  /// Cancels every delivery of a job that has not ended. Needs `publish` and the whole job.
  /// inside the caller's limit.
  ///
  /// # Errors.
  ///
  /// 403 `account_not_in_scope`; 404.
  ///
  /// [id] - Job ID.
  @POST('/api/v1/publish-jobs/{id}/cancel')
  Future<CancelReportDto> cancelPublishJob({
    @Path('id') required String id,
  });
}
