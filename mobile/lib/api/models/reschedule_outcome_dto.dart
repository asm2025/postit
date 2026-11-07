// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'reschedule_outcome_dto.g.dart';

/// What a reschedule moved.
@JsonSerializable()
class RescheduleOutcomeDto {
  const RescheduleOutcomeDto({
    required this.failed,
    required this.moved,
  });
  
  factory RescheduleOutcomeDto.fromJson(Map<String, Object?> json) => _$RescheduleOutcomeDtoFromJson(json);
  
  /// Natively scheduled uploads the platform did not move; they keep their old time.
  final List<String> failed;

  /// Deliveries now at the new time.
  final List<String> moved;

  Map<String, Object?> toJson() => _$RescheduleOutcomeDtoToJson(this);
}
