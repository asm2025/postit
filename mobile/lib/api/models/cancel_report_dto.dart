// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'cancel_report_dto.g.dart';

/// What a cancel did to each delivery that had not ended.
@JsonSerializable()
class CancelReportDto {
  const CancelReportDto({
    required this.cancelled,
    required this.notCancellable,
    required this.requested,
  });
  
  factory CancelReportDto.fromJson(Map<String, Object?> json) => _$CancelReportDtoFromJson(json);
  
  /// Now cancelled.
  final List<String> cancelled;

  /// Left as they were: an unknown outcome (resolve it instead), a platform still.
  /// processing, or a native upload the agent could not delete.
  @JsonKey(name: 'not_cancellable')
  final List<String> notCancellable;

  /// Running: the worker stops at its next safe checkpoint.
  final List<String> requested;

  Map<String, Object?> toJson() => _$CancelReportDtoToJson(this);
}
