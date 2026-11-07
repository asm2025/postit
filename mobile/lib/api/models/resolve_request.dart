// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'resolved_outcome.dart';

part 'resolve_request.g.dart';

/// Settling a delivery whose outcome nobody can verify.
@JsonSerializable()
class ResolveRequest {
  const ResolveRequest({
    required this.outcome,
  });
  
  factory ResolveRequest.fromJson(Map<String, Object?> json) => _$ResolveRequestFromJson(json);
  
  /// Whether the post is live (`published`) or not and will not be retried (`failed`).
  final ResolvedOutcome outcome;

  Map<String, Object?> toJson() => _$ResolveRequestToJson(this);
}
