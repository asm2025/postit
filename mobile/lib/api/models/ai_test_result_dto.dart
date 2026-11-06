// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_failure_reason_dto.dart';

part 'ai_test_result_dto.g.dart';

/// What a connection test found.
@JsonSerializable()
class AiTestResultDto {
  const AiTestResultDto({
    required this.ok,
    this.modelsFound,
    this.reason,
  });
  
  factory AiTestResultDto.fromJson(Map<String, Object?> json) => _$AiTestResultDtoFromJson(json);
  
  /// How many models the provider's live list holds, when it was read.
  @JsonKey(name: 'models_found')
  final int? modelsFound;

  /// Whether the provider answered.
  final bool ok;

  /// Why not.
  final AiFailureReasonDto? reason;

  Map<String, Object?> toJson() => _$AiTestResultDtoToJson(this);
}
