// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

/// Why a provider call failed. The provider's own message is never passed on, since it may.
/// echo the request.
@JsonEnum()
enum AiFailureReasonDto {
  @JsonValue('key_rejected')
  keyRejected('key_rejected'),
  @JsonValue('model_not_found')
  modelNotFound('model_not_found'),
  @JsonValue('rate_limited')
  rateLimited('rate_limited'),
  @JsonValue('bad_request')
  badRequest('bad_request'),
  @JsonValue('unavailable')
  unavailable('unavailable'),
  @JsonValue('timeout')
  timeout('timeout'),
  @JsonValue('address_refused')
  addressRefused('address_refused'),
  @JsonValue('invalid_response')
  invalidResponse('invalid_response'),
  @JsonValue('invalid_output')
  invalidOutput('invalid_output'),
  /// Default value for all unparsed values, allows backward compatibility when adding new values on the backend.
  $unknown(null);

  const AiFailureReasonDto(this.json);

  factory AiFailureReasonDto.fromJson(String json) => values.firstWhere(
        (e) => e.json == json,
        orElse: () => $unknown,
      );

  final String? json;

  @override
  String toString() => json?.toString() ?? super.toString();
  /// Returns all defined enum values excluding the $unknown value.
  static List<AiFailureReasonDto> get $valuesDefined => values.where((value) => value != $unknown).toList();
}
