// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'campaign_dto.g.dart';

/// A campaign.
@JsonSerializable()
class CampaignDto {
  const CampaignDto({
    required this.archived,
    required this.createdAt,
    required this.id,
    required this.name,
    required this.updatedAt,
    this.color,
    this.endsOn,
    this.startsOn,
  });
  
  factory CampaignDto.fromJson(Map<String, Object?> json) => _$CampaignDtoFromJson(json);
  
  /// Whether it is hidden from the default list.
  final bool archived;

  /// A display color, as the client chose it.
  final String? color;

  /// When it was created.
  @JsonKey(name: 'created_at')
  final DateTime createdAt;

  /// The last day it runs.
  @JsonKey(name: 'ends_on')
  final DateTime? endsOn;

  /// The campaign.
  final String id;

  /// Its name.
  final String name;

  /// The first day it runs.
  @JsonKey(name: 'starts_on')
  final DateTime? startsOn;

  /// When it last changed.
  @JsonKey(name: 'updated_at')
  final DateTime updatedAt;

  Map<String, Object?> toJson() => _$CampaignDtoToJson(this);
}
