// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

import 'ai_protocol_dto.dart';

part 'ai_preset_dto.g.dart';

/// One entry of the preset catalog.
@JsonSerializable()
class AiPresetDto {
  const AiPresetDto({
    required this.available,
    required this.baseUrl,
    required this.guideUrl,
    required this.key,
    required this.local,
    required this.name,
    required this.protocol,
    required this.subscriptionNote,
  });
  
  factory AiPresetDto.fromJson(Map<String, Object?> json) => _$AiPresetDtoFromJson(json);
  
  /// Whether it can be added on this server: false for local presets while.
  /// `ai.allow_private_endpoints` is off.
  final bool available;

  /// The prefilled base URL.
  @JsonKey(name: 'base_url')
  final String baseUrl;

  /// Where the user gets a key.
  @JsonKey(name: 'guide_url')
  final String guideUrl;

  /// The preset key, such as `gemini`.
  final String key;

  /// A server on the user's own machine.
  final bool local;

  /// Display name.
  final String name;

  /// Wire protocol.
  final AiProtocolDto protocol;

  /// A consumer chat subscription from the same company cannot be used here; the.
  /// guide says so.
  @JsonKey(name: 'subscription_note')
  final bool subscriptionNote;

  Map<String, Object?> toJson() => _$AiPresetDtoToJson(this);
}
