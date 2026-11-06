// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'remove_media_fix_type.dart';
import 'suggested_fix.dart';

part 'remove_media_fix.g.dart';

/// Remove the media item at `index`, or all media when it is absent.
@JsonSerializable()
class RemoveMediaFix {
  const RemoveMediaFix({
    required this.type,
    this.index,
  });
  
  factory RemoveMediaFix.fromJson(Map<String, Object?> json) => _$RemoveMediaFixFromJson(json);
  
  /// Zero-based position; absent for every item.
  final int? index;
  final RemoveMediaFixType type;

  Map<String, Object?> toJson() => _$RemoveMediaFixToJson(this);
}
