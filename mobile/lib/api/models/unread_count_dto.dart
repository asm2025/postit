// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'unread_count_dto.g.dart';

/// The bell badge.
@JsonSerializable()
class UnreadCountDto {
  const UnreadCountDto({
    required this.count,
  });
  
  factory UnreadCountDto.fromJson(Map<String, Object?> json) => _$UnreadCountDtoFromJson(json);
  
  /// Unread notifications.
  final int count;

  Map<String, Object?> toJson() => _$UnreadCountDtoToJson(this);
}
