// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'target_report_dto.dart';
import 'violation.dart';

part 'validation_dto.g.dart';

/// The verdict of a validate call. The call itself succeeded either way, so it answers 200.
@JsonSerializable()
class ValidationDto {
  const ValidationDto({
    required this.targets,
    required this.valid,
    required this.violations,
  });
  
  factory ValidationDto.fromJson(Map<String, Object?> json) => _$ValidationDtoFromJson(json);
  
  /// One entry per target.
  final List<TargetReportDto> targets;

  /// Whether a publish of the same content, targets, and time would be accepted.
  final bool valid;

  /// Violations that belong to no target: the schedule's.
  final List<Violation> violations;

  Map<String, Object?> toJson() => _$ValidationDtoToJson(this);
}
