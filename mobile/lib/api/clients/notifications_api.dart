// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:dio/dio.dart';
import 'package:retrofit/retrofit.dart';

import '../models/mark_read_request.dart';
import '../models/mark_read_response.dart';
import '../models/page_notification_dto.dart';
import '../models/unread_count_dto.dart';

part 'notifications_api.g.dart';

@RestApi()
abstract class NotificationsApi {
  factory NotificationsApi(Dio dio, {String? baseUrl}) = _NotificationsApi;

  /// The caller's notifications, newest first.
  ///
  /// # Errors.
  ///
  /// 422 for invalid paging.
  ///
  /// [page] - 1-based page number (default 1).
  ///
  /// [pageSize] - Items per page, 1-100 (default 20).
  @GET('/api/v1/notifications')
  Future<PageNotificationDto> listNotifications({
    @Query('page') int? page,
    @Query('page_size') int? pageSize,
  });

  /// Marks the caller's notifications read: the named ones, or all of them. IDs that are not the.
  /// caller's are ignored.
  ///
  /// # Errors.
  ///
  /// 422 unless the body names exactly one of `ids` (1 to 200) or `all: true`.
  @POST('/api/v1/notifications/read')
  Future<MarkReadResponse> markNotificationsRead({
    @Body() required MarkReadRequest body,
  });

  /// How many notifications the caller has not read. Answers 304 with no body when.
  /// `If-None-Match` holds the current `ETag`, so the bell can poll cheaply.
  ///
  /// # Errors.
  ///
  /// None beyond authentication.
  @GET('/api/v1/notifications/unread-count')
  Future<UnreadCountDto> unreadNotificationCount();
}
