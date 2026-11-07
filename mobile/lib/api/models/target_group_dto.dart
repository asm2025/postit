// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'target_group_dto.g.dart';

/// A named set of accounts a post can be sent to in one pick. A delegate with a limited.
/// account list sees only the members inside the limit.
@JsonSerializable()
class TargetGroupDto {
  const TargetGroupDto({
    required this.accountIds,
    required this.id,
    required this.name,
    required this.updatedAt,
  });
  
  factory TargetGroupDto.fromJson(Map<String, Object?> json) => _$TargetGroupDtoFromJson(json);
  
  /// Its member accounts the caller can see.
  @JsonKey(name: 'account_ids')
  final List<String> accountIds;

  /// The group.
  final String id;

  /// Its name.
  final String name;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$TargetGroupDtoToJson(this);
}
