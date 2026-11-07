// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'publishing_settings_dto.g.dart';

/// The live publishing switch.
@JsonSerializable()
class PublishingSettingsDto {
  const PublishingSettingsDto({
    required this.live,
    required this.nativeScheduledCount,
  });
  
  factory PublishingSettingsDto.fromJson(Map<String, Object?> json) => _$PublishingSettingsDtoFromJson(json);
  
  /// Whether live publishing is on. While off, live deliveries park and dry runs still run.
  final bool live;

  /// Uploads that platforms hold for a later publish time: the switch cannot stop them.
  @JsonKey(name: 'native_scheduled_count')
  final int nativeScheduledCount;

  Map<String, Object?> toJson() => _$PublishingSettingsDtoToJson(this);
}
