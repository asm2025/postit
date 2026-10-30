// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'mark_read_response.g.dart';

/// How many notifications changed.
@JsonSerializable()
class MarkReadResponse {
  const MarkReadResponse({
    required this.updated,
  });
  
  factory MarkReadResponse.fromJson(Map<String, Object?> json) => _$MarkReadResponseFromJson(json);
  
  /// Notifications newly marked read.
  final int updated;

  Map<String, Object?> toJson() => _$MarkReadResponseToJson(this);
}
