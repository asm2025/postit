// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'patch_campaign_request.g.dart';

/// Changes to a campaign. Absent fields stay as they are; `null` clears a date or the color.
@JsonSerializable()
class PatchCampaignRequest {
  const PatchCampaignRequest({
    this.archived,
    this.color,
    this.endsOn,
    this.name,
    this.startsOn,
  });
  
  factory PatchCampaignRequest.fromJson(Map<String, Object?> json) => _$PatchCampaignRequestFromJson(json);
  
  /// Archive or restore the campaign.
  final bool? archived;

  /// A new display color, or `null` to clear it.
  final String? color;

  /// A new last day, or `null` to clear it.
  @JsonKey(name: 'ends_on')
  final DateTime? endsOn;

  /// A new name.
  final String? name;

  /// A new first day, or `null` to clear it.
  @JsonKey(name: 'starts_on')
  final DateTime? startsOn;

  Map<String, Object?> toJson() => _$PatchCampaignRequestToJson(this);
}
