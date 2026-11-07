// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'create_campaign_request.g.dart';

/// A campaign to create.
@JsonSerializable()
class CreateCampaignRequest {
  const CreateCampaignRequest({
    required this.name,
    this.color,
    this.endsOn,
    this.startsOn,
  });
  
  factory CreateCampaignRequest.fromJson(Map<String, Object?> json) => _$CreateCampaignRequestFromJson(json);
  
  /// A display color.
  final String? color;

  /// The last day it runs; not before `starts_on` (`validation_failed`, `field: "ends_on"`).
  @JsonKey(name: 'ends_on')
  final DateTime? endsOn;

  /// Its name.
  final String name;

  /// The first day it runs.
  @JsonKey(name: 'starts_on')
  final DateTime? startsOn;

  Map<String, Object?> toJson() => _$CreateCampaignRequestToJson(this);
}
