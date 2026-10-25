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
    this.field,
    this.missingScopes,
    this.requestId,
    this.violations,
  });
  
  factory ProblemDetails.fromJson(Map<String, Object?> json) => _$ProblemDetailsFromJson(json);
  
  /// Machine-readable code.
  final ErrorCode code;

  /// Explanation specific to this occurrence.
  final String? detail;

  /// `credentials_invalid`: the credential field at fault, when known.
  final String? field;

  /// `credentials_invalid`: scopes the credentials lack, when the platform says.
  @JsonKey(name: 'missing_scopes')
  final List<String>? missingScopes;

  /// The `X-Request-Id` of the failed request, for support.
  @JsonKey(name: 'request_id')
  final String? requestId;

  /// The HTTP status code.
  final int status;

  /// Short summary of the problem type.
  final String title;

  /// Always `about:blank`; `code` is the discriminator.
  final String type;

  /// `validation_failed` on account routes: every rule the input broke, by field.
  final List<dynamic>? violations;

  Map<String, Object?> toJson() => _$ProblemDetailsToJson(this);
}
