// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'data5.dart';

part 'page_media_dto.g.dart';

/// One page of results with its position in the full result set.
@JsonSerializable()
class PageMediaDto {
  const PageMediaDto({
    required this.data,
    required this.page,
    required this.pageSize,
    required this.total,
  });
  
  factory PageMediaDto.fromJson(Map<String, Object?> json) => _$PageMediaDtoFromJson(json);
  
  /// The items on this page.
  final List<Data5> data;

  /// 1-based number of this page.
  final int page;

  /// Items per page requested.
  @JsonKey(name: 'page_size')
  final int pageSize;

  /// Total number of items across all pages.
  final int total;

  Map<String, Object?> toJson() => _$PageMediaDtoToJson(this);
}
