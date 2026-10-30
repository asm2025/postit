// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'notification_dto.dart';

part 'page_notification_dto.g.dart';

/// One page of results with its position in the full result set.
@JsonSerializable()
class PageNotificationDto {
  const PageNotificationDto({
    required this.data,
    required this.page,
    required this.pageSize,
    required this.total,
  });
  
  factory PageNotificationDto.fromJson(Map<String, Object?> json) => _$PageNotificationDtoFromJson(json);
  
  /// The items on this page.
  final List<NotificationDto> data;

  /// 1-based number of this page.
  final int page;

  /// Items per page requested.
  @JsonKey(name: 'page_size')
  final int pageSize;

  /// Total number of items across all pages.
  final int total;

  Map<String, Object?> toJson() => _$PageNotificationDtoToJson(this);
}
