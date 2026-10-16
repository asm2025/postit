// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'error_code.dart';

part 'problem_details.g.dart';

/// The RFC 9457 `application/problem+json` body every error response carries.
@JsonSerializable()
class ProblemDetails {
  const ProblemDetails({
    required this.code,
    required this.status,
    required this.title,
    required this.type,
    this.detail,
    this.requestId,
  });
  
  factory ProblemDetails.fromJson(Map<String, Object?> json) => _$ProblemDetailsFromJson(json);
  
  /// Machine-readable code.
  final ErrorCode code;

  /// Explanation specific to this occurrence.
  final String? detail;

  /// The `X-Request-Id` of the failed request, for support.
  @JsonKey(name: 'request_id')
  final String? requestId;

  /// The HTTP status code.
  final int status;

  /// Short summary of the problem type.
  final String title;

  /// Always `about:blank`; `code` is the discriminator.
  final String type;

  Map<String, Object?> toJson() => _$ProblemDetailsToJson(this);
}
