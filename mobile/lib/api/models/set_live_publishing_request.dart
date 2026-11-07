// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'set_live_publishing_request.g.dart';

/// Turning live publishing on or off.
@JsonSerializable()
class SetLivePublishingRequest {
  const SetLivePublishingRequest({
    required this.live,
  });
  
  factory SetLivePublishingRequest.fromJson(Map<String, Object?> json) => _$SetLivePublishingRequestFromJson(json);
  
  /// Whether live publishing is on.
  final bool live;

  Map<String, Object?> toJson() => _$SetLivePublishingRequestToJson(this);
}
