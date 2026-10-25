// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/audit_event_kind_dto.dart';
import '../models/job_action_response.dart';
import '../models/job_detail_dto.dart';
import '../models/job_state_dto.dart';
import '../models/job_stats_dto.dart';
import '../models/page_audit_event_dto.dart';
import '../models/page_job_dto.dart';
import '../models/recurring_job_dto.dart';

part 'admin_api.g.dart';

@RestApi()
abstract class AdminApi {
  factory AdminApi(Dio dio, {String? baseUrl}) = _AdminApi;

  /// Lists audit events, newest first, with optional filters.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 422 for invalid filters or paging.
  ///
  /// [kind] - An audit event kind, e.g. `user_approved`.
  ///
  /// [from] - Inclusive lower bound (RFC 3339).
  ///
  /// [to] - Inclusive upper bound (RFC 3339).
  ///
  /// [actorUserId] - Only events performed by this user.
  ///
  /// [subjectUserId] - Only events about this user.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/admin/audit')
  Future<PageAuditEventDto> listAuditEvents({
    @Query('kind') AuditEventKindDto? kind,
    @Query('from') DateTime? from,
    @Query('to') DateTime? to,
    @Query('actor_user_id') String? actorUserId,
    @Query('subject_user_id') String? subjectUserId,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Lists jobs, newest first, with optional filters.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 422 for invalid filters or paging.
  ///
  /// [state] - `queued`, `scheduled`, `running`, `succeeded`, `failed`, `dead`, `killed` or `cancelled`.
  ///
  /// [jobType] - Only jobs of this type.
  ///
  /// [from] - Inclusive lower bound (RFC 3339) on the state's relevant timestamp.
  ///
  /// [to] - Inclusive upper bound (RFC 3339) on the state's relevant timestamp.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/admin/jobs')
  Future<PageJobDto> listJobs({
    @Query('state') JobStateDto? state,
    @Query('job_type') String? jobType,
    @Query('from') DateTime? from,
    @Query('to') DateTime? to,
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// The recurring jobs with their last and next runs.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins.
  @GET('/api/v1/admin/jobs/recurring')
  Future<List<RecurringJobDto>> listRecurringJobs();

  /// Runs a recurring job now, outside its schedule.
  ///
  /// With an `Idempotency-Key`, a repeated request (a client or network retry) answers with.
  /// the first run's response instead of starting a second run.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 404 for a name that is not a recurring job.
  ///
  /// [name] - Recurring job name.
  ///
  /// [idempotencyKey] - Replays the first response for a repeated key instead of starting a second run (24 h).
  @POST('/api/v1/admin/jobs/recurring/{name}/trigger')
  Future<JobActionResponse> triggerRecurringJob({
    @Path('name') required String name,
    @Header('Idempotency-Key') String? idempotencyKey,
  });

  /// Job counts per state, type and queue, and attempt throughput over a window.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 422 for a window outside 1-168 hours.
  ///
  /// [windowHours] - Throughput window in hours, 1-168 (default 24).
  @GET('/api/v1/admin/jobs/stats')
  Future<JobStatsDto> getJobStats({
    @Query('window_hours') int? windowHours,
  });

  /// One job with its attempt history.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins and 404 for an unknown job.
  ///
  /// [id] - Job ID.
  @GET('/api/v1/admin/jobs/{id}')
  Future<JobDetailDto> getJob({
    @Path('id') required String id,
  });

  /// Deletes a finished job and its attempt history.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins, 404 for an unknown job and 409 when the job has not finished.
  /// (`job_action_not_allowed`).
  ///
  /// [id] - Job ID.
  @DELETE('/api/v1/admin/jobs/{id}')
  Future<void> deleteJob({
    @Path('id') required String id,
  });

  /// Cancels a queued or scheduled job before it runs.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins, 404 for an unknown job and 409 when the job is not queued.
  /// (`job_action_not_allowed`).
  ///
  /// [id] - Job ID.
  @POST('/api/v1/admin/jobs/{id}/cancel')
  Future<void> cancelJob({
    @Path('id') required String id,
  });

  /// Re-runs a failed, dead, killed or cancelled job as a new job.
  ///
  /// # Errors.
  ///
  /// Returns 403 for non-admins, 404 for an unknown job and 409 when the job's state or type.
  /// does not allow a retry (`job_action_not_allowed`).
  ///
  /// [id] - Job ID.
  @POST('/api/v1/admin/jobs/{id}/retry')
  Future<JobActionResponse> retryJob({
    @Path('id') required String id,
  });
}
