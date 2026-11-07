// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'post_override.dart';

part 'post_override_dto.g.dart';

/// A per-account override of a post.
@JsonSerializable()
class PostOverrideDto {
  const PostOverrideDto({
    required this.accountId,
    required this.options,
    required this.patch,
    required this.updatedAt,
  });
  
  factory PostOverrideDto.fromJson(Map<String, Object?> json) => _$PostOverrideDtoFromJson(json);
  
  /// The account it applies to.
  @JsonKey(name: 'account_id')
  final String accountId;

  /// Platform options for this account.
  final dynamic options;

  /// The fields that differ from the post.
  final PostOverride patch;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$PostOverrideDtoToJson(this);
}
