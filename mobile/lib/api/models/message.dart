// coverage:ignore-file
// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, unused_import, invalid_annotation_target, unnecessary_import

import 'package:json_annotation/json_annotation.dart';

part 'message.g.dart';

/// A message key and its English text. Clients translate known keys and fall back to `en`.
@JsonSerializable()
class Message {
  const Message({
    required this.en,
    required this.key,
  });
  
  factory Message.fromJson(Map<String, Object?> json) => _$MessageFromJson(json);
  
  /// English default shown when no translation exists.
  final String en;

  /// Stable key, namespaced by platform ID for agent text (e.g. `tiktok.privacy.label`).
  final String key;

  Map<String, Object?> toJson() => _$MessageToJson(this);
}
