// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'vault_rotate_response.g.dart';

/// A vault rotation that was queued.
@JsonSerializable()
class VaultRotateResponse {
  const VaultRotateResponse({
    required this.jobId,
  });
  
  factory VaultRotateResponse.fromJson(Map<String, Object?> json) => _$VaultRotateResponseFromJson(json);
  
  /// The queued `vault_rotate` job; follow it in the job console.
  @JsonKey(name: 'job_id')
  final String jobId;

  Map<String, Object?> toJson() => _$VaultRotateResponseToJson(this);
}
