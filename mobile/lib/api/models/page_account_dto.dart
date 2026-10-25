// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'data.dart';

part 'page_account_dto.g.dart';

/// One page of results with its position in the full result set.
@JsonSerializable()
class PageAccountDto {
  const PageAccountDto({
    required this.data,
    required this.page,
    required this.pageSize,
    required this.total,
  });
  
  factory PageAccountDto.fromJson(Map<String, Object?> json) => _$PageAccountDtoFromJson(json);
  
  /// The items on this page.
  final List<Data> data;

  /// 1-based number of this page.
  final int page;

  /// Items per page requested.
  @JsonKey(name: 'page_size')
  final int pageSize;

  /// Total number of items across all pages.
  final int total;

  Map<String, Object?> toJson() => _$PageAccountDtoToJson(this);
}
