// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'retry_request.g.dart';

/// Sending a failed or stuck delivery again.
@JsonSerializable()
class RetryRequest {
  const RetryRequest({
    this.acknowledgeOutcomeUnknown,
    this.confirm,
  });
  
  factory RetryRequest.fromJson(Map<String, Object?> json) => _$RetryRequestFromJson(json);
  
  /// For a delivery whose outcome is unknown: the caller accepts that the post may already.
  /// be live.
  @JsonKey(name: 'acknowledge_outcome_unknown')
  final bool? acknowledgeOutcomeUnknown;

  /// The caller's confirmation.
  final bool? confirm;

  Map<String, Object?> toJson() => _$RetryRequestToJson(this);
}
