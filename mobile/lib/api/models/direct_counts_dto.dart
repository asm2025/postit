// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'direct_counts_dto.g.dart';

/// How far a direct (email) delivery has got.
@JsonSerializable()
class DirectCountsDto {
  const DirectCountsDto({
    required this.audience,
    required this.bounced,
    required this.failed,
    required this.queued,
    required this.sent,
    required this.skipped,
  });
  
  factory DirectCountsDto.fromJson(Map<String, Object?> json) => _$DirectCountsDtoFromJson(json);
  
  /// Recipients the delivery was materialized for.
  final int audience;

  /// Recipients whose address bounced after the provider accepted it.
  final int bounced;

  /// Recipients whose send failed, including cancelled ones.
  final int failed;

  /// Recipients still waiting.
  final int queued;

  /// Recipients the provider accepted.
  final int sent;

  /// Recipients skipped because the address is suppressed.
  final int skipped;

  Map<String, Object?> toJson() => _$DirectCountsDtoToJson(this);
}
