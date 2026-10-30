// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'mark_read_request.g.dart';

/// Which notifications to mark read: exactly one of `ids` (1 to 200 of the caller's own;.
/// others are ignored) or `all: true`.
@JsonSerializable()
class MarkReadRequest {
  const MarkReadRequest({
    this.all,
    this.ids,
  });
  
  factory MarkReadRequest.fromJson(Map<String, Object?> json) => _$MarkReadRequestFromJson(json);
  
  /// Every unread notification of the caller. Only `true` selects anything.
  final bool? all;

  /// These notifications.
  final List<String>? ids;

  Map<String, Object?> toJson() => _$MarkReadRequestToJson(this);
}
